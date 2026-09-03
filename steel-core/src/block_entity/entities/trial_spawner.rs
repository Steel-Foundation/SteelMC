//! Trial spawner block entity implementation.
//!
//! Port of vanilla `TrialSpawnerBlockEntity` + `TrialSpawner` +
//! `TrialSpawnerState` state machine. Trial spawners differ from regular mob
//! spawners: they detect nearby players, spawn a bounded wave of mobs, and
//! only enter their reward-ejection/cooldown cycle once every spawned mob has
//! been defeated.
//!
//! Steel differences:
//! - Ominous `OminousItemSpawner` entities are not implemented yet, so the
//!   ominous item-dispensing part of the active state is skipped.
//! - Spawn-potential entity NBT tags are applied for known gameplay flags
//!   (`IsBaby`) instead of full NBT loading; other tags are logged and
//!   dropped. Extracted vanilla configs only use `IsBaby` today.
//! - Spawned mobs' equipment drop chances are not zeroed (vanilla
//!   `SpawnData.equipment` carries `slot_drop_chances` per slot).

use std::sync::{Arc, Weak};
use uuid::Uuid;

use glam::DVec3;
use rand::RngExt as _;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_registry::RegistryExt;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, TrialSpawnerState};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::loot_table::LootContext;
use steel_registry::trial_spawner::{TrialSpawnerConfig, TrialSpawnerEntityTag};
use steel_registry::sound_events;
use steel_registry::{REGISTRY, vanilla_block_entity_types, vanilla_game_rules, vanilla_mob_effects};
use steel_utils::locks::SyncMutex;
use steel_utils::types::{Difficulty, UpdateFlags};
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, Identifier, WorldAabb};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::entity::{Entity, ENTITIES, EntitySpawnReason, LivingEntity as _, next_entity_id};
use steel_protocol::packets::game::SoundSource;

use crate::player::Player;
use crate::world::{ClipBlockShape, ClipFluid};
use crate::world::{LevelAccessor as _, World};

const NORMAL_CONFIG_TAG: &str = "normal_config";
const OMINOUS_CONFIG_TAG: &str = "ominous_config";
const REGISTERED_PLAYERS_TAG: &str = "registered_players";
const CURRENT_MOBS_TAG: &str = "current_mobs";
const COOLDOWN_ENDS_AT_TAG: &str = "cooldown_ends_at";
const NEXT_MOB_SPAWNS_AT_TAG: &str = "next_mob_spawns_at";
const TOTAL_MOBS_SPAWNED_TAG: &str = "total_mobs_spawned";
const SPAWN_DATA_TAG: &str = "spawn_data";
const EJECTING_LOOT_TABLE_TAG: &str = "ejecting_loot_table";

/// Vanilla `TrialSpawner.DEFAULT_TARGET_COOLDOWN_LENGTH`.
const TARGET_COOLDOWN_LENGTH: i64 = 36_000;
/// Vanilla `TrialSpawner.MAX_MOB_TRACKING_DISTANCE`.
const MAX_MOB_TRACKING_DISTANCE_SQR: f64 = 47.0 * 47.0;

/// Vanilla `TrialSpawnerStateData` mutable state.
struct SpawnerData {
    normal_config_key: Option<Identifier>,
    ominous_config_key: Option<Identifier>,
    detected_players: Vec<Uuid>,
    current_mobs: Vec<Uuid>,
    cooldown_ends_at: i64,
    next_mob_spawns_at: i64,
    total_mobs_spawned: i32,
    /// The spawned-entity payload of vanilla's next `SpawnData.entity`.
    next_spawn_entity: Option<NbtCompound>,
    ejecting_loot_table: Option<Identifier>,
}

impl SpawnerData {
    fn reset(&mut self) {
        self.current_mobs.clear();
        self.next_spawn_entity = None;
        self.reset_statistics();
    }

    fn reset_statistics(&mut self) {
        self.detected_players.clear();
        self.total_mobs_spawned = 0;
        self.next_mob_spawns_at = 0;
        self.cooldown_ends_at = 0;
    }
}

/// Trial spawner block entity.
pub struct TrialSpawnerBlockEntity {
    base: BlockEntityBase,
    data: SyncMutex<SpawnerData>,
}

// SAFETY: Steel owns this concrete block entity key.
unsafe impl DowncastType for TrialSpawnerBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/trial_spawner");
}

