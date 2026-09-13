//! Normal mob-spawner block entity and its persistent spawn configuration.

use std::sync::Weak;

use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_block_entity_types;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, Identifier};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::chunk::light::LightLayer;
use crate::inventory::equipment::EquipmentSlot;
use crate::world::World;

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

    fn from_nbt(nbt: &NbtCompoundView<'_, '_>) -> Option<Self> {
        let min = nbt.int("min_inclusive").unwrap_or(Self::DEFAULT.min);
        let max = nbt.int("max_inclusive").unwrap_or(Self::DEFAULT.max);
        (0..=15)
            .contains(&min)
            .then_some(())
            .and_then(|()| (0..=15).contains(&max).then_some(()))
            .and_then(|()| (min <= max).then_some(Self { min, max }))
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
        let block_light_limit = nbt
            .compound("block_light_limit")
            .map_or(Some(LightRange::DEFAULT), |range| {
                LightRange::from_nbt(&range)
            });
        let sky_light_limit = nbt
            .compound("sky_light_limit")
            .map_or(Some(LightRange::DEFAULT), |range| {
                LightRange::from_nbt(&range)
            });
        Some(Self {
            block_light_limit: block_light_limit?,
            sky_light_limit: sky_light_limit?,
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
    /// Handles vanilla block event `1`.
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "The event method mirrors the vanilla BaseSpawner delegation API."
    )]
    pub const fn on_event_triggered(&self, event: i32) -> bool {
        event == EVENT_SPAWN
    }
}
fn read_i32(nbt: &NbtCompoundView<'_, '_>, key: &str, default: i32) -> i32 {
    nbt.int(key)
        .or_else(|| nbt.short(key).map(i32::from))
        .or_else(|| nbt.byte(key).map(i32::from))
        .unwrap_or(default)
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

    fn trigger_event(&self, event: i32, _data: i32) -> bool {
        self.spawner.on_event_triggered(event)
    }
}
#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::{borrow::read_compound, owned::NbtTag};

    use super::*;

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
    #[test]
    fn nbt_round_trip_keeps_short_spawner_fields_and_spawn_data() {
        let mut root = NbtCompound::new();
        root.insert("Delay", 37_i16);
        root.insert("MinSpawnDelay", 40_i32);
        root.insert("MaxSpawnDelay", 120_i32);
        root.insert("SpawnCount", 5_i16);
        root.insert("MaxNearbyEntities", 9_i32);
        root.insert("RequiredPlayerRange", 20_i16);
        root.insert("SpawnRange", 6_i32);

        let mut rules = NbtCompound::new();
        let mut block_light = NbtCompound::new();
        block_light.insert("min_inclusive", 2_i32);
        block_light.insert("max_inclusive", 7_i32);
        let mut sky_light = NbtCompound::new();
        sky_light.insert("min_inclusive", 1_i32);
        sky_light.insert("max_inclusive", 5_i32);
        rules.insert("block_light_limit", block_light);
        rules.insert("sky_light_limit", sky_light);

        let mut active = NbtCompound::new();
        active.insert("entity", entity("minecraft:zombie"));
        active.insert("custom_spawn_rules", rules);
        let mut equipment = NbtCompound::new();
        equipment.insert("loot_table", "minecraft:chests/simple_dungeon");
        equipment.insert("slot_drop_chances", 0.25_f32);
        active.insert("equipment", equipment);
        root.insert("SpawnData", active);

        let mut potential = NbtCompound::new();
        potential.insert("data", spawn_data("minecraft:skeleton").to_nbt());
        potential.insert("weight", 2_i32);
        root.insert("SpawnPotentials", NbtList::Compound(vec![potential]));

        let mut bytes = Vec::new();
        root.write(&mut bytes);
        let borrowed = read_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("spawner NBT should be readable");
        let spawner = BaseSpawner::new();
        spawner.load(&borrowed);

        let mut saved = NbtCompound::new();
        spawner.save(&mut saved);
        assert_eq!(saved.short("Delay"), Some(37));
        assert_eq!(saved.short("MinSpawnDelay"), Some(40));
        assert_eq!(saved.short("MaxSpawnDelay"), Some(120));
        assert_eq!(saved.short("SpawnCount"), Some(5));
        assert_eq!(saved.short("MaxNearbyEntities"), Some(9));
        assert_eq!(saved.short("RequiredPlayerRange"), Some(20));
        assert_eq!(saved.short("SpawnRange"), Some(6));
        assert_eq!(
            saved
                .compound("SpawnData")
                .and_then(|data| data.compound("entity"))
                .and_then(|entity| entity.string("id"))
                .map(|id| id.to_str().into_owned()),
            Some("minecraft:zombie".to_owned())
        );
        assert_eq!(
            saved
                .compound("SpawnData")
                .and_then(|data| data.compound("custom_spawn_rules"))
                .and_then(|rules| rules.compound("block_light_limit"))
                .and_then(|range| range.int("max_inclusive")),
            Some(7)
        );
        assert!(matches!(
            saved
                .compound("SpawnData")
                .and_then(|data| data.compound("equipment"))
                .and_then(|data| data.get("slot_drop_chances")),
            Some(tag) if tag.float() == Some(0.25)
        ));
        assert!(matches!(
            saved.get("SpawnPotentials"),
            Some(NbtTag::List(NbtList::Compound(entries))) if entries.len() == 1
        ));
    }
}
