//! Shared spawn-cycle state and ticking used by spawner block entities.

use std::sync::Arc;

use glam::DVec3;
use rand::{Rng, RngExt, rng};
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList};
use steel_registry::entity_type::{EntityTypeRef, MobCategory};
use steel_registry::loot_table::{LootContext, LootTableRef};
use steel_registry::{
    REGISTRY, RegistryExt, level_events, vanilla_blocks, vanilla_game_events, vanilla_game_rules,
};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::SyncMutex;
use steel_utils::nbt::NbtNumeric;
use steel_utils::types::Difficulty;
use steel_utils::{BlockPos, Identifier, WorldAabb};

use super::spawn_data::{EquipmentTable, SpawnData, WeightedSpawnData};
use crate::entity::{
    Entity, EntitySpawnReason, Mob, SpawnPlacements, entity_loot_ref, load_entity_recursive_owned,
    nbt_dvec3,
};
use crate::inventory::equipment::EquipmentSlot;
use crate::physics::{WorldCollisionProvider, has_collision};
use crate::world::World;

const EVENT_SPAWN: i32 = 1;
const DEFAULT_SPAWN_DELAY: i32 = 20;
const DEFAULT_MIN_SPAWN_DELAY: i32 = 200;
const DEFAULT_MAX_SPAWN_DELAY: i32 = 800;
const DEFAULT_SPAWN_COUNT: i32 = 4;
const DEFAULT_MAX_NEARBY_ENTITIES: i32 = 6;
const DEFAULT_REQUIRED_PLAYER_RANGE: i32 = 16;
const DEFAULT_SPAWN_RANGE: i32 = 4;

#[derive(Debug)]
struct SpawnerState {
    spawn_delay: i32,
    spawn_potentials: Vec<WeightedSpawnData>,
    next_spawn_data: Option<SpawnData>,
    min_spawn_delay: i32,
    max_spawn_delay: i32,
    spawn_count: i32,
    max_nearby_entities: i32,
    required_player_range: i32,
    spawn_range: i32,
}

#[derive(Debug, Default)]
pub(super) struct SpawnerTickResult {
    pub(super) state_changed: bool,
    pub(super) next_spawn_data_changed: bool,
}

impl Default for SpawnerState {
    fn default() -> Self {
        Self {
            spawn_delay: DEFAULT_SPAWN_DELAY,
            spawn_potentials: Vec::new(),
            next_spawn_data: None,
            min_spawn_delay: DEFAULT_MIN_SPAWN_DELAY,
            max_spawn_delay: DEFAULT_MAX_SPAWN_DELAY,
            spawn_count: DEFAULT_SPAWN_COUNT,
            max_nearby_entities: DEFAULT_MAX_NEARBY_ENTITIES,
            required_player_range: DEFAULT_REQUIRED_PLAYER_RANGE,
            spawn_range: DEFAULT_SPAWN_RANGE,
        }
    }
}

/// Vanilla-style mutable spawner state shared by normal spawner block entities.
#[derive(Debug)]
pub struct BaseSpawner {
    state: SyncMutex<SpawnerState>,
}

impl Default for BaseSpawner {
    fn default() -> Self {
        Self::new()
    }
}

