use steel_registry::biome::BiomeRef;
use steel_registry::entity_variant::{FoxVariant, RabbitVariant};
use steel_registry::items::ItemRef;
use steel_registry::shared_structs::pick_spawn_conditioned_entry;
use steel_registry::wolf_variant::WolfVariantRef;
use steel_registry::{REGISTRY, RegistryExt, vanilla_biome_tags::BiomeTag, vanilla_items};
use steel_utils::random::{Random, RandomSource};
use steel_utils::value_providers::UniformIntProvider;

use crate::entity::{AgeableMobGroupData, WorldgenSpawnProperties};

use super::required_variant;

const RABBIT_COLOR_ROLL_BOUND: i32 = 100;
const WHITE_RABBIT_WEIGHT: i32 = 80;
const BROWN_RABBIT_WEIGHT: i32 = 50;
const SALT_RABBIT_WEIGHT: i32 = 40;
const FOX_MOUTH_ITEM_CHANCE: f32 = 0.2;
const FOX_EMERALD_CHANCE: f32 = 0.05;
const FOX_EGG_CUTOFF: f32 = 0.2;
const FOX_RABBIT_DROP_CUTOFF: f32 = 0.4;
const FOX_WHEAT_CUTOFF: f32 = 0.6;
const FOX_LEATHER_CUTOFF: f32 = 0.8;
const FROG_LONG_JUMP_COOLDOWN_TICKS: UniformIntProvider = UniformIntProvider {
    min_inclusive: 100,
    max_inclusive: 140,
};

pub(super) fn frog_properties(
    biome: BiomeRef,
    random: &mut RandomSource,
) -> WorldgenSpawnProperties {
    WorldgenSpawnProperties::Frog {
        variant: pick_spawn_conditioned_entry(
            REGISTRY.frog_variants.iter().map(|(_, variant)| variant),
            |variant| variant.spawn_conditions,
            biome,
            random,
        ),
        long_jump_cooldown_ticks: FROG_LONG_JUMP_COOLDOWN_TICKS.sample(random),
    }
}

pub(super) fn wolf_properties(
    group_variant: &mut Option<WolfVariantRef>,
    group: &mut AgeableMobGroupData,
    biome: BiomeRef,
    random: &mut RandomSource,
) -> WorldgenSpawnProperties {
    if group_variant.is_none() {
        *group_variant = pick_spawn_conditioned_entry(
            REGISTRY.wolf_variants.iter().map(|(_, variant)| variant),
            |variant| variant.spawn_conditions,
            biome,
            random,
        );

        if group_variant.is_some() {
            *group = AgeableMobGroupData::with_should_spawn_baby(false);
        }
    }

    let sound_registry = &REGISTRY.wolf_sound_variants;
    let sound_id = random.next_i32_bounded(sound_registry.len() as i32) as usize;

    WorldgenSpawnProperties::Wolf {
        variant: *group_variant,
        sound_variant: required_variant(sound_registry.by_id(sound_id)),
    }
}

pub(super) fn fox_variant(biome: BiomeRef) -> FoxVariant {
    if biome.has_tag(&BiomeTag::SPAWNS_SNOW_FOXES) {
        FoxVariant::Snow
    } else {
        FoxVariant::Red
    }
}

pub(super) fn fox_mouth_item(random: &mut RandomSource) -> Option<ItemRef> {
    if random.next_f32() >= FOX_MOUTH_ITEM_CHANCE {
        return None;
    }

    let odds = random.next_f32();
    Some(if odds < FOX_EMERALD_CHANCE {
        &vanilla_items::EMERALD
    } else if odds < FOX_EGG_CUTOFF {
        &vanilla_items::EGG
    } else if odds < FOX_RABBIT_DROP_CUTOFF {
        if random.next_bool() {
            &vanilla_items::RABBIT_FOOT
        } else {
            &vanilla_items::RABBIT_HIDE
        }
    } else if odds < FOX_WHEAT_CUTOFF {
        &vanilla_items::WHEAT
    } else if odds < FOX_LEATHER_CUTOFF {
        &vanilla_items::LEATHER
    } else {
        &vanilla_items::FEATHER
    })
}

pub(super) fn rabbit_variant(biome: BiomeRef, random: &mut RandomSource) -> RabbitVariant {
    let roll = random.next_i32_bounded(RABBIT_COLOR_ROLL_BOUND);
    if biome.has_tag(&BiomeTag::SPAWNS_WHITE_RABBITS) {
        if roll < WHITE_RABBIT_WEIGHT {
            RabbitVariant::White
        } else {
            RabbitVariant::WhiteSplotched
        }
    } else if biome.has_tag(&BiomeTag::SPAWNS_GOLD_RABBITS) {
        RabbitVariant::Gold
    } else if roll < BROWN_RABBIT_WEIGHT {
        RabbitVariant::Brown
    } else if roll < BROWN_RABBIT_WEIGHT + SALT_RABBIT_WEIGHT {
        RabbitVariant::Salt
    } else {
        RabbitVariant::Black
    }
}
