use std::{cmp::Ordering, collections::BTreeMap, str::FromStr};

use serde::{Deserialize, Deserializer, de::Error as _};
use simdnbt::ToNbtTag;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_utils::Identifier;
use steel_utils::random::Random;

use crate::biome::BiomeRef;
use crate::{REGISTRY, TaggedRegistryExt};

/// Block state data as encoded by vanilla registry JSON.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockStateData {
    /// Block identifier.
    #[serde(rename = "Name")]
    pub name: Identifier,
    /// String-valued block-state properties.
    #[serde(rename = "Properties", default)]
    pub properties: BTreeMap<String, String>,
}

/// Fluid state data as encoded by vanilla registry JSON.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FluidStateData {
    /// Fluid identifier.
    #[serde(rename = "Name")]
    pub name: Identifier,
    /// String-valued fluid-state properties.
    #[serde(rename = "Properties", default)]
    pub properties: BTreeMap<String, String>,
}

pub fn deserialize_tag_identifier<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Identifier, D::Error> {
    let value = String::deserialize(deserializer)?;
    let tag = value.strip_prefix('#').unwrap_or(&value);
    Identifier::from_str(tag).map_err(D::Error::custom)
}

pub fn deserialize_optional_tag_identifier<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Identifier>, D::Error> {
    let Some(value) = Option::<String>::deserialize(deserializer)? else {
        return Ok(None);
    };
    let tag = value.strip_prefix('#').unwrap_or(&value);
    Identifier::from_str(tag)
        .map(Some)
        .map_err(D::Error::custom)
}

/// A single entry in the list of spawn conditions.
#[derive(Debug)]
pub struct SpawnConditionEntry {
    pub priority: i32,
    pub condition: Option<BiomeCondition>,
}

impl SpawnConditionEntry {
    #[must_use]
    pub fn matches_biome(&self, biome: BiomeRef) -> bool {
        self.condition
            .as_ref()
            .is_none_or(|condition| condition.matches_biome(biome))
    }
}

impl ToNbtTag for &SpawnConditionEntry {
    fn to_nbt_tag(self) -> NbtTag {
        let mut e = NbtCompound::new();
        e.insert("priority", self.priority);
        if let Some(cond) = &self.condition {
            e.insert("condition", cond.to_nbt_tag());
        }
        NbtTag::Compound(e)
    }
}

/// Defines a condition based on a biome or list of biomes.
#[derive(Debug)]
pub struct BiomeCondition {
    pub condition_type: &'static str,
    pub biomes: BiomeConditionTarget,
}

impl BiomeCondition {
    #[must_use]
    pub fn matches_biome(&self, biome: BiomeRef) -> bool {
        if self.condition_type != "minecraft:biome" {
            return false;
        }

        self.biomes.matches_biome(biome)
    }
}

/// Vanilla spawn-condition biome target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BiomeConditionTarget {
    /// A biome tag target encoded as `#namespace:path` in registry data.
    Tag(Identifier),
    /// A direct biome target encoded as `namespace:path` in registry data.
    Direct(Identifier),
}

impl BiomeConditionTarget {
    #[must_use]
    pub fn matches_biome(&self, biome: BiomeRef) -> bool {
        match self {
            Self::Tag(tag) => REGISTRY.biomes.is_in_tag(biome, tag),
            Self::Direct(key) => &biome.key == key,
        }
    }

    fn to_vanilla_string(&self) -> String {
        match self {
            Self::Tag(tag) => format!("#{tag}"),
            Self::Direct(key) => key.to_string(),
        }
    }
}

impl ToNbtTag for &BiomeCondition {
    fn to_nbt_tag(self) -> NbtTag {
        let mut c = NbtCompound::new();
        c.insert("type", self.condition_type);
        c.insert("biomes", self.biomes.to_vanilla_string());
        NbtTag::Compound(c)
    }
}

