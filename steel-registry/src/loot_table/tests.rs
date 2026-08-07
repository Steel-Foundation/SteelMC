use crate::data_components::vanilla_components::INSTRUMENT;
use crate::vanilla_instrument_tags::InstrumentTag;
use crate::vanilla_items;
use crate::{init_vanilla_registry, vanilla_loot_tables};

use super::*;
use rand::SeedableRng;

fn test_rng() -> rand::rngs::StdRng {
    rand::rngs::StdRng::seed_from_u64(12345)
}

fn init_test_registries() {
    init_vanilla_registry();
}

#[test]
fn test_oak_log_loot() {
    init_test_registries();
    let mut rng = test_rng();

    let mut ctx = LootContext::new(&mut rng);
    let items = vanilla_loot_tables::BLOCKS_OAK_LOG.get_random_items(&mut ctx);

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].count, 1);
    assert_eq!(items[0].item.key, Identifier::vanilla_static("oak_log"));
}

#[test]
fn set_instrument_selects_from_the_configured_holder_set() {
    init_test_registries();
    let mut rng = test_rng();
    let mut ctx = LootContext::new(&mut rng);
    let mut goat_horn = ItemStack::new(&vanilla_items::GOAT_HORN);
    let function = LootFunction::SetInstrument {
        options: InstrumentOptions::Tag(InstrumentTag::REGULAR_GOAT_HORNS),
    };

    function.apply(&mut goat_horn, &mut ctx);

    let selected = goat_horn
        .get(INSTRUMENT)
        .and_then(|component| component.instrument().as_reference())
        .expect("set_instrument should select a registered instrument");
    assert!(
        REGISTRY
            .instruments
            .is_in_tag(selected, &InstrumentTag::REGULAR_GOAT_HORNS)
    );
}

#[test]
fn test_diamond_ore_loot_no_silk_touch() {
    // Without silk touch, diamond ore should drop diamond (not the ore block)
    init_test_registries();
    let mut rng = test_rng();

    let mut ctx = LootContext::new(&mut rng);
    let items = vanilla_loot_tables::BLOCKS_DIAMOND_ORE.get_random_items(&mut ctx);

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].count, 1);
    // Without silk touch enchantment, diamond ore drops diamond
    assert_eq!(items[0].item.key, Identifier::vanilla_static("diamond"));
}

#[test]
fn test_grass_block_loot_no_silk_touch() {
    // Without silk touch, grass block should drop dirt
    init_test_registries();
    let mut rng = test_rng();

    let mut ctx = LootContext::new(&mut rng);
    let items = vanilla_loot_tables::BLOCKS_GRASS_BLOCK.get_random_items(&mut ctx);

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].count, 1);
    // Without silk touch, grass block drops dirt
    assert_eq!(items[0].item.key, Identifier::vanilla_static("dirt"));
}

#[test]
fn test_stone_loot_no_silk_touch() {
    // Without silk touch, stone should drop cobblestone
    init_test_registries();
    let mut rng = test_rng();

    let mut ctx = LootContext::new(&mut rng);
    let items = vanilla_loot_tables::BLOCKS_STONE.get_random_items(&mut ctx);

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].count, 1);
    // Without silk touch, stone drops cobblestone
    assert_eq!(items[0].item.key, Identifier::vanilla_static("cobblestone"));
}

#[test]
fn test_pig_loot_drops_raw_porkchop_when_not_on_fire() {
    init_test_registries();
    let mut rng = test_rng();
    let pig_key = Identifier::vanilla_static("pig");
    let pig = EntityRef {
        entity_type: Some(&pig_key),
        flags: EntityRefFlags::default(),
        equipment: None,
        custom_name: None,
    };

    let mut ctx = LootContext::new(&mut rng).with_this_entity(pig);
    let items = vanilla_loot_tables::ENTITIES_PIG.get_random_items(&mut ctx);

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item.key, Identifier::vanilla_static("porkchop"));
    assert!((1..=3).contains(&items[0].count));
}