impl BaseSpawner {
    /// Creates a spawner with vanilla defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: SyncMutex::new(SpawnerState::default()),
        }
    }

    /// Loads the vanilla spawner fields from NBT.
    pub fn load(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt: NbtCompoundView<'_, '_> = nbt.into();
        let mut state = self.state.lock();
        state.spawn_delay = nbt
            .get("Delay")
            .and_then(|tag| tag.short_value())
            .map_or(DEFAULT_SPAWN_DELAY, i32::from);
        state.next_spawn_data = nbt
            .compound("SpawnData")
            .and_then(|data| SpawnData::from_nbt(&data));
        let loaded_spawn_potentials = nbt.list("SpawnPotentials").map(|entries| {
            entries
                .compounds()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|entry| {
                    let weight = entry.get("weight")?.codec_i32()?;
                    if weight < 0 {
                        return None;
                    }
                    let data = entry.compound("data")?;
                    Some(WeightedSpawnData {
                        weight,
                        data: SpawnData::from_nbt(&data)?,
                    })
                })
                .collect::<Vec<_>>()
        });
        state.spawn_potentials = loaded_spawn_potentials.unwrap_or_else(|| {
            vec![WeightedSpawnData {
                weight: 1,
                data: state.next_spawn_data.clone().unwrap_or_default(),
            }]
        });
        state.min_spawn_delay = nbt
            .get("MinSpawnDelay")
            .and_then(|tag| tag.int_value())
            .unwrap_or(DEFAULT_MIN_SPAWN_DELAY);
        state.max_spawn_delay = nbt
            .get("MaxSpawnDelay")
            .and_then(|tag| tag.int_value())
            .unwrap_or(DEFAULT_MAX_SPAWN_DELAY);
        state.spawn_count = nbt
            .get("SpawnCount")
            .and_then(|tag| tag.int_value())
            .unwrap_or(DEFAULT_SPAWN_COUNT);
        state.max_nearby_entities = nbt
            .get("MaxNearbyEntities")
            .and_then(|tag| tag.int_value())
            .unwrap_or(DEFAULT_MAX_NEARBY_ENTITIES);
        state.required_player_range = nbt
            .get("RequiredPlayerRange")
            .and_then(|tag| tag.int_value())
            .unwrap_or(DEFAULT_REQUIRED_PLAYER_RANGE);
        state.spawn_range = nbt
            .get("SpawnRange")
            .and_then(|tag| tag.int_value())
            .unwrap_or(DEFAULT_SPAWN_RANGE);
    }

    /// Saves the vanilla spawner fields to NBT.
    pub fn save(&self, nbt: &mut NbtCompound) {
        let state = self.state.lock();
        nbt.insert("Delay", state.spawn_delay as i16);
        nbt.insert("MinSpawnDelay", state.min_spawn_delay as i16);
        nbt.insert("MaxSpawnDelay", state.max_spawn_delay as i16);
        nbt.insert("SpawnCount", state.spawn_count as i16);
        nbt.insert("MaxNearbyEntities", state.max_nearby_entities as i16);
        nbt.insert("RequiredPlayerRange", state.required_player_range as i16);
        nbt.insert("SpawnRange", state.spawn_range as i16);
        if let Some(next_spawn_data) = &state.next_spawn_data {
            nbt.insert("SpawnData", next_spawn_data.to_nbt());
        }
        let potentials = state
            .spawn_potentials
            .iter()
            .map(|entry| {
                let mut nbt = NbtCompound::new();
                nbt.insert("data", entry.data.to_nbt());
                nbt.insert("weight", entry.weight);
                nbt
            })
            .collect::<Vec<_>>();
        nbt.insert("SpawnPotentials", NbtList::Compound(potentials));
    }

    /// Selects or returns the active spawn data.
    #[cfg(test)]
    pub(super) fn get_or_create_next_spawn_data(&self) -> SpawnData {
        let mut random = rng();
        self.get_or_create_next_spawn_data_with_rng(&mut random).0
    }

    fn get_or_create_next_spawn_data_with_rng<R: Rng + ?Sized>(
        &self,
        random: &mut R,
    ) -> (SpawnData, bool) {
        let mut state = self.state.lock();
        if let Some(data) = &state.next_spawn_data {
            return (data.clone(), false);
        }

        let data = choose_weighted(&state.spawn_potentials, random).unwrap_or_default();
        state.next_spawn_data = Some(data.clone());
        (data, true)
    }

    /// Configures the active spawn entity.
    pub fn set_entity_id(&self, entity_type: EntityTypeRef) {
        let mut random = rng();
        let mut state = self.state.lock();
        if state.next_spawn_data.is_none() {
            state.next_spawn_data =
                Some(choose_weighted(&state.spawn_potentials, &mut random).unwrap_or_default());
        }
        if let Some(data) = state.next_spawn_data.as_mut() {
            replace_entity_id(&mut data.entity_to_spawn, entity_type);
        }
    }

    /// Returns whether this spawner handles the given block event.
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "The event method mirrors the vanilla BaseSpawner delegation API."
    )]
    pub const fn on_event_triggered(&self, event: i32) -> bool {
        event == EVENT_SPAWN
    }

    /// Returns the configured delay for diagnostics and focused tests.
    #[must_use]
    pub fn spawn_delay(&self) -> i32 {
        self.state.lock().spawn_delay
    }

    #[expect(
        clippy::too_many_lines,
        reason = "The order of this method mirrors vanilla BaseSpawner.serverTick."
    )]
    pub(super) fn server_tick(&self, world: &Arc<World>, pos: BlockPos) -> SpawnerTickResult {
        let center = DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + 0.5,
            f64::from(pos.z()) + 0.5,
        );
        let Some(_) =
            world.nearest_player(center, f64::from(self.required_player_range()), |player| {
                player.is_alive() && !player.is_spectator()
            })
        else {
            return SpawnerTickResult::default();
        };
        if !world.get_game_rule(&vanilla_game_rules::SPAWNER_BLOCKS_WORK) {
            return SpawnerTickResult::default();
        }

        let mut random = rng();
        let mut result = SpawnerTickResult::default();
        if self.spawn_delay() == -1 {
            result = self.delay(world, pos, &mut random);
        }

        if self.decrement_spawn_delay() {
            return result;
        }

        let (next_spawn_data, selected_data_changed) =
            self.get_or_create_next_spawn_data_with_rng(&mut random);
        result.next_spawn_data_changed |= selected_data_changed;
        result.state_changed |= selected_data_changed;
        let spawn_count = self.spawn_count();
        let spawn_range = self.spawn_range();
        let max_nearby_entities = self.max_nearby_entities();
        let mut spawned_any = false;

        for _ in 0..spawn_count {
            let Some(entity_type) = entity_type_from_owned_nbt(&next_spawn_data.entity_to_spawn)
            else {
                return self.delay_result(world, pos, &mut random, result);
            };

            let spawn_position = configured_or_random_position(
                &next_spawn_data.entity_to_spawn,
                pos,
                spawn_range,
                &mut random,
            );
            let dimensions = entity_type.dimensions;
            let spawn_box = WorldAabb::entity_box(
                spawn_position.x,
                spawn_position.y,
                spawn_position.z,
                f64::from(dimensions.half_width()),
                f64::from(dimensions.height),
            );
            if has_collision(&WorldCollisionProvider::new(world), spawn_box) {
                continue;
            }

            let spawn_block_pos = BlockPos::from(spawn_position);
            if let Some(custom_rules) = next_spawn_data.custom_spawn_rules {
                if entity_type.mob_category == MobCategory::Monster
                    && world.difficulty() == Difficulty::Peaceful
                {
                    continue;
                }
                if !custom_rules.is_valid_position(world, spawn_block_pos) {
                    continue;
                }
            } else if !SpawnPlacements::check_spawner_spawn_rules(
                entity_type,
                world,
                spawn_block_pos,
            ) {
                continue;
            }

            let Some(loaded) = load_entity_recursive_owned(
                world,
                &next_spawn_data.entity_to_spawn,
                EntitySpawnReason::Spawner,
                |entity| {
                    entity.snap_to(spawn_position, entity.rotation().0, entity.rotation().1);
                },
            ) else {
                return self.delay_result(world, pos, &mut random, result);
            };
            let entity = &loaded.root;

            let entity_class = entity.downcast_type_key();
            let nearby_box = WorldAabb::new(
                f64::from(pos.x()),
                f64::from(pos.y()),
                f64::from(pos.z()),
                f64::from(pos.x() + 1),
                f64::from(pos.y() + 1),
                f64::from(pos.z() + 1),
            )
            .inflate(f64::from(spawn_range));
            let nearby_count = world
                .get_entities_in_aabb_matching(&nearby_box, |candidate| {
                    !candidate.is_spectator() && candidate.downcast_type_key() == entity_class
                })
                .len();
            if nearby_count as i32 >= max_nearby_entities {
                return self.delay_result(world, pos, &mut random, result);
            }

            let yaw = random.random::<f32>() * 360.0;
            entity.snap_to(entity.position(), yaw, 0.0);
            if let Some(mob) = entity.as_mob() {
                if (next_spawn_data.custom_spawn_rules.is_none()
                    && !mob.check_spawn_rules(world, EntitySpawnReason::Spawner))
                    || !mob.check_spawn_obstruction(world)
                {
                    continue;
                }

                // TODO: Vanilla calls `finalizeSpawn(SPAWNER)` here when the entity NBT holds
                // only an `id`. Blocked on local difficulty (`DifficultyInstance`: world
                // difficulty, overworld time, chunk InhabitedTime, moon brightness), which
                // `Mob::finalize_spawn` does not take yet. Equipment applies either way.
                if let Some(equipment) = &next_spawn_data.equipment {
                    equip_from_table(mob, equipment);
                }
            }

            if world
                .try_add_fresh_entity_with_passengers(Arc::clone(entity))
                .is_err()
            {
                return self.delay_result(world, pos, &mut random, result);
            }

            world.level_event(level_events::PARTICLES_MOBBLOCK_SPAWN, pos, 0, None);
            world.game_event(
                &vanilla_game_events::ENTITY_PLACE,
                spawn_block_pos,
                &crate::world::game_event::GameEventContext::new(Some(entity.as_ref()), None),
            );
            if entity.as_mob().is_some() {
                entity.broadcast_entity_event(EntityStatus::SilverfishMergeAnim);
            }
            spawned_any = true;
        }

        if spawned_any {
            result = self.delay_result(world, pos, &mut random, result);
        }
        result
    }

    fn delay_result<R: Rng + ?Sized>(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        random: &mut R,
        mut result: SpawnerTickResult,
    ) -> SpawnerTickResult {
        let delay_result = self.delay(world, pos, random);
        result.state_changed |= delay_result.state_changed;
        result.next_spawn_data_changed |= delay_result.next_spawn_data_changed;
        result
    }

    fn delay<R: Rng + ?Sized>(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        random: &mut R,
    ) -> SpawnerTickResult {
        let mut result = SpawnerTickResult {
            state_changed: true,
            next_spawn_data_changed: false,
        };
        {
            let mut state = self.state.lock();
            let delay = state.max_spawn_delay.saturating_sub(state.min_spawn_delay);
            state.spawn_delay = if delay <= 0 {
                state.min_spawn_delay
            } else {
                state.min_spawn_delay + random.random_range(0..delay)
            };
            if let Some(next_spawn_data) = choose_weighted(&state.spawn_potentials, random)
                && state.next_spawn_data.as_ref() != Some(&next_spawn_data)
            {
                state.next_spawn_data = Some(next_spawn_data);
                result.next_spawn_data_changed = true;
            }
        }
        world.block_event(pos, &vanilla_blocks::SPAWNER, EVENT_SPAWN, 0);
        result
    }

    fn decrement_spawn_delay(&self) -> bool {
        let mut state = self.state.lock();
        if state.spawn_delay > 0 {
            state.spawn_delay -= 1;
            true
        } else {
            false
        }
    }

    fn required_player_range(&self) -> i32 {
        self.state.lock().required_player_range
    }

    fn spawn_count(&self) -> i32 {
        self.state.lock().spawn_count
    }

    fn max_nearby_entities(&self) -> i32 {
        self.state.lock().max_nearby_entities
    }

    fn spawn_range(&self) -> i32 {
        self.state.lock().spawn_range
    }
}