impl TrialSpawnerBlockEntity {
    /// Creates a new trial spawner block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            base: BlockEntityBase::new(
                &vanilla_block_entity_types::TRIAL_SPAWNER,
                level,
                pos,
                state,
            ),
            data: SyncMutex::new(SpawnerData {
                normal_config_key: None,
                ominous_config_key: None,
                detected_players: Vec::new(),
                current_mobs: Vec::new(),
                cooldown_ends_at: 0,
                next_mob_spawns_at: 0,
                total_mobs_spawned: 0,
                next_spawn_entity: None,
                ejecting_loot_table: None,
            }),
        }
    }

    /// Resolves the active config (vanilla `TrialSpawner.activeConfig`).
    #[must_use]
    pub fn active_config(&self, is_ominous: bool) -> &'static TrialSpawnerConfig {
        let data = self.data.lock();
        let key = if is_ominous {
            data.ominous_config_key.as_ref()
        } else {
            data.normal_config_key.as_ref()
        };
        key.and_then(|key| REGISTRY.trial_spawner_configs.by_key(key))
            .unwrap_or(&steel_registry::trial_spawner::DEFAULT)
    }

    /// Vanilla `TrialSpawnerStateData.hasMobToSpawn`.
    fn has_mob_to_spawn(
        &self,
        config: &TrialSpawnerConfig,
        next_spawn_entity: Option<&NbtCompound>,
    ) -> bool {
        next_spawn_entity.is_some_and(|entity| entity.string("id").is_some())
            || !config.spawn_potentials.is_empty()
    }

    /// Vanilla `TrialSpawner.canSpawnInLevel`.
    fn can_spawn_in_level(&self, world: &World) -> bool {
        world.get_game_rule(&vanilla_game_rules::SPAWNER_BLOCKS_WORK)
            && world.difficulty() != Difficulty::Peaceful
            && world.get_game_rule(&vanilla_game_rules::SPAWN_MOBS)
    }

    /// Vanilla `TrialSpawnerStateData.tryDetectPlayers`, including the
    /// bad-omen/trial-omen ominous activation. Detection is throttled to once
    /// every 20 ticks per position (vanilla `(pos.asLong + gameTime) % 20`).
    fn try_detect_players(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        config: &TrialSpawnerConfig,
        is_ominous: bool,
        in_cooldown: bool,
    ) {
        if (Self::pack_pos(pos) + world.game_time()) % 20 != 0 {
            return;
        }
        if in_cooldown && is_ominous {
            // Vanilla never re-detects players while an ominous spawner cools down.
            return;
        }

        let center = Self::center_of(pos);
        let range_sqr = f64::from(config.spawn_range) * f64::from(config.spawn_range);
        let mut candidates: Vec<Arc<Player>> = Vec::new();
        world.players.iter_players(|_, player| {
            if !player.is_spectator()
                && crate::entity::Entity::is_alive(&**player)
                && player.position().distance_squared(center) <= range_sqr
            {
                candidates.push(player.clone());
            }
            true
        });

        let searching_first_player = self.data.lock().detected_players.is_empty();
        if searching_first_player {
            candidates.retain(|player| Self::has_line_of_sight(world, player, pos));
        }

        // Vanilla checks the line-of-sight players for a trial/bad omen and
        // turns the spawner ominous on the first match.
        if !is_ominous
            && let Some(player) = candidates.iter().find_map(|player| {
                if player.has_mob_effect(vanilla_mob_effects::TRIAL_OMEN) {
                    Some(player.clone())
                } else if player.has_mob_effect(vanilla_mob_effects::BAD_OMEN) {
                    Some(player.clone())
                } else {
                    None
                }
            })
        {
            self.apply_ominous(world, pos, &player);
            return;
        }

        let mut data = self.data.lock();
        for player in candidates {
            if !data.detected_players.contains(&player.uuid()) {
                data.detected_players.push(player.uuid());
                // Vanilla delays the first spawn by 40 ticks after detection.
                data.next_mob_spawns_at = (world.game_time() + 40).max(data.next_mob_spawns_at);
                world.level_event(
                    if is_ominous { 3019 } else { 3013 },
                    pos,
                    data.detected_players.len() as i32,
                    None,
                );
            }
        }
    }

    /// Vanilla `TrialSpawner.applyOminous` + `resetAfterBecomingOminous`.
    fn apply_ominous(&self, world: &Arc<World>, pos: BlockPos, player: &Player) {
        let game_time = world.game_time();
        let config = self.active_config(true);

        {
            let data = self.data.lock();
            if let Some(omen) = player.mob_effect(vanilla_mob_effects::BAD_OMEN) {
                // Vanilla `transformBadOmenIntoTrialOmen`.
                player.remove_mob_effect(vanilla_mob_effects::BAD_OMEN);
                player.add_mob_effect(crate::entity::MobEffectInstance::with_duration(
                    vanilla_mob_effects::TRIAL_OMEN,
                    18_000 * (omen.amplifier() + 1),
                    0,
                ));
            }
            for uuid in &data.current_mobs {
                if let Some(entity) = world.get_entity_by_uuid(uuid) {
                    world.level_event(3012, entity.block_position(), 0, None);
                    entity.set_removed(crate::entity::RemovalReason::Discarded);
                }
            }
        }

        let next_state_id = self
            .get_block_state()
            .set_value(&BlockStateProperties::OMINOUS, true);
        world.set_block_state(pos, next_state_id, UpdateFlags::UPDATE_ALL);
        world.level_event(3020, pos, 1, None);

        let mut data = self.data.lock();
        data.current_mobs.clear();
        if !config.spawn_potentials.is_empty() {
            data.next_spawn_entity = None;
        }
        data.total_mobs_spawned = 0;
        data.next_mob_spawns_at = game_time + i64::from(config.ticks_between_spawn);
        data.cooldown_ends_at = game_time + config.ticks_between_item_spawners();
    }

    /// Vanilla `PlayerDetector` line-of-sight check: the clip from the player's
    /// eye to the spawner center must miss or hit the spawner block itself.
    fn has_line_of_sight(world: &World, player: &Player, pos: BlockPos) -> bool {
        let center = Self::center_of(pos);
        let eye = DVec3::new(player.position().x, player.get_eye_y(), player.position().z);
        let hit = world.clip(eye, center, ClipBlockShape::Visual, ClipFluid::None);
        hit.is_miss() || hit.block_pos == pos
    }

    fn center_of(pos: BlockPos) -> DVec3 {
        DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + 0.5,
            f64::from(pos.z()) + 0.5,
        )
    }

    fn pack_pos(pos: BlockPos) -> i64 {
        ((i64::from(pos.x()) & 0x3FF_FFFF) << 38)
            | ((i64::from(pos.z()) & 0x3FF_FFFF) << 12)
            | (i64::from(pos.y()) & 0xFFF)
    }

    /// Vanilla `TrialSpawner.spawnMob`.
    fn spawn_mob(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        config: &TrialSpawnerConfig,
        is_ominous: bool,
    ) -> Option<Uuid> {
        // Vanilla `getOrCreateNextSpawnData`: reuse or weighted-pick and store.
        let next_spawn_entity = {
            let mut data = self.data.lock();
            if data.next_spawn_entity.is_none() {
                data.next_spawn_entity = Self::pick_spawn_potential(config);
            }
            data.next_spawn_entity.clone()
        };
        let next_spawn_entity = next_spawn_entity?;

        let Some(raw_id) = next_spawn_entity.string("id").map(|s| s.to_string()) else {
            return None;
        };
        let Ok(key) = raw_id.parse::<Identifier>() else {
            return None;
        };
        let Some(entity_type) = REGISTRY.entity_types.by_key(&key) else {
            return None;
        };

        let mut rng = rand::rng();
        let spawn_pos = DVec3::new(
            f64::from(pos.x())
                + 0.5
                + (rng.random::<f64>() - rng.random::<f64>()) * f64::from(config.spawn_range),
            f64::from(pos.y()) + f64::from(rng.random_range(-1..=1)),
            f64::from(pos.z())
                + 0.5
                + (rng.random::<f64>() - rng.random::<f64>()) * f64::from(config.spawn_range),
        );

        if !Self::spawn_position_is_free(world, entity_type, spawn_pos) {
            return None;
        }
        if !Self::in_line_of_sight(world, pos, spawn_pos) {
            return None;
        }

        let Some(entity) = ENTITIES.create(
            entity_type,
            next_entity_id(),
            spawn_pos,
            Arc::downgrade(world),
        ) else {
            return None;
        };
        if entity.try_set_position(spawn_pos).is_err() {
            return None;
        }

        if let Some(mob) = entity.as_mob() {
            // Vanilla finalizes the spawn only when the payload carries no
            // configuration beyond `id`, then always marks it persistent.
            if next_spawn_entity.len() == 1 {
                mob.finalize_spawn(world, EntitySpawnReason::TrialSpawner, None);
            }
            mob.set_persistence_required();
        }
        Self::apply_extra_entity_tags(&*entity, &next_spawn_entity);
        Self::apply_equipment(&*entity, config, &next_spawn_entity);

        let uuid = entity.uuid();
        if world.try_add_entity(entity).is_err() {
            return None;
        }

        // Vanilla flame level events 3011 (spawner) / 3012 (spawn position).
        world.level_event(3011, pos, 0, None);
        world.level_event(
            3012,
            BlockPos::new(
                spawn_pos.x.floor() as i32,
                spawn_pos.y.floor() as i32,
                spawn_pos.z.floor() as i32,
            ),
            0,
            None,
        );
        let _ = is_ominous;

        let mut data = self.data.lock();
        data.next_spawn_entity = Some(next_spawn_entity);
        Some(uuid)
    }

    /// Weighted pick from the config's spawn potentials
    /// (vanilla `WeightedList.getRandom`).
    fn pick_spawn_potential(config: &TrialSpawnerConfig) -> Option<NbtCompound> {
        let total: u32 = config.spawn_potentials.iter().map(|p| p.weight).sum();
        let potential = if total == 0 {
            config.spawn_potentials.first()?
        } else {
            let mut roll = rand::rng().random_range(0..total);
            config
                .spawn_potentials
                .iter()
                .find(|potential| {
                    if roll < potential.weight {
                        true
                    } else {
                        roll -= potential.weight;
                        false
                    }
                })?
        };
        Some(Self::spawn_entity_nbt(potential))
    }

    /// Builds the `SpawnData.entity` NBT payload for a spawn potential.
    fn spawn_entity_nbt(
        potential: &steel_registry::trial_spawner::TrialSpawnerSpawnPotential,
    ) -> NbtCompound {
        let mut entity = NbtCompound::new();
        entity.insert("id", potential.entity_id);
        for tag in potential.extra_tags {
            match tag {
                TrialSpawnerEntityTag::Int { name, value } => {
                    entity.insert(*name, *value);
                }
                TrialSpawnerEntityTag::String { name, value } => {
                    entity.insert(*name, *value);
                }
            }
        }
        entity
    }

    /// Applies known gameplay flags from the spawn-potential entity payload.
    fn apply_extra_entity_tags(entity: &dyn Entity, payload: &NbtCompound) {
        if let Some(is_baby) = payload.int("IsBaby")
            && let Some(ageable) = entity.as_ageable_mob()
        {
            ageable.set_baby(is_baby != 0);
        }
    }

    /// Vanilla `SpawnData.equipment`: rolls the equipment loot table and equips
    /// the spawned mob (vanilla `SpawnData.equip`).
    fn apply_equipment(
        entity: &dyn Entity,
        config: &TrialSpawnerConfig,
        payload: &NbtCompound,
    ) {
        if payload.len() != 1 {
            return;
        }
        let Some(raw_id) = payload.string("id").map(|s| s.to_string()) else {
            return;
        };
        let Some(equipment) = config
            .spawn_potentials
            .iter()
            .find(|potential| potential.entity_id == raw_id)
            .and_then(|potential| potential.equipment)
        else {
            return;
        };

        let Ok(key) = equipment.loot_table.parse::<Identifier>() else {
            return;
        };
        let Some(table) = REGISTRY.loot_tables.by_key(&key) else {
            return;
        };
        let Some(living) = entity.as_living_entity() else {
            return;
        };

        let mut rng = rand::rng();
        let position = entity.position();
        let mut context =
            LootContext::new(&mut rng).with_origin(position.x, position.y, position.z);
        let stacks = table.get_random_items(&mut context);

        let mut equipped_any = false;
        for stack in stacks {
            if stack.is_empty() {
                continue;
            }
            for slot in steel_registry::equipment::EquipmentSlot::ALL {
                if living.is_equippable_in_slot(&stack, slot) {
                    living.with_equipment_slot_mut(slot, &mut |slot_item| {
                        *slot_item = stack.clone();
                    });
                    equipped_any = true;
                    break;
                }
            }
        }
        if equipped_any {
            living.detect_equipment_updates();
        }
    }

    fn spawn_position_is_free(world: &World, entity_type: EntityTypeRef, pos: DVec3) -> bool {
        // Approximates vanilla `level.noCollision(type.getSpawnAABB(...))`.
        let dims = entity_type.dimensions;
        let half = f64::from(dims.width) / 2.0;
        let aabb = WorldAabb::new(
            pos.x - half,
            pos.y,
            pos.z - half,
            pos.x + half,
            pos.y + f64::from(dims.height),
            pos.z + half,
        );
        world
            .get_entities_in_aabb_matching(&aabb, |entity| !entity.is_spectator())
            .is_empty()
    }

    /// Vanilla `TrialSpawner.inLineOfSight`.
    fn in_line_of_sight(world: &World, origin_pos: BlockPos, dest: DVec3) -> bool {
        let origin = Self::center_of(origin_pos);
        let hit = world.clip(dest, origin, ClipBlockShape::Visual, ClipFluid::None);
        hit.is_miss()
            || hit.block_pos
                == BlockPos::new(
                    origin.x.floor() as i32,
                    origin.y.floor() as i32,
                    origin.z.floor() as i32,
                )
    }

    /// Vanilla `TrialSpawner.ejectReward`.
    fn eject_reward(&self, world: &Arc<World>, pos: BlockPos, loot_table: &Identifier) {
        let Some(table) = REGISTRY.loot_tables.by_key(loot_table) else {
            return;
        };
        let mut rng = rand::rng();
        let mut context = LootContext::new(&mut rng).with_origin(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + 0.5,
            f64::from(pos.z()) + 0.5,
        );
        let drops = table.get_random_items(&mut context);
        if drops.is_empty() {
            return;
        }
        let eject_pos = DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + 1.2,
            f64::from(pos.z()) + 0.5,
        );
        for item in drops {
            // Vanilla `DefaultDispenseItemBehavior.spawnItem` upward with
            // speed 2 plus small jitter.
            let velocity = DVec3::new(
                rand::random::<f64>() * 0.2 - 0.1,
                2.0,
                rand::random::<f64>() * 0.2 - 0.1,
            );
            world.spawn_item_with_velocity(eject_pos, item, velocity);
        }
        world.level_event(3014, pos, 0, None);
    }

    /// Runs the vanilla trial-spawner tick state machine
    /// (`TrialSpawner.tickServer` + `TrialSpawnerState.tickAndGetNext`).
    pub fn tick_spawner(&self, world: &Arc<World>) {
        let pos = self.get_block_pos();
        let game_time = world.game_time();
        let state_id = self.get_block_state();
        let current_state: TrialSpawnerState =
            state_id.get_value(&BlockStateProperties::TRIAL_SPAWNER_STATE);
        let is_ominous: bool = state_id.get_value(&BlockStateProperties::OMINOUS);
        let config = self.active_config(is_ominous);

        // Vanilla untracks dead/gone mobs before ticking and resets the next
        // spawn timer when the tracked set shrinks.
        {
            let mut data = self.data.lock();
            let before = data.current_mobs.len();
            let center = Self::center_of(pos);
            data.current_mobs.retain(|uuid| {
                world.get_entity_by_uuid(uuid).is_some_and(|entity| {
                    entity.is_alive()
                        && entity.position().distance_squared(center)
                            <= MAX_MOB_TRACKING_DISTANCE_SQR
                })
            });
            if data.current_mobs.len() != before {
                data.next_mob_spawns_at = game_time + i64::from(config.ticks_between_spawn);
            }
        }

        let next_state = match current_state {
            TrialSpawnerState::Inactive => {
                let data = self.data.lock();
                if self.has_mob_to_spawn(config, data.next_spawn_entity.as_ref()) {
                    TrialSpawnerState::WaitingForPlayers
                } else {
                    TrialSpawnerState::Inactive
                }
            }
            TrialSpawnerState::WaitingForPlayers => {
                if !self.can_spawn_in_level(world) {
                    self.data.lock().reset_statistics();
                    TrialSpawnerState::WaitingForPlayers
                } else if !self.has_mob_to_spawn(config, self.data.lock().next_spawn_entity.as_ref())
                {
                    TrialSpawnerState::Inactive
                } else {
                    self.try_detect_players(world, pos, config, is_ominous, false);
                    if self.data.lock().detected_players.is_empty() {
                        TrialSpawnerState::WaitingForPlayers
                    } else {
                        TrialSpawnerState::Active
                    }
                }
            }
            TrialSpawnerState::Active => {
                if !self.can_spawn_in_level(world) {
                    self.data.lock().reset_statistics();
                    TrialSpawnerState::WaitingForPlayers
                } else if !self.has_mob_to_spawn(config, self.data.lock().next_spawn_entity.as_ref())
                {
                    TrialSpawnerState::Inactive
                } else {
                    let additional_players =
                        self.data.lock().detected_players.len().saturating_sub(1) as i32;
                    self.try_detect_players(world, pos, config, is_ominous, false);

                    // Never hold the spawner lock across `spawn_mob`: it takes
                    // the same lock internally and these mutexes are not
                    // reentrant, so holding it here deadlocks the tick thread
                    // on the first spawn attempt.
                    let wave_complete = {
                        let data = self.data.lock();
                        data.total_mobs_spawned
                            >= config.calculate_target_total_mobs(additional_players)
                            && data.current_mobs.is_empty()
                    };
                    if wave_complete {
                        let mut data = self.data.lock();
                        data.cooldown_ends_at = game_time + TARGET_COOLDOWN_LENGTH;
                        data.total_mobs_spawned = 0;
                        data.next_mob_spawns_at = 0;
                        TrialSpawnerState::WaitingForRewardEjection
                    } else {
                        let spawn_due = {
                            let data = self.data.lock();
                            game_time >= data.next_mob_spawns_at
                                && (data.current_mobs.len() as i32)
                                    < config.calculate_target_simultaneous_mobs(additional_players)
                        };
                        if spawn_due {
                            let spawned = self.spawn_mob(world, pos, config, is_ominous);
                            if let Some(uuid) = spawned {
                                let mut data = self.data.lock();
                                data.current_mobs.push(uuid);
                                data.total_mobs_spawned += 1;
                                data.next_mob_spawns_at =
                                    game_time + i64::from(config.ticks_between_spawn);
                            }
                        }
                        TrialSpawnerState::Active
                    }
                }
            }
            TrialSpawnerState::WaitingForRewardEjection => {
                // Vanilla opens the shutter 40 ticks after the cooldown began.
                let cooldown_started_at = self.data.lock().cooldown_ends_at - TARGET_COOLDOWN_LENGTH;
                if game_time >= cooldown_started_at + 40 {
                    world.play_sound(
                        &sound_events::BLOCK_TRIAL_SPAWNER_OPEN_SHUTTER,
                        SoundSource::Blocks,
                        pos,
                        1.0,
                        1.0,
                        None,
                    );
                    TrialSpawnerState::EjectingReward
                } else {
                    TrialSpawnerState::WaitingForRewardEjection
                }
            }
            TrialSpawnerState::EjectingReward => {
                let cooldown_started_at = self.data.lock().cooldown_ends_at - TARGET_COOLDOWN_LENGTH;
                if (game_time - cooldown_started_at) % 30 != 0 {
                    TrialSpawnerState::EjectingReward
                } else {
                    let loot_table = {
                        let mut data = self.data.lock();
                        if data.ejecting_loot_table.is_none() {
                            data.ejecting_loot_table = Self::pick_eject_table(config);
                        }
                        data.ejecting_loot_table.clone()
                    };
                    let detected_empty = {
                        let mut data = self.data.lock();
                        let empty = data.detected_players.is_empty();
                        if empty {
                            world.play_sound(
                                &sound_events::BLOCK_TRIAL_SPAWNER_CLOSE_SHUTTER,
                                SoundSource::Blocks,
                                pos,
                                1.0,
                                1.0,
                                None,
                            );
                            data.ejecting_loot_table = None;
                        } else {
                            data.detected_players.remove(0);
                        }
                        empty
                    };
                    if detected_empty {
                        TrialSpawnerState::Cooldown
                    } else {
                        if let Some(loot_table) = loot_table {
                            self.eject_reward(world, pos, &loot_table);
                        }
                        TrialSpawnerState::EjectingReward
                    }
                }
            }
            TrialSpawnerState::Cooldown => {
                self.try_detect_players(world, pos, config, is_ominous, true);
                let outcome = {
                    let mut data = self.data.lock();
                    if !data.detected_players.is_empty() {
                        data.total_mobs_spawned = 0;
                        data.next_mob_spawns_at = 0;
                        Some((false, TrialSpawnerState::Active))
                    } else if game_time >= data.cooldown_ends_at {
                        Some((true, TrialSpawnerState::WaitingForPlayers))
                    } else {
                        None
                    }
                };
                match outcome {
                    Some((clear_ominous, next)) => {
                        if clear_ominous {
                            // Vanilla `removeOminous` flips the block's ominous
                            // property back off and resets all state.
                            if is_ominous {
                                let next_state_id =
                                    state_id.set_value(&BlockStateProperties::OMINOUS, false);
                                world.set_block_state(pos, next_state_id, UpdateFlags::UPDATE_ALL);
                            }
                            self.data.lock().reset();
                        }
                        next
                    }
                    None => TrialSpawnerState::Cooldown,
                }
            }
        };

        if next_state != current_state {
            let next_state_id =
                state_id.set_value(&BlockStateProperties::TRIAL_SPAWNER_STATE, next_state);
            world.set_block_state(pos, next_state_id, UpdateFlags::UPDATE_ALL);
        }
        self.set_changed();
    }

    /// Weighted pick from `loot_tables_to_eject`.
    fn pick_eject_table(config: &TrialSpawnerConfig) -> Option<Identifier> {
        let total: u32 = config.loot_tables_to_eject.iter().map(|e| e.weight).sum();
        if total == 0 {
            return None;
        }
        let mut roll = rand::rng().random_range(0..total);
        for entry in config.loot_tables_to_eject {
            if roll < entry.weight {
                return entry.loot_table.parse::<Identifier>().ok();
            }
            roll -= entry.weight;
        }
        None
    }
}

