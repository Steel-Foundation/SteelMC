//! Normal mob-spawner block entity and its persistent spawn configuration.

use std::sync::{Arc, Weak};

use glam::DVec3;
use rand::{Rng, RngExt, rng};
use simdnbt::borrow::NbtTag as BorrowedNbtTag;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList};
use steel_registry::entity_type::{EntityTypeRef, MobCategory};
use steel_registry::loot_table::{LootContext, LootTableRef};
use steel_registry::{
    REGISTRY, RegistryExt, level_events, vanilla_block_entity_types, vanilla_blocks,
    vanilla_game_events, vanilla_game_rules,
};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::SyncMutex;
use steel_utils::nbt::NbtNumeric;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, Identifier, WorldAabb};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::chunk::light::LightLayer;
use crate::entity::{
    Entity, EntitySpawnReason, Mob, SpawnPlacements, entity_loot_ref, load_entity_recursive_owned,
};
use crate::inventory::equipment::EquipmentSlot;
use crate::physics::{WorldCollisionProvider, has_collision};
use crate::world::World;
use steel_utils::types::Difficulty;

const EVENT_SPAWN: i32 = 1;
const DEFAULT_SPAWN_DELAY: i32 = 20;
const DEFAULT_MIN_SPAWN_DELAY: i32 = 200;
const DEFAULT_MAX_SPAWN_DELAY: i32 = 800;
const DEFAULT_SPAWN_COUNT: i32 = 4;
const DEFAULT_MAX_NEARBY_ENTITIES: i32 = 6;
const DEFAULT_REQUIRED_PLAYER_RANGE: i32 = 16;
const DEFAULT_SPAWN_RANGE: i32 = 4;

/// Inclusive light range used by custom spawner rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightRange {
    /// Lowest accepted light value.
    pub min: i32,
    /// Highest accepted light value.
    pub max: i32,
}

impl LightRange {
    const DEFAULT: Self = Self { min: 0, max: 15 };

    fn from_nbt(tag: Option<BorrowedNbtTag<'_, '_>>) -> Option<Self> {
        // The optional interval codec defaults malformed ranges before validating light bounds.
        let range = tag
            .and_then(|tag| {
                if let Some(value) = tag.codec_i32() {
                    return Some(Self {
                        min: value,
                        max: value,
                    });
                }
                let (min, max) = if let Some(compound) = tag.compound() {
                    (
                        compound.get("min_inclusive")?.codec_i32()?,
                        compound.get("max_inclusive")?.codec_i32()?,
                    )
                } else {
                    let values = if let Some(list) = tag.list() {
                        list.to_owned()
                            .as_nbt_tags()
                            .iter()
                            .map(NbtNumeric::codec_i32)
                            .collect::<Option<Vec<_>>>()?
                    } else if let Some(values) = tag.int_array() {
                        values
                    } else if let Some(values) = tag.long_array() {
                        values.into_iter().map(|value| value as i32).collect()
                    } else {
                        tag.byte_array()?
                            .iter()
                            .map(|value| i32::from(*value as i8))
                            .collect()
                    };
                    let [min, max] = values.as_slice() else {
                        return None;
                    };
                    (*min, *max)
                };
                (min <= max).then_some(Self { min, max })
            })
            .unwrap_or(Self::DEFAULT);
        (range.min >= 0 && range.max <= 15).then_some(range)
    }

    fn to_nbt(self) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        nbt.insert("min_inclusive", self.min);
        nbt.insert("max_inclusive", self.max);
        nbt
    }

    /// Returns whether `value` is in this range.
    #[must_use]
    pub const fn contains(self, value: u8) -> bool {
        value as i32 >= self.min && value as i32 <= self.max
    }
}

/// Optional light restrictions stored in a spawner's `SpawnData`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomSpawnRules {
    /// Accepted block-light range.
    pub block_light_limit: LightRange,
    /// Accepted effective sky-light range.
    pub sky_light_limit: LightRange,
}

impl CustomSpawnRules {
    fn from_nbt(nbt: &NbtCompoundView<'_, '_>) -> Option<Self> {
        Some(Self {
            block_light_limit: LightRange::from_nbt(nbt.get("block_light_limit"))?,
            sky_light_limit: LightRange::from_nbt(nbt.get("sky_light_limit"))?,
        })
    }

