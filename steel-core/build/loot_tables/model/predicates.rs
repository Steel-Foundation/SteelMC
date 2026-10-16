use super::{Bounds, IntRange, yes};
use serde::{Deserialize, Deserializer, de::IgnoredAny};
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemPredicate {
    // TODO: Support vanilla holder-set lists and exact component predicates.
    pub items: Option<String>,
    pub count: Option<IntRange>,
    pub predicates: Option<ItemComponentPredicates>,
}

#[derive(Deserialize)]
pub struct ItemComponentPredicates {
    #[serde(rename = "minecraft:enchantments")]
    pub enchantments: Option<Vec<EnchantmentPredicate>>,
    // The currently supported non-enchantment predicates require an empty
    // payload. Unknown nonempty codecs fail decoding instead of becoming presence checks.
    #[expect(
        clippy::zero_sized_map_values,
        reason = "JSON objects map component keys to validated empty predicate payloads"
    )]
    #[serde(flatten)]
    pub presence: BTreeMap<String, EmptyPredicate>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyPredicate {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnchantmentPredicate {
    // TODO: Support optional enchantment holder sets and exact/ranged levels.
    pub enchantments: String,
    pub levels: Option<Bounds<i32>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityPredicate {
    // TODO: Add vanilla entity subpredicates: distance, effects, entity_tags,
    // location, movement, movement_affected_by, nbt, passenger, periodic_tick,
    // predicates, slots, stepping_on, targeted_entity, team,
    // type_specific/lightning, and type_specific/player.
    #[serde(rename = "minecraft:entity_type")]
    pub entity_type: Option<String>,
    #[serde(rename = "minecraft:flags")]
    pub flags: Option<BTreeMap<EntityFlag, bool>>,
    #[serde(rename = "minecraft:equipment")]
    pub equipment: Option<BTreeMap<EquipmentSlot, ItemPredicate>>,
    #[serde(rename = "minecraft:components")]
    pub components: Option<EntityComponents>,
    #[serde(rename = "minecraft:type_specific/sheep")]
    pub sheep: Option<SheepPredicate>,
    #[serde(
        default,
        rename = "minecraft:vehicle",
        deserialize_with = "deferred_predicate"
    )]
    pub vehicle: Option<IgnoredAny>,
    #[serde(
        default,
        rename = "minecraft:type_specific/cube_mob",
        deserialize_with = "deferred_predicate"
    )]
    pub cube_mob: Option<IgnoredAny>,
    #[serde(
        default,
        rename = "minecraft:type_specific/fishing_hook",
        deserialize_with = "deferred_predicate"
    )]
    pub fishing_hook: Option<IgnoredAny>,
    #[serde(
        default,
        rename = "minecraft:type_specific/raider",
        deserialize_with = "deferred_predicate"
    )]
    pub raider: Option<IgnoredAny>,
}

// Even a null payload must retain the unsupported key instead of becoming an
// absent predicate through Option's usual null handling.
fn deferred_predicate<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<IgnoredAny>, D::Error> {
    IgnoredAny::deserialize(deserializer).map(Some)
}

// Ordering preserves the previous lexically ordered JSON predicate evaluation.
#[derive(Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntityFlag {
    #[serde(rename = "is_baby")]
    Baby,
    #[serde(rename = "is_on_fire")]
    OnFire,
    #[serde(rename = "is_sneaking")]
    Sneaking,
    #[serde(rename = "is_sprinting")]
    Sprinting,
    #[serde(rename = "is_swimming")]
    Swimming,
}

#[derive(Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum EquipmentSlot {
    Body,
    Chest,
    Feet,
    Head,
    Legs,
    Mainhand,
    Offhand,
    Saddle,
}

#[derive(Deserialize)]
pub struct EntityComponents {
    #[serde(rename = "minecraft:sheep/color")]
    pub sheep_color: Option<String>,
    #[serde(rename = "minecraft:chicken/variant")]
    pub chicken_variant: Option<String>,
    // Component payloads without a supported entity capability remain explicit
    // runtime errors, including dynamically named component types.
    #[expect(
        clippy::zero_sized_map_values,
        reason = "deferred component payloads are discarded but their object keys must be retained for errors"
    )]
    #[serde(flatten)]
    pub unsupported: BTreeMap<String, IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheepPredicate {
    pub sheared: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DamageSourcePredicate {
    #[serde(default)]
    pub tags: Vec<TagPredicate>,
    pub source_entity: Option<EntityPredicate>,
    pub direct_entity: Option<EntityPredicate>,
    pub is_direct: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagPredicate {
    pub id: String,
    #[serde(default = "yes")]
    pub expected: bool,
}