impl BlockEntity for TrialSpawnerBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn tick(&self, world: &Arc<World>) {
        self.tick_spawner(world);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let data = self.data.lock();
        if let Some(config) = &data.normal_config_key {
            nbt.insert(NORMAL_CONFIG_TAG, config.to_string());
        }
        if let Some(config) = &data.ominous_config_key {
            nbt.insert(OMINOUS_CONFIG_TAG, config.to_string());
        }
        if !data.detected_players.is_empty() {
            nbt.insert(
                REGISTERED_PLAYERS_TAG,
                uuids_to_int_array_list(&data.detected_players),
            );
        }
        if !data.current_mobs.is_empty() {
            nbt.insert(CURRENT_MOBS_TAG, uuids_to_int_array_list(&data.current_mobs));
        }
        nbt.insert(COOLDOWN_ENDS_AT_TAG, data.cooldown_ends_at);
        nbt.insert(NEXT_MOB_SPAWNS_AT_TAG, data.next_mob_spawns_at);
        nbt.insert(TOTAL_MOBS_SPAWNED_TAG, data.total_mobs_spawned);
        if let Some(spawn_entity) = &data.next_spawn_entity {
            let mut spawn_data = NbtCompound::new();
            spawn_data.insert("entity", spawn_entity.clone());
            nbt.insert(SPAWN_DATA_TAG, spawn_data);
        }
        if let Some(loot_table) = &data.ejecting_loot_table {
            nbt.insert(EJECTING_LOOT_TABLE_TAG, loot_table.to_string());
        }
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let view: NbtCompoundView<'_, '_> = nbt.into();
        let mut data = self.data.lock();
        data.normal_config_key = view
            .string(NORMAL_CONFIG_TAG)
            .and_then(|s| s.to_string().parse::<Identifier>().ok());
        data.ominous_config_key = view
            .string(OMINOUS_CONFIG_TAG)
            .and_then(|s| s.to_string().parse::<Identifier>().ok());
        data.detected_players = read_uuid_list(&view, REGISTERED_PLAYERS_TAG);
        data.current_mobs = read_uuid_list(&view, CURRENT_MOBS_TAG);
        data.cooldown_ends_at = view.long(COOLDOWN_ENDS_AT_TAG).unwrap_or(0);
        data.next_mob_spawns_at = view.long(NEXT_MOB_SPAWNS_AT_TAG).unwrap_or(0);
        data.total_mobs_spawned = view.int(TOTAL_MOBS_SPAWNED_TAG).unwrap_or(0);
        data.next_spawn_entity = view
            .compound(SPAWN_DATA_TAG)
            .and_then(|spawn_data| spawn_data.compound("entity"))
            .map(|entity| entity.to_owned());
        data.ejecting_loot_table = view
            .string(EJECTING_LOOT_TABLE_TAG)
            .and_then(|s| s.to_string().parse::<Identifier>().ok());
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        // Vanilla `TrialSpawnerStateData.getUpdateTag`: the spawn data for the
        // spinning mob plus the next spawn time while active.
        let data = self.data.lock();
        let state_id = self.get_block_state();
        let state: TrialSpawnerState =
            state_id.get_value(&BlockStateProperties::TRIAL_SPAWNER_STATE);
        let mut tag = NbtCompound::new();
        if state == TrialSpawnerState::Active {
            tag.insert("next_mob_spawns_at", data.next_mob_spawns_at);
        }
        if let Some(spawn_entity) = &data.next_spawn_entity {
            let mut spawn_data = NbtCompound::new();
            spawn_data.insert("entity", spawn_entity.clone());
            tag.insert(SPAWN_DATA_TAG, spawn_data);
        }
        Some(tag)
    }
}