pub(super) fn choose_weighted<R: Rng + ?Sized>(
    entries: &[WeightedSpawnData],
    random: &mut R,
) -> Option<SpawnData> {
    let total_weight = entries
        .iter()
        .map(|entry| i64::from(entry.weight))
        .sum::<i64>();
    if total_weight == 0 {
        return None;
    }

    let mut selection = random.random_range(0..total_weight);
    for entry in entries {
        let weight = i64::from(entry.weight);
        if selection < weight {
            return Some(entry.data.clone());
        }
        selection -= weight;
    }
    None
}

fn entity_type_from_owned_nbt(entity: &NbtCompound) -> Option<EntityTypeRef> {
    let id = entity.string("id")?.to_str().parse::<Identifier>().ok()?;
    REGISTRY.entity_types.by_key(&id)
}

fn configured_or_random_position<R: Rng + ?Sized>(
    entity: &NbtCompound,
    spawner_pos: BlockPos,
    spawn_range: i32,
    random: &mut R,
) -> DVec3 {
    entity
        .get("Pos")
        .and_then(nbt_dvec3)
        .filter(|position| position.is_finite())
        .unwrap_or_else(|| random_spawn_position(spawner_pos, spawn_range, random))
}

fn random_spawn_position<R: Rng + ?Sized>(
    spawner_pos: BlockPos,
    spawn_range: i32,
    random: &mut R,
) -> DVec3 {
    DVec3::new(
        f64::from(spawner_pos.x())
            + (random.random::<f64>() - random.random::<f64>()) * f64::from(spawn_range)
            + 0.5,
        f64::from(spawner_pos.y()) + f64::from(random.random_range(0..3)) - 1.0,
        f64::from(spawner_pos.z())
            + (random.random::<f64>() - random.random::<f64>()) * f64::from(spawn_range)
            + 0.5,
    )
}