    fn to_nbt(self) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        nbt.insert("block_light_limit", self.block_light_limit.to_nbt());
        nbt.insert("sky_light_limit", self.sky_light_limit.to_nbt());
        nbt
    }

    /// Returns whether the world lighting at `pos` satisfies this rule.
    #[must_use]
    pub fn is_valid_position(&self, world: &World, pos: BlockPos) -> bool {
        self.block_light_limit
            .contains(world.light_value_at(LightLayer::Block, pos))
            && self
                .sky_light_limit
                .contains(world.effective_sky_brightness(pos))
    }
}

/// Equipment loot-table configuration attached to spawner data.
#[derive(Debug, Clone, PartialEq)]
pub struct EquipmentTable {
    /// Loot table used to generate equipment.
    pub loot_table: Identifier,
    /// Per-slot drop chances. A scalar NBT value is expanded to all slots.
    pub slot_drop_chances: Vec<(EquipmentSlot, f32)>,
}

impl EquipmentTable {
    fn from_nbt(nbt: &NbtCompoundView<'_, '_>) -> Option<Self> {
        let loot_table = nbt.string("loot_table")?.to_str().parse().ok()?;
        let mut slot_drop_chances = Vec::new();
        match nbt.get("slot_drop_chances") {
            Some(tag) if tag.float().is_some() => {
                let chance = tag.float()?;
                for slot in EquipmentSlot::ALL {
                    slot_drop_chances.push((slot, chance));
                }
            }
            Some(tag) => {
                let chances = tag.compound()?;
                for (name, value) in chances.iter() {
                    let name = name.to_str();
                    let slot = EquipmentSlot::by_name(name.as_ref())?;
                    slot_drop_chances.push((slot, value.float()?));
                }
            }
            None => {}
        }

        Some(Self {
            loot_table,
            slot_drop_chances,
        })
    }

    fn to_nbt(&self) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        nbt.insert("loot_table", self.loot_table.to_string());
        if !self.slot_drop_chances.is_empty() {
            let all_slots = EquipmentSlot::ALL;
            let scalar = self.slot_drop_chances.len() == all_slots.len()
                && all_slots.iter().all(|slot| {
                    self.slot_drop_chances
                        .iter()
                        .find(|(entry_slot, _)| entry_slot == slot)
                        .is_some_and(|(_, chance)| {
                            self.slot_drop_chances
                                .first()
                                .is_some_and(|(_, first)| chance.to_bits() == first.to_bits())
                        })
                });
            if scalar {
                if let Some((_, chance)) = self.slot_drop_chances.first() {
                    nbt.insert("slot_drop_chances", *chance);
                }
            } else {
                let mut chances = NbtCompound::new();
                for (slot, chance) in &self.slot_drop_chances {
                    chances.insert(slot.name(), *chance);
                }
                nbt.insert("slot_drop_chances", chances);
            }
        }
        nbt
    }

    /// Returns the configured drop chance for a slot, if one was supplied.
    #[must_use]
    pub fn drop_chance(&self, slot: EquipmentSlot) -> Option<f32> {
        self.slot_drop_chances
            .iter()
            .find(|(entry_slot, _)| *entry_slot == slot)
            .map(|(_, chance)| *chance)
    }
}

/// One weighted `SpawnPotentials` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct WeightedSpawnData {
    /// Spawn selection weight.
    pub weight: i32,
    /// Data selected by this entry.
    pub data: SpawnData,
}

/// Entity and optional rules used by a spawner.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnData {
    /// Entity NBT, including its `id`.
    pub entity_to_spawn: NbtCompound,
    /// Optional custom lighting rules.
    pub custom_spawn_rules: Option<CustomSpawnRules>,
    /// Optional equipment loot table.
    pub equipment: Option<EquipmentTable>,
}

impl Default for SpawnData {
    fn default() -> Self {
        Self {
            entity_to_spawn: NbtCompound::new(),
            custom_spawn_rules: None,
            equipment: None,
        }
    }
}