/// Reads a vanilla UUID int-array list into `Vec<Uuid>`.
fn read_uuid_list(view: &NbtCompoundView<'_, '_>, tag: &str) -> Vec<Uuid> {
    let Some(list) = view.list(tag) else {
        return Vec::new();
    };
    let Some(int_arrays) = list.int_arrays() else {
        return Vec::new();
    };
    int_arrays
        .iter()
        .filter_map(|ints| uuid_from_ints(&ints.to_vec()))
        .collect()
}

fn uuids_to_int_array_list(uuids: &[Uuid]) -> simdnbt::owned::NbtList {
    simdnbt::owned::NbtList::IntArray(
        uuids
            .iter()
            .map(|uuid| {
                let bytes = uuid.as_u128().to_be_bytes();
                bytes
                    .chunks_exact(4)
                    .map(|chunk| i32::from_be_bytes(chunk.try_into().expect("4-byte chunk")))
                    .collect()
            })
            .collect(),
    )
}

fn uuid_from_ints(ints: &[i32]) -> Option<Uuid> {
    if ints.len() != 4 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (index, value) in ints.iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    Some(Uuid::from_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_compound;
    use simdnbt::owned::NbtCompound;
    use steel_registry::init_vanilla_registry;
    use steel_registry::vanilla_blocks;
    use steel_utils::Identifier;

    use super::*;

    fn template_nbt() -> NbtCompound {
        let mut nbt = NbtCompound::new();
        nbt.insert("normal_config", "minecraft:trial_chamber/melee/zombie/normal");
        nbt.insert("ominous_config", "minecraft:trial_chamber/melee/zombie/ominous");
        nbt
    }

    macro_rules! borrowed {
        ($nbt:ident, $out:ident) => {
            let mut bytes: Vec<u8> = Vec::new();
            $nbt.write(&mut bytes);
            let $out =
                read_compound(&mut Cursor::new(bytes.as_slice())).expect("test nbt should reborrow");
        };
    }

    fn spawner_with_template_nbt() -> TrialSpawnerBlockEntity {
        init_vanilla_registry();
        let spawner = TrialSpawnerBlockEntity::new(
            Weak::new(),
            BlockPos::new(1, 2, 3),
            vanilla_blocks::TRIAL_SPAWNER.default_state(),
        );
        let nbt = template_nbt();
        borrowed!(nbt, compound);
        spawner.load_additional(&compound);
        spawner
    }

    #[test]
    fn template_config_keys_resolve_to_vanilla_configs() {
        let spawner = spawner_with_template_nbt();

        let normal = spawner.active_config(false);
        assert_eq!(
            normal.key,
            Identifier::vanilla_static("trial_chamber/melee/zombie/normal")
        );
        assert_eq!(
            normal.spawn_potentials[0].entity_id,
            "minecraft:zombie"
        );

        let ominous = spawner.active_config(true);
        assert_eq!(
            ominous.key,
            Identifier::vanilla_static("trial_chamber/melee/zombie/ominous")
        );
    }

    #[test]
    fn missing_configs_fall_back_to_vanilla_defaults() {
        init_vanilla_registry();
        let spawner = TrialSpawnerBlockEntity::new(
            Weak::new(),
            BlockPos::ZERO,
            vanilla_blocks::TRIAL_SPAWNER.default_state(),
        );
        assert_eq!(
            spawner.active_config(false).key,
            Identifier::vanilla_static("default")
        );
    }

    #[test]
    fn target_mob_counts_follow_vanilla_flooring() {
        let spawner = spawner_with_template_nbt();
        let config = spawner.active_config(false);
        // zombie normal: simultaneous_mobs 3, simultaneous_mobs_added_per_player 0.5
        assert_eq!(config.calculate_target_simultaneous_mobs(0), 3);
        assert_eq!(config.calculate_target_simultaneous_mobs(1), 3);
        assert_eq!(config.calculate_target_simultaneous_mobs(2), 4);
    }

    #[test]
    fn state_data_round_trips_through_save_and_load() {
        init_vanilla_registry();
        let spawner = TrialSpawnerBlockEntity::new(
            Weak::new(),
            BlockPos::new(1, 2, 3),
            vanilla_blocks::TRIAL_SPAWNER.default_state(),
        );
        let nbt = template_nbt();
        borrowed!(nbt, compound);
        spawner.load_additional(&compound);
        {
            let mut data = spawner.data.lock();
            data.cooldown_ends_at = 123_456;
            data.next_mob_spawns_at = 789;
            data.total_mobs_spawned = 4;
            data.current_mobs
                .push(Uuid::from_u128(0x0123_4567_89ab_cdef_fedc_ba98_7654_3210));
            let mut payload = NbtCompound::new();
            payload.insert("id", "minecraft:zombie");
            data.next_spawn_entity = Some(payload);
        }

        let mut saved = NbtCompound::new();
        spawner.save_additional(&mut saved);
        borrowed!(saved, saved_borrowed);

        let restored = TrialSpawnerBlockEntity::new(
            Weak::new(),
            BlockPos::new(1, 2, 3),
            vanilla_blocks::TRIAL_SPAWNER.default_state(),
        );
        restored.load_additional(&saved_borrowed);

        let data = restored.data.lock();
        assert_eq!(data.cooldown_ends_at, 123_456);
        assert_eq!(data.next_mob_spawns_at, 789);
        assert_eq!(data.total_mobs_spawned, 4);
        assert_eq!(
            data.current_mobs[0],
            Uuid::from_u128(0x0123_4567_89ab_cdef_fedc_ba98_7654_3210)
        );
        assert_eq!(
            data.next_spawn_entity
                .as_ref()
                .and_then(|payload| payload.string("id").map(|s| s.to_string())),
            Some("minecraft:zombie".to_owned())
        );
        assert_eq!(
            data.normal_config_key,
            Some(Identifier::vanilla_static("trial_chamber/melee/zombie/normal"))
        );
    }
}