fn equip_from_table(mob: &dyn Mob, equipment: &EquipmentTable) {
    let Some(table): Option<LootTableRef> = REGISTRY.loot_tables.by_key(&equipment.loot_table)
    else {
        return;
    };

    let mut random = rng();
    let entity_ref = entity_loot_ref(mob.as_entity_event_source());
    let mut context = LootContext::new(&mut random)
        .with_origin(mob.position().x, mob.position().y, mob.position().z)
        .with_this_entity(entity_ref);
    let mut inserted = [false; EquipmentSlot::ALL.len()];
    for mut item in table.get_random_items(&mut context) {
        if item.is_empty() {
            continue;
        }
        let slot = item
            .get_equippable_slot()
            .unwrap_or(EquipmentSlot::MainHand);
        if inserted[slot.index()] {
            continue;
        }

        let equipped = slot.limit(&mut item);
        if equipped.is_empty() {
            continue;
        }
        mob.living_base().equipment().lock().set(slot, equipped);
        if let Some(chance) = equipment.drop_chance(slot) {
            let _ = mob.set_equipment_drop_chance(slot, chance);
        }
        inserted[slot.index()] = true;
    }
}

fn replace_entity_id(entity: &mut NbtCompound, entity_type: EntityTypeRef) {
    while entity.remove("id").is_some() {}
    entity.insert("id", entity_type.key.to_string());
}