/// Picks entries using vanilla `PriorityProvider.pick` semantics.
pub fn pick_spawn_conditioned_entry<T: Copy>(
    entries: impl IntoIterator<Item = T>,
    selectors: impl Fn(T) -> &'static [SpawnConditionEntry],
    biome: BiomeRef,
    random: &mut impl Random,
) -> Option<T> {
    let mut selected = Vec::new();
    let mut highest_priority = i32::MIN;

    for entry in entries {
        for selector in selectors(entry) {
            if !selector.matches_biome(biome) {
                continue;
            }

            match selector.priority.cmp(&highest_priority) {
                Ordering::Greater => {
                    selected.clear();
                    selected.push(entry);
                    highest_priority = selector.priority;
                }
                Ordering::Equal => selected.push(entry),
                Ordering::Less => {}
            }
        }
    }

    let bound = i32::try_from(selected.len()).ok()?;
    if bound == 0 {
        return None;
    }

    let index = random.next_i32_bounded(bound) as usize;
    selected.get(index).copied()
}

/// Serialize a `spawn_conditions` list into the enclosing compound.
/// Matches vanilla's `[{priority, condition?}, …]` shape exactly.
pub fn insert_spawn_conditions(compound: &mut NbtCompound, entries: &[SpawnConditionEntry]) {
    let list: Vec<NbtCompound> = entries
        .iter()
        .map(|entry| {
            let mut e = NbtCompound::new();
            e.insert("priority", entry.priority);
            if let Some(cond) = &entry.condition {
                e.insert("condition", cond.to_nbt_tag());
            }
            e
        })
        .collect();
    compound.insert("spawn_conditions", NbtTag::List(NbtList::Compound(list)));
}

#[cfg(test)]
mod tests {
    use steel_utils::random::{Random, RandomSplitter};

    use crate::{test_support::init_test_registry, vanilla_biomes};

    use super::{SpawnConditionEntry, pick_spawn_conditioned_entry};

    static HIGH_DUPLICATE_SELECTORS: [SpawnConditionEntry; 2] = [
        SpawnConditionEntry {
            priority: 2,
            condition: None,
        },
        SpawnConditionEntry {
            priority: 2,
            condition: None,
        },
    ];
    static HIGH_SINGLE_SELECTOR: [SpawnConditionEntry; 1] = [SpawnConditionEntry {
        priority: 2,
        condition: None,
    }];
    static LOWER_SELECTOR: [SpawnConditionEntry; 1] = [SpawnConditionEntry {
        priority: 1,
        condition: None,
    }];

    struct IndexRandom {
        index: i32,
    }

    impl Random for IndexRandom {
        fn fork(&mut self) -> Self {
            unreachable!("selector tests do not fork random")
        }

        fn next_i32(&mut self) -> i32 {
            unreachable!("selector tests only use bounded random")
        }

        fn next_i32_bounded(&mut self, bound: i32) -> i32 {
            assert!(self.index < bound);
            self.index
        }

        fn next_i64(&mut self) -> i64 {
            unreachable!("selector tests only use bounded random")
        }

        fn next_f32(&mut self) -> f32 {
            unreachable!("selector tests only use bounded random")
        }

        fn next_f64(&mut self) -> f64 {
            unreachable!("selector tests only use bounded random")
        }

        fn next_bool(&mut self) -> bool {
            unreachable!("selector tests only use bounded random")
        }

        fn next_gaussian(&mut self) -> f64 {
            unreachable!("selector tests only use bounded random")
        }

        fn next_positional(&mut self) -> RandomSplitter {
            unreachable!("selector tests only use bounded random")
        }
    }

    #[test]
    fn pick_spawn_conditioned_entry_keeps_duplicate_highest_priority_matches() {
        init_test_registry();

        let mut random = IndexRandom { index: 1 };
        let selected = pick_spawn_conditioned_entry(
            [1, 2, 3],
            |entry| match entry {
                1 => &HIGH_DUPLICATE_SELECTORS,
                2 => &HIGH_SINGLE_SELECTOR,
                _ => &LOWER_SELECTOR,
            },
            &vanilla_biomes::PLAINS,
            &mut random,
        );

        assert_eq!(selected, Some(1));
    }
}