#[test]
fn test_pig_loot_smelt_condition_uses_entity_fire_flag() {
    init_test_registries();
    let mut rng = test_rng();
    let pig_key = Identifier::vanilla_static("pig");
    let pig = EntityRef {
        entity_type: Some(&pig_key),
        flags: EntityRefFlags {
            is_on_fire: true,
            ..EntityRefFlags::default()
        },
        equipment: None,
        custom_name: None,
    };

    let mut ctx = LootContext::new(&mut rng).with_this_entity(pig);
    let items = vanilla_loot_tables::ENTITIES_PIG.get_random_items(&mut ctx);

    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].item.key,
        Identifier::vanilla_static("cooked_porkchop")
    );
    assert!((1..=3).contains(&items[0].count));
}

#[test]
fn test_uniform_get_int_reaches_inclusive_max() {
    // Vanilla UniformGenerator.getInt uses Mth.nextInt(rand, min, max), which
    // samples the integer range inclusively; a uniform 1..3 count must yield 3.
    let provider = NumberProvider::Uniform { min: 1.0, max: 3.0 };
    let mut seen = [false; 4];
    for seed in 0u64..1000 {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let value = provider.get_int(&mut rng);
        seen[value as usize] = true;
    }
    assert!(
        seen[1] && seen[2] && seen[3],
        "uniform 1..=3 must produce 1, 2 and 3, saw {seen:?}"
    );
}

#[test]
fn test_explosion_decay_function() {
    // Test the explosion_decay function directly
    init_test_registries();

    // explosion_decay reduces count based on 1/radius probability per item
    let cond_func = ConditionalLootFunction {
        function: LootFunction::ExplosionDecay,
        conditions: &[],
    };

    let mut total_survived = 0;
    let initial_count = 10;

    for seed in 0u64..100 {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let mut ctx = LootContext::new(&mut rng).with_explosion(4.0);
        let mut item = ItemStack::with_count(&crate::vanilla_items::STONE, initial_count);
        cond_func.function.apply(&mut item, &mut ctx);
        total_survived += item.count;
    }

    // With 10 items each trial, 100 trials = 1000 items total
    // Each has 25% (1/4.0) chance to survive = ~250 expected
    // Allow for variance: 150-350 range
    assert!(
        total_survived > 150 && total_survived < 350,
        "Expected ~250 items with explosion decay (25% of 1000), got {total_survived}"
    );
}

#[test]
fn ominous_bottle_amplifier_function_clamps_to_persistent_range() {
    use crate::data_components::vanilla_components::OMINOUS_BOTTLE_AMPLIFIER;

    init_test_registries();
    for (provided, expected) in [(-3.0, 0), (2.0, 2), (9.0, 4)] {
        let mut rng = test_rng();
        let mut context = LootContext::new(&mut rng);
        let mut item = ItemStack::new(&crate::vanilla_items::OMINOUS_BOTTLE);
        LootFunction::SetOminousBottleAmplifier {
            amplifier: NumberProvider::Constant(provided),
        }
        .apply(&mut item, &mut context);

        assert_eq!(
            item.get(OMINOUS_BOTTLE_AMPLIFIER)
                .map(|amplifier| amplifier.value()),
            Some(expected)
        );
    }
}

#[test]
fn test_survives_explosion_condition() {
    init_test_registries();

    // Test that survives_explosion condition works
    // Gravel has survives_explosion on its alternatives
    let mut survived = 0;
    for seed in 0..100 {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let mut ctx = LootContext::new(&mut rng).with_explosion(4.0);
        let items = vanilla_loot_tables::BLOCKS_GRAVEL.get_random_items(&mut ctx);
        if !items.is_empty() {
            survived += 1;
        }
    }

    // With radius 4.0, ~25% should survive
    assert!(
        survived > 10 && survived < 50,
        "Expected ~25% survival rate, got {survived}%"
    );
}
