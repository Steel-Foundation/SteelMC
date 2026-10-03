//! Spawner `SpawnData` and its optional light and equipment rules.

use simdnbt::borrow::NbtCompound as NbtCompoundView;
use simdnbt::borrow::NbtTag as BorrowedNbtTag;
use simdnbt::owned::NbtCompound;
use steel_utils::nbt::NbtNumeric;
use steel_utils::{BlockPos, Identifier};

use crate::chunk::light::LightLayer;
use crate::inventory::equipment::EquipmentSlot;
use crate::world::World;

/// Inclusive light range used by custom spawner rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightRange {
    pub min: i32,
    pub max: i32,
}

impl LightRange {
    pub(super) const DEFAULT: Self = Self { min: 0, max: 15 };
    fn from_nbt(tag: Option<BorrowedNbtTag<'_, '_>>) -> Option<Self> {
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
    pub block_light_limit: LightRange,
    pub sky_light_limit: LightRange,
}

impl CustomSpawnRules {
    pub(super) fn from_nbt(nbt: &NbtCompoundView<'_, '_>) -> Option<Self> {
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
    pub loot_table: Identifier,
    pub slot_drop_chances: Vec<(EquipmentSlot, f32)>,
}

impl EquipmentTable {
    fn from_nbt(nbt: &NbtCompoundView<'_, '_>) -> Option<Self> {
        let loot_table = nbt.string("loot_table")?.to_str().parse().ok()?;
        let mut slot_drop_chances = Vec::new();
        match nbt.get("slot_drop_chances") {
            Some(tag) if tag.codec_f32().is_some() => {
                let chance = tag.codec_f32()?;
                for slot in EquipmentSlot::ALL {
                    slot_drop_chances.push((slot, chance));
                }
            }
            Some(tag) => {
                let chances = tag.compound()?;
                for (name, value) in chances.iter() {
                    let name = name.to_str();
                    let slot = EquipmentSlot::by_name(name.as_ref())?;
                    slot_drop_chances.push((slot, value.codec_f32()?));
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
    pub weight: i32,
    pub data: SpawnData,
}

/// Entity and optional rules used by a spawner.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnData {
    pub entity_to_spawn: NbtCompound,
    pub custom_spawn_rules: Option<CustomSpawnRules>,
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
    pub(super) fn from_nbt(nbt: &NbtCompoundView<'_, '_>) -> Option<Self> {
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

    pub(super) fn to_nbt(&self) -> NbtCompound {
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
