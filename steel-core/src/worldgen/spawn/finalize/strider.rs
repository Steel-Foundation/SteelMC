use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::{
    REGISTRY, TaggedRegistryExt, vanilla_enchantment_tags::EnchantmentTag, vanilla_entities,
    vanilla_items,
};
use steel_utils::random::{Random, RandomSource};
use steel_utils::types::Difficulty;

use crate::enchantment_helper::select_enchantment;
use crate::entity::{AgeableMobGroupData, MobSpawnState, WorldgenSpawnProperties};
use crate::special_dates::is_halloween;
use crate::world::DifficultyInstance;

use super::{SpawnData, SpawnOverrides};

const PIGLIN_JOCKEY_ODDS: i32 = 30;
const STRIDER_JOCKEY_ODDS: i32 = 10;
const STRIDER_BABY_CHANCE: f32 = 0.5;
const ZOMBIE_BABY_CHANCE: f32 = 0.05;
const ZOMBIE_PICKUP_SCALE: f32 = 0.55;
const ZOMBIE_DOOR_BREAK_SCALE: f32 = 0.1;
const PIGLIN_SPEAR_ODDS: i32 = 20;
const SPAWN_WEAPON_ENCHANT_CHANCE: f32 = 0.25;
const SPAWN_EQUIPMENT_ENCHANT_MIN_COST: i32 = 5;
const SPAWN_EQUIPMENT_ENCHANT_COST_SPAN: f32 = 17.0;
const HALLOWEEN_PUMPKIN_CHANCE: f32 = 0.25;
const HALLOWEEN_JACK_O_LANTERN_CHANCE: f32 = 0.1;

pub(super) fn sample_passenger(
    ageable: AgeableMobGroupData,
    difficulty: DifficultyInstance,
    spawning_monsters: bool,
    random: &mut RandomSource,
) -> (AgeableMobGroupData, Option<SpawnData>, bool) {
    let no_group_babies = AgeableMobGroupData::with_baby_spawn_chance(0.0);
    if random.next_i32_bounded(PIGLIN_JOCKEY_ODDS) == 0 && spawning_monsters {
        if vanilla_entities::ZOMBIFIED_PIGLIN.allowed_in_peaceful
            || difficulty.difficulty() != Difficulty::Peaceful
        {
            (
                no_group_babies,
                Some(sample_piglin(difficulty, random)),
                true,
            )
        } else {
            (ageable, None, false)
        }
    } else if random.next_i32_bounded(STRIDER_JOCKEY_ODDS) == 0 {
        let passenger = SpawnData {
            entity_type: &vanilla_entities::STRIDER,
            mob: MobSpawnState::sample(random),
            overrides: SpawnOverrides {
                is_baby: true,
                ..SpawnOverrides::default()
            },
        };

        (no_group_babies, Some(passenger), false)
    } else {
        (
            AgeableMobGroupData::with_baby_spawn_chance(STRIDER_BABY_CHANCE),
            None,
            false,
        )
    }
}

// TODO: Share these rolls with the zombified piglin's spawn initialization once implemented.
fn sample_piglin(difficulty: DifficultyInstance, random: &mut RandomSource) -> SpawnData {
    let is_baby = random.next_f32() < ZOMBIE_BABY_CHANCE;
    let mob = MobSpawnState::sample(random);
    let difficulty_multiplier = difficulty.special_multiplier();
    let can_pick_up_loot = random.next_f32() < ZOMBIE_PICKUP_SCALE * difficulty_multiplier;
    let can_break_doors = random.next_f32() < ZOMBIE_DOOR_BREAK_SCALE * difficulty_multiplier;

    let weapon = if random.next_i32_bounded(PIGLIN_SPEAR_ODDS) == 0 {
        &vanilla_items::GOLDEN_SPEAR
    } else {
        &vanilla_items::GOLDEN_SWORD
    };

    if random.next_f32() < SPAWN_WEAPON_ENCHANT_CHANCE * difficulty_multiplier {
        let cost = random.next_i32_between(
            SPAWN_EQUIPMENT_ENCHANT_MIN_COST,
            SPAWN_EQUIPMENT_ENCHANT_MIN_COST
                + (difficulty_multiplier * SPAWN_EQUIPMENT_ENCHANT_COST_SPAN) as i32,
        );

        let _ = select_enchantment(
            random,
            &ItemStack::new(weapon),
            cost,
            REGISTRY
                .enchantments
                .iter_tag(&EnchantmentTag::ON_MOB_SPAWN_EQUIPMENT),
        );
    }

    let head_item = if is_halloween() && random.next_f32() < HALLOWEEN_PUMPKIN_CHANCE {
        let item: ItemRef = if random.next_f32() < HALLOWEEN_JACK_O_LANTERN_CHANCE {
            &vanilla_items::JACK_O_LANTERN
        } else {
            &vanilla_items::CARVED_PUMPKIN
        };

        Some(item)
    } else {
        None
    };

    SpawnData {
        entity_type: &vanilla_entities::ZOMBIFIED_PIGLIN,
        mob,
        overrides: SpawnOverrides {
            is_baby,
            properties: WorldgenSpawnProperties::ZombifiedPiglin {
                can_break_doors,
                difficulty_multiplier,
            },
            main_hand_item: Some(&vanilla_items::WARPED_FUNGUS_ON_A_STICK),
            head_item,
            can_pick_up_loot: Some(can_pick_up_loot),
            ..SpawnOverrides::default()
        },
    }
}