impl SpawnData {
    fn from_nbt(nbt: &NbtCompoundView<'_, '_>) -> Option<Self> {
        let mut entity_to_spawn = nbt.compound("entity")?.to_owned();
        normalize_entity_id(&mut entity_to_spawn);
        let custom_spawn_rules = match nbt.get("custom_spawn_rules") {
            Some(tag) => Some(CustomSpawnRules::from_nbt(&tag.compound()?)?),
            None => None,
        };
        let equipment = match nbt.get("equipment") {
            Some(tag) => Some(EquipmentTable::from_nbt(&tag.compound()?)?),
            None => None,
        };
        Some(Self {
            entity_to_spawn,
            custom_spawn_rules,
            equipment,
        })
    }

    fn to_nbt(&self) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        nbt.insert("entity", self.entity_to_spawn.clone());
        if let Some(rules) = self.custom_spawn_rules {
            nbt.insert("custom_spawn_rules", rules.to_nbt());
        }
        if let Some(equipment) = &self.equipment {
            nbt.insert("equipment", equipment.to_nbt());
        }
        nbt
    }
}

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
struct SpawnerTickResult {
    state_changed: bool,
    next_spawn_data_changed: bool,
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
            .short("Delay")
            .map(i32::from)
            .or_else(|| nbt.int("Delay"))
            .unwrap_or(DEFAULT_SPAWN_DELAY);
        state.next_spawn_data = nbt
            .compound("SpawnData")
            .and_then(|data| SpawnData::from_nbt(&data));
        let loaded_spawn_potentials = nbt
            .list("SpawnPotentials")
            .and_then(|entries| entries.compounds())
            .and_then(|entries| {
                entries
                    .into_iter()
                    .map(|entry| {
                        let weight = entry
                            .int("weight")
                            .or_else(|| entry.short("weight").map(i32::from))?;
                        let data = entry.compound("data")?;
                        Some(WeightedSpawnData {
                            weight,
                            data: SpawnData::from_nbt(&data)?,
                        })
                    })
                    .collect::<Option<Vec<_>>>()
            });
        state.spawn_potentials = loaded_spawn_potentials.unwrap_or_else(|| {
            vec![WeightedSpawnData {
                weight: 1,
                data: state.next_spawn_data.clone().unwrap_or_default(),
            }]
        });
        state.min_spawn_delay = read_i32(&nbt, "MinSpawnDelay", DEFAULT_MIN_SPAWN_DELAY);
        state.max_spawn_delay = read_i32(&nbt, "MaxSpawnDelay", DEFAULT_MAX_SPAWN_DELAY);
        state.spawn_count = read_i32(&nbt, "SpawnCount", DEFAULT_SPAWN_COUNT);
        state.max_nearby_entities =
            read_i32(&nbt, "MaxNearbyEntities", DEFAULT_MAX_NEARBY_ENTITIES);
        state.required_player_range =
            read_i32(&nbt, "RequiredPlayerRange", DEFAULT_REQUIRED_PLAYER_RANGE);
        state.spawn_range = read_i32(&nbt, "SpawnRange", DEFAULT_SPAWN_RANGE);
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
    pub fn get_or_create_next_spawn_data(&self) -> SpawnData {
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

    /// Configures the active spawn entity, as a spawn egg does in vanilla.
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

    /// Handles vanilla block event `1`.
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "The event method mirrors the vanilla BaseSpawner delegation API."
    )]
    pub const fn on_event_triggered(&self, event: i32) -> bool {
        event == EVENT_SPAWN
    }

    /// Applies the client-side portion of vanilla block event `1`.
    pub fn on_client_event_triggered(&self, event: i32) -> bool {
        if event != EVENT_SPAWN {
            return false;
        }

        let mut state = self.state.lock();
        state.spawn_delay = state.min_spawn_delay;
        true
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
    fn server_tick(&self, world: &Arc<World>, pos: BlockPos) -> SpawnerTickResult {
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
            result.state_changed = true;
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

                // TODO(spawner-local-difficulty): Before default SpawnData can use vanilla
                // finalization, add DifficultyInstance from world difficulty, overworld time,
                // the spawn chunk's InhabitedTime, and moon brightness, then call
                // Mob::finalize_spawn with EntitySpawnReason::Spawner. Keep custom SpawnData
                // and equipment-table application separate from this deferred path.
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
                // Vanilla uses entity-event byte 20 for mob spawn poof particles.
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

fn read_i32(nbt: &NbtCompoundView<'_, '_>, key: &str, default: i32) -> i32 {
    nbt.int(key)
        .or_else(|| nbt.short(key).map(i32::from))
        .or_else(|| nbt.byte(key).map(i32::from))
        .unwrap_or(default)
}

fn choose_weighted<R: Rng + ?Sized>(
    entries: &[WeightedSpawnData],
    random: &mut R,
) -> Option<SpawnData> {
    let total_weight = entries
        .iter()
        .map(|entry| i64::from(entry.weight.max(0)))
        .sum::<i64>();
    if total_weight == 0 {
        return None;
    }

    let mut selection = random.random_range(0..total_weight);
    for entry in entries {
        let weight = i64::from(entry.weight.max(0));
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
    let Some(values) = entity.list("Pos").and_then(NbtList::doubles) else {
        return random_spawn_position(spawner_pos, spawn_range, random);
    };
    let &[x, y, z] = values.as_slice() else {
        return random_spawn_position(spawner_pos, spawn_range, random);
    };
    let position = DVec3::new(x, y, z);
    if position.is_finite() {
        position
    } else {
        random_spawn_position(spawner_pos, spawn_range, random)
    }
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

fn normalize_entity_id(entity: &mut NbtCompound) {
    let Some(id) = entity.string("id") else {
        return;
    };
    let Ok(id) = id.to_str().parse::<Identifier>() else {
        while entity.remove("id").is_some() {}
        return;
    };
    while entity.remove("id").is_some() {}
    entity.insert("id", id.to_string());
}

/// Capability implemented by normal spawner block entities.
pub trait Spawner {
    /// Sets the next spawn entity type.
    fn set_entity_id(&self, entity_type: EntityTypeRef);
}

/// Concrete normal mob-spawner block entity.
pub struct SpawnerBlockEntity {
    base: BlockEntityBase,
    spawner: BaseSpawner,
}

// SAFETY: This key is owned by Steel and uniquely identifies `SpawnerBlockEntity`.
unsafe impl DowncastType for SpawnerBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/spawner");
}

impl SpawnerBlockEntity {
    /// Creates a normal mob-spawner block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            base: BlockEntityBase::new(&vanilla_block_entity_types::MOB_SPAWNER, level, pos, state),
            spawner: BaseSpawner::new(),
        }
    }

    /// Returns the spawner state owner.
    #[must_use]
    pub const fn spawner(&self) -> &BaseSpawner {
        &self.spawner
    }
}

impl Spawner for SpawnerBlockEntity {
    fn set_entity_id(&self, entity_type: EntityTypeRef) {
        self.spawner.set_entity_id(entity_type);
        self.set_changed();
        if let Some(world) = self.get_level() {
            world.send_block_updated(self.get_block_pos());
        }
    }
}

impl BlockEntity for SpawnerBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        self.spawner.load(nbt);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.spawner.save(nbt);
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        let mut nbt = self.save_custom_only();
        nbt.remove("SpawnPotentials");
        Some(nbt)
    }

    fn trigger_event(&self, event: i32, _data: i32) -> bool {
        self.spawner.on_event_triggered(event)
    }

    fn tick(&self, world: &Arc<World>) {
        let result = self.spawner.server_tick(world, self.get_block_pos());
        if result.state_changed {
            self.set_changed();
        }
        if result.next_spawn_data_changed {
            world.send_block_updated(self.get_block_pos());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        io::Cursor,
        sync::{Arc, Weak},
    };

    use rand::TryRng;
    use simdnbt::{
        borrow::read_compound,
        owned::{NbtList, NbtTag},
    };
    use steel_registry::{init_vanilla_registry, vanilla_entities};
    use steel_utils::nbt::parse_snbt_compound;

    use crate::entity::init_entities;
    use crate::player::ResetReason;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world};

    use super::*;

    #[derive(Debug)]
    struct FixedRng(u64);

    impl TryRng for FixedRng {
        type Error = Infallible;

        fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
            Ok(self.0 as u32)
        }

        fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
            Ok(self.0)
        }

        fn try_fill_bytes(&mut self, bytes: &mut [u8]) -> Result<(), Self::Error> {
            bytes.fill(self.0 as u8);
            Ok(())
        }
    }

    fn entity(id: &str) -> NbtCompound {
        let mut entity = NbtCompound::new();
        entity.insert("id", id);
        entity
    }

    fn spawn_data(id: &str) -> SpawnData {
        SpawnData {
            entity_to_spawn: entity(id),
            custom_spawn_rules: None,
            equipment: None,
        }
    }

    fn load_spawner(nbt: &NbtCompound) -> BaseSpawner {
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("spawner NBT should be readable");
        let spawner = BaseSpawner::new();
        spawner.load(&borrowed);
        spawner
    }

    #[test]
    fn custom_spawner_light_ranges_decode_vanilla_interval_forms() {
        let mut compound = NbtCompound::new();
        compound.insert("min_inclusive", 2_i8);
        compound.insert("max_inclusive", 7_i16);
        for (tag, expected) in [
            (NbtTag::Int(0), LightRange { min: 0, max: 0 }),
            (NbtTag::Byte(5), LightRange { min: 5, max: 5 }),
            (
                NbtTag::List(NbtList::Int(vec![2, 7])),
                LightRange { min: 2, max: 7 },
            ),
            (NbtTag::IntArray(vec![2, 7]), LightRange { min: 2, max: 7 }),
            (NbtTag::Compound(compound), LightRange { min: 2, max: 7 }),
        ] {
            for key in ["block_light_limit", "sky_light_limit"] {
                let mut root = NbtCompound::new();
                root.insert(key, tag.clone());
                let mut bytes = Vec::new();
                root.write(&mut bytes);
                let borrowed = read_compound(&mut Cursor::new(bytes.as_slice()))
                    .expect("light range NBT should be readable");
                let rules = CustomSpawnRules::from_nbt(&(&borrowed).into())
                    .expect("vanilla interval form should decode");
                let range = if key == "block_light_limit" {
                    rules.block_light_limit
                } else {
                    rules.sky_light_limit
                };
                assert_eq!(range, expected);
            }
        }
    }

    #[test]
    fn custom_spawner_light_ranges_default_malformed_but_reject_out_of_bounds() {
        for (tag, expected) in [
            (
                NbtTag::List(NbtList::Int(vec![7, 2])),
                Some(LightRange::DEFAULT),
            ),
            (
                NbtTag::List(NbtList::Int(vec![2])),
                Some(LightRange::DEFAULT),
            ),
            (
                NbtTag::Compound(NbtCompound::new()),
                Some(LightRange::DEFAULT),
            ),
            (NbtTag::Int(16), None),
            (NbtTag::List(NbtList::Int(vec![-1, 7])), None),
        ] {
            let mut root = NbtCompound::new();
            root.insert("block_light_limit", tag);
            let mut bytes = Vec::new();
            root.write(&mut bytes);
            let borrowed = read_compound(&mut Cursor::new(bytes.as_slice()))
                .expect("light range NBT should be readable");
            let rules = CustomSpawnRules::from_nbt(&(&borrowed).into());
            assert_eq!(rules.map(|rules| rules.block_light_limit), expected);
        }
    }

    #[test]
    fn unsupported_spawn_data_delays_and_reselects_without_inserting() {
        init_vanilla_registry();
        init_entities();

        let mut root = NbtCompound::new();
        root.insert("Delay", 0_i16);
        root.insert("MinSpawnDelay", 10_i32);
        root.insert("MaxSpawnDelay", 10_i32);
        root.insert("SpawnCount", 1_i32);

        root.insert("SpawnData", spawn_data("minecraft:blaze").to_nbt());

        let mut potential = NbtCompound::new();
        potential.insert("data", spawn_data("minecraft:pig").to_nbt());
        potential.insert("weight", 1_i32);
        root.insert("SpawnPotentials", NbtList::Compound(vec![potential]));

        let spawner = load_spawner(&root);

        let world = fresh_test_world("spawner_skips_unsupported_entity");
        let player = TestPlayerBuilder::new(Arc::clone(&world), "SpawnerPlayer", 1).build();
        assert!(world.add_player(player, ResetReason::InitialJoin));

        let result = spawner.server_tick(&world, BlockPos::ZERO);

        assert!(result.state_changed);
        assert!(result.next_spawn_data_changed);
        assert_eq!(spawner.spawn_delay(), 10);
        assert_eq!(
            spawner.get_or_create_next_spawn_data(),
            spawn_data("minecraft:pig")
        );
        assert!(!world.has_entity_in_aabb_matching(
            &WorldAabb::new(-4.0, -4.0, -4.0, 5.0, 5.0, 5.0),
            |entity| entity.as_player().is_none(),
        ));
    }

    #[test]
    fn weighted_selection_uses_only_positive_weights_and_preserves_order() {
        let entries = [
            WeightedSpawnData {
                weight: -10,
                data: spawn_data("minecraft:pig"),
            },
            WeightedSpawnData {
                weight: 1,
                data: spawn_data("minecraft:zombie"),
            },
            WeightedSpawnData {
                weight: 3,
                data: spawn_data("minecraft:skeleton"),
            },
        ];

        for (random, expected) in [(0, "minecraft:zombie"), (1_u64 << 62, "minecraft:skeleton")] {
            assert_eq!(
                choose_weighted(&entries, &mut FixedRng(random)),
                Some(spawn_data(expected))
            );
        }
    }

    #[test]
    fn nbt_round_trip_keeps_short_spawner_fields_and_spawn_data() {
        let root = parse_snbt_compound(
            r#"{
            Delay:37s, MinSpawnDelay:40, MaxSpawnDelay:120, SpawnCount:5s,
            MaxNearbyEntities:9, RequiredPlayerRange:20s, SpawnRange:6,
            SpawnData:{
                entity:{id:"minecraft:zombie"},
                custom_spawn_rules:{
                    block_light_limit:{min_inclusive:2,max_inclusive:7},
                    sky_light_limit:{min_inclusive:1,max_inclusive:5}
                },
                equipment:{loot_table:"minecraft:chests/simple_dungeon",slot_drop_chances:0.25f}
            },
            SpawnPotentials:[{data:{entity:{id:"minecraft:skeleton"}},weight:2}]
        }"#,
        )
        .expect("valid spawner fixture");
        let spawner = load_spawner(&root);
        let mut saved = NbtCompound::new();
        spawner.save(&mut saved);
        for (key, expected) in [
            ("Delay", 37),
            ("MinSpawnDelay", 40),
            ("MaxSpawnDelay", 120),
            ("SpawnCount", 5),
            ("MaxNearbyEntities", 9),
            ("RequiredPlayerRange", 20),
            ("SpawnRange", 6),
        ] {
            assert_eq!(saved.short(key), Some(expected), "{key}");
        }
        for key in ["SpawnData", "SpawnPotentials"] {
            assert_eq!(saved.get(key), root.get(key), "{key}");
        }
    }

    #[test]
    fn update_tag_omits_spawn_potentials_for_client_sync() {
        init_vanilla_registry();
        let mut root = NbtCompound::new();
        root.insert("SpawnData", spawn_data("minecraft:zombie").to_nbt());
        root.insert(
            "SpawnPotentials",
            NbtList::Compound(vec![NbtCompound::new()]),
        );
        let mut bytes = Vec::new();
        root.write(&mut bytes);
        let borrowed = read_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("spawner NBT should be readable");

        let spawner = SpawnerBlockEntity::new(
            Weak::new(),
            BlockPos::ZERO,
            vanilla_blocks::SPAWNER.default_state(),
        );
        spawner.load_additional(&borrowed);
        let update_tag = spawner
            .get_update_tag()
            .expect("spawner should provide an update tag");

        assert!(update_tag.get("SpawnPotentials").is_none());
        assert!(update_tag.get("SpawnData").is_some());
    }

    #[test]
    fn set_entity_id_normalizes_the_active_spawn_data() {
        init_vanilla_registry();
        let spawner = BaseSpawner::new();
        spawner.set_entity_id(&vanilla_entities::BLAZE);

        let mut saved = NbtCompound::new();
        spawner.save(&mut saved);
        assert_eq!(
            saved.compound("SpawnData"),
            Some(&spawn_data("minecraft:blaze").to_nbt())
        );
    }

    #[test]
    fn client_spawn_event_resets_to_minimum_delay_only_for_event_one() {
        let mut root = NbtCompound::new();
        root.insert("Delay", 100_i16);
        root.insert("MinSpawnDelay", 33_i32);
        let spawner = load_spawner(&root);

        assert!(!spawner.on_client_event_triggered(2));
        assert_eq!(spawner.spawn_delay(), 100);
        assert!(spawner.on_client_event_triggered(1));
        assert_eq!(spawner.spawn_delay(), 33);
    }
}
