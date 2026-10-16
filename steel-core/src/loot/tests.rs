// TODO: Add seeded Java/Rust differential coverage for vanilla tables, comparing
// item components, counts, order, placement, and remaining RNG state.

use super::{vanilla::fixtures, *};
use crate::inventory::container::{Container, SimpleContainer};
use steel_registry::vanilla_enchantments;
use steel_registry::{
    REGISTRY, TaggedRegistryExt, init_vanilla_registry, item_stack::ItemStack, vanilla_items,
};
use steel_utils::{
    Identifier,
    random::{Random, RandomSource, legacy_random::LegacyRandom},
};

fn random(seed: u64) -> RandomSource {
    RandomSource::Legacy(LegacyRandom::from_seed(seed))
}

fn tables() -> LootTableBehaviorRegistry {
    init_vanilla_registry();
    let mut registry = LootTableBehaviorRegistry::new();
    fixtures::register(&mut registry);
    registry
}

fn generate(table: &LootTable, seed: u64) -> Vec<ItemStack> {
    let tables = tables();
    let mut rng = random(seed);
    let mut ctx = LootContext::new(&mut rng);
    ctx.tables = Some(&tables);
    table
        .get_random_items(&mut ctx)
        .expect("table should generate")
}

#[test]
fn empty_alternative_succeeds_without_falling_through() {
    assert_eq!(generate(&fixtures::EMPTY_ALTERNATIVE, 12345), []);
}

#[test]
fn group_expands_children_before_weighted_selection() {
    let tables = tables();
    for seed in 0..100 {
        let mut expected = random(seed);
        let choice = expected.next_i32_bounded(10);
        let expected_item = match choice {
            0 => &vanilla_items::DIAMOND,
            1 => &vanilla_items::IRON_INGOT,
            _ => &vanilla_items::DIRT,
        };
        let mut rng = random(seed);
        let mut ctx = LootContext::new(&mut rng);
        ctx.tables = Some(&tables);
        let items = fixtures::GROUP_WEIGHT
            .get_random_items(&mut ctx)
            .expect("group loot");
        assert_eq!(items.len(), 1);
        assert!(items[0].is(expected_item));
        assert_eq!(rng.next_i64(), expected.next_i64());
    }
}

#[test]
fn sequence_keeps_prior_candidates_and_stops_after_failed_expansion() {
    let items = generate(&fixtures::SEQUENCE, 12345);
    assert_eq!(items.len(), 1);
    assert!(items[0].is(&vanilla_items::DIAMOND));
    let items = generate(&fixtures::GROUP_SINGLE_FAILURE, 12345);
    assert_eq!(items.len(), 1);
    assert!(items[0].is(&vanilla_items::DIRT));
}

#[test]
fn modifiers_run_per_stack_in_entry_pool_table_order() {
    let tables = tables();
    let mut expected = random(12345);
    let expected_counts: Vec<_> = (0..2)
        .map(|_| {
            expected.next_i32_between(2, 5)
                + expected.next_i32_between(1, 9)
                + expected.next_i32_between(1, 9)
        })
        .collect();
    let mut rng = random(12345);
    let mut ctx = LootContext::new(&mut rng);
    ctx.tables = Some(&tables);
    let items = fixtures::MODIFIER_ORDER
        .get_random_items(&mut ctx)
        .expect("modified loot");
    assert_eq!(
        items.iter().map(ItemStack::count).collect::<Vec<_>>(),
        expected_counts
    );
    assert_eq!(rng.next_i64(), expected.next_i64());
    assert_eq!(generate(&fixtures::COMPOSITE_MODIFIER, 12345)[0].count(), 5);
}

#[test]
fn partial_composites_keep_modifier_scopes_and_continue_with_siblings() {
    let tables = tables();
    for seed in 0..100 {
        let mut expected = random(seed);
        expected.next_f32();
        let (item, count) = match expected.next_i32_bounded(4) {
            0 => (&vanilla_items::DIAMOND, 2),
            1 => (&vanilla_items::IRON_INGOT, 15),
            2 => (&vanilla_items::DIRT, 14),
            _ => (&vanilla_items::COPPER_INGOT, 13),
        };
        let mut rng = random(seed);
        let mut ctx = LootContext::new(&mut rng);
        ctx.tables = Some(&tables);
        let items = fixtures::PARTIAL_COMPOSITE_MODIFIERS
            .get_random_items(&mut ctx)
            .expect("partially expanded loot");
        assert_eq!(items.len(), 1);
        assert!(items[0].is(item));
        assert_eq!(items[0].count(), count);
        assert_eq!(rng.next_i64(), expected.next_i64());
    }
}

#[test]
fn expanded_tag_items_each_receive_their_own_weight() {
    let tables = tables();
    let members = REGISTRY
        .items
        .get_tag(&Identifier::vanilla_static("creeper_drop_music_discs"))
        .expect("disc tag");
    for seed in 0..100 {
        let mut expected = random(seed);
        let choice = expected.next_i32_bounded(members.len() as i32 * 2 + 1) as usize;
        let item = members
            .get(choice / 2)
            .copied()
            .unwrap_or(&vanilla_items::DIAMOND);
        let mut rng = random(seed);
        let mut ctx = LootContext::new(&mut rng);
        ctx.tables = Some(&tables);
        let result = fixtures::TAG_WEIGHT
            .get_random_items(&mut ctx)
            .expect("tag loot");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].item, item);
        assert_eq!(rng.next_i64(), expected.next_i64());
    }
}

#[test]
fn nested_raw_output_is_modified_before_stack_normalization() {
    let items = generate(&fixtures::RAW_NESTED, 12345);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].count(), 10);
    let items = generate(&fixtures::OVERSIZED, 12345);
    assert_eq!(
        items.iter().map(ItemStack::count).collect::<Vec<_>>(),
        [64, 64, 2]
    );
}

#[test]
fn unsupported_operations_are_errors_only_when_executed() {
    let tables = tables();
    let mut rng = random(12345);
    let mut ctx = LootContext::new(&mut rng);
    ctx.tables = Some(&tables);
    let error = fixtures::UNSUPPORTED_AFTER_OUTPUT
        .get_random_items(&mut ctx)
        .expect_err("unsupported function");
    assert!(
        matches!(error,LootError::Unsupported{table:Some(key),operation:"minecraft:enchant_randomly"} if key == fixtures::UNSUPPORTED_AFTER_OUTPUT.key)
    );
    assert_eq!(ctx.visited, []);
    assert_eq!(generate(&fixtures::CONDITIONAL_UNSUPPORTED, 12345).len(), 1);
}

#[test]
fn failed_generation_does_not_change_container_contents() {
    let tables = tables();
    let mut rng = random(12345);
    let mut ctx = LootContext::new(&mut rng);
    ctx.tables = Some(&tables);
    let mut container = SimpleContainer::new(3);
    container.set_item(1, ItemStack::new(&vanilla_items::STONE));
    let result = fixtures::UNSUPPORTED_AFTER_OUTPUT.generate_for_container(&mut ctx);
    assert!(result.is_err());
    if let Ok(loot) = result {
        loot.fill(&mut container);
    }
    assert!(container.get_item(0).is_empty());
    assert!(container.get_item(1).is(&vanilla_items::STONE));
    assert!(container.get_item(2).is_empty());
}

#[test]
fn vanilla_block_tables_keep_supported_behavior() {
    init_vanilla_registry();
    let mut rng = random(12345);
    let mut ctx = LootContext::new(&mut rng);
    let items = vanilla_loot_tables::BLOCKS_OAK_LOG
        .get_random_items(&mut ctx)
        .expect("oak log loot");
    assert_eq!(items.len(), 1);
    assert!(items[0].is(&vanilla_items::OAK_LOG));
    let items = vanilla_loot_tables::BLOCKS_DIAMOND_ORE
        .get_random_items(&mut ctx)
        .expect("diamond loot");
    assert_eq!(items.len(), 1);
    assert!(items[0].is(&vanilla_items::DIAMOND));
}

#[test]
fn damage_source_condition_requires_origin_and_damage_source() {
    use steel_registry::vanilla_damage_types;

    init_vanilla_registry();
    for has_origin in [false, true] {
        for has_damage_source in [false, true] {
            let mut rng = random(12345);
            let mut ctx = LootContext::new(&mut rng);
            if has_origin {
                ctx = ctx.with_origin(0.0, 0.0, 0.0);
            }
            if has_damage_source {
                ctx = ctx.with_damage_source(DamageSourceInfo {
                    damage_type: Some(&vanilla_damage_types::LIGHTNING_BOLT.key),
                    tags: &[],
                    is_direct: true,
                });
            }
            let items = vanilla_loot_tables::ENTITIES_TURTLE
                .get_random_items(&mut ctx)
                .expect("turtle loot");
            assert_eq!(
                items.iter().any(|item| item.is(&vanilla_items::BOWL)),
                has_origin && has_damage_source,
                "origin={has_origin}, damage_source={has_damage_source}"
            );
        }
    }
}

#[test]
fn recursion_skips_the_active_table_and_restores_visitation() {
    static RECURSIVE: LootTable = LootTable {
        key: Identifier::new_static("steel", "test/recursive"),
        param_set: Identifier::vanilla_static("generic"),
        random_sequence: None,
    };
    fn recursive(ctx: &mut LootContext<'_>, output: &mut LootOutput<'_>) -> LootResult<()> {
        RECURSIVE.emit_raw(ctx, output)?;
        output(ctx, ItemStack::new(&vanilla_items::DIAMOND))
    }
    let mut tables = tables();
    tables.register(&RECURSIVE, recursive);
    let mut rng = random(12345);
    let mut ctx = LootContext::new(&mut rng);
    ctx.tables = Some(&tables);
    for _ in 0..2 {
        assert_eq!(
            RECURSIVE
                .get_random_items(&mut ctx)
                .expect("recursive loot")
                .len(),
            1
        );
        assert_eq!(ctx.visited, []);
    }
}

#[test]
fn container_fill_matches_java_split_shuffle_and_random_state() {
    let tables = tables();
    let mut rng = random(12345);
    let mut ctx = LootContext::new(&mut rng);
    ctx.tables = Some(&tables);
    let loot = fixtures::OVERSIZED
        .generate_for_container(&mut ctx)
        .expect("container loot");
    let mut container = SimpleContainer::new(6);
    container.set_item(1, ItemStack::new(&vanilla_items::STONE));
    loot.fill(&mut container);
    assert!(container.get_item(1).is(&vanilla_items::STONE));
    // Reference: local 26.3 splitting/shuffling algorithm with java.util.Random(12345).
    assert_eq!(
        container
            .items()
            .iter()
            .map(ItemStack::count)
            .collect::<Vec<_>>(),
        [62, 1, 2, 2, 62, 2]
    );
    assert_eq!(rng.next_i64(), 2_919_502_189_498_201_214);
}

#[test]
fn container_fill_handles_overflow_zero_capacity_and_occupied_slots() {
    let tables = tables();
    for (size, occupied, counts) in [
        (1, None, vec![64]),
        (0, None, vec![]),
        (2, Some(0), vec![1, 64]),
    ] {
        let mut rng = random(12345);
        let mut ctx = LootContext::new(&mut rng);
        ctx.tables = Some(&tables);
        let loot = fixtures::OVERSIZED
            .generate_for_container(&mut ctx)
            .expect("container loot");
        let mut container = SimpleContainer::new(size);
        if let Some(slot) = occupied {
            container.set_item(slot, ItemStack::new(&vanilla_items::STONE));
        }
        loot.fill(&mut container);
        assert_eq!(
            container
                .items()
                .iter()
                .map(ItemStack::count)
                .collect::<Vec<_>>(),
            counts
        );
        if let Some(slot) = occupied {
            assert!(container.get_item(slot).is(&vanilla_items::STONE));
        }
        assert_eq!(rng.next_i64(), -1_236_052_134_575_208_584);
    }
}

#[test]
fn instrument_selection_and_signed_book_enchantment_modifiers() {
    use steel_registry::data_components::vanilla_components::INSTRUMENT;
    use steel_registry::vanilla_instrument_tags::InstrumentTag;
    let items = generate(&fixtures::INSTRUMENT, 12345);
    let selected = items[0]
        .get(INSTRUMENT)
        .and_then(|component| component.instrument().as_reference())
        .expect("selected horn instrument");
    assert!(
        REGISTRY
            .instruments
            .is_in_tag(selected, &InstrumentTag::REGULAR_GOAT_HORNS)
    );
    let items = generate(&fixtures::BOOK_ENCHANTMENT, 12345);
    assert!(items[0].is(&vanilla_items::ENCHANTED_BOOK));
    assert_eq!(
        items[0]
            .get_enchantments_for_crafting()
            .expect("stored enchantments")
            .get_level(&vanilla_enchantments::SHARPNESS.key),
        6
    );
}

#[test]
fn sheep_shearing_uses_color_and_sheared_predicates() {
    init_vanilla_registry();
    for sheared in [false, true] {
        let mut rng = random(12345);
        let entity = EntityRef {
            sheep_color: Some(steel_registry::DyeColor::Red),
            sheep_sheared: Some(sheared),
            ..EntityRef::default()
        };
        let mut ctx = LootContext::new(&mut rng).with_this_entity(entity);
        let items = vanilla_loot_tables::SHEARING_SHEEP
            .get_random_items(&mut ctx)
            .expect("sheep loot");
        if sheared {
            assert_eq!(items, []);
        } else {
            assert!((1..=3).contains(&items.len()));
            assert!(items.iter().all(|item| item.is(&vanilla_items::RED_WOOL)));
        }
    }
}

#[test]
fn looting_uses_attacker_equipment_and_limits_the_final_count() {
    use crate::entity::{LivingEntity, entities::PigEntity, entity_loot_ref};
    use glam::DVec3;
    use std::sync::Weak;
    use steel_registry::{equipment::EquipmentSlot, vanilla_entities};
    let tables = tables();
    let attacker = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let mut sword = ItemStack::new(&vanilla_items::DIAMOND_SWORD);
    sword.set_enchantments(&[(vanilla_enchantments::LOOTING.key.clone(), 4)], false);
    attacker
        .living_base()
        .equipment()
        .lock()
        .set(EquipmentSlot::MainHand, sword);
    let mut helmet = ItemStack::new(&vanilla_items::DIAMOND_HELMET);
    helmet.set_enchantments(&[(vanilla_enchantments::LOOTING.key.clone(), 9)], false);
    attacker
        .living_base()
        .equipment()
        .lock()
        .set(EquipmentSlot::Head, helmet);
    let mut tool = ItemStack::new(&vanilla_items::DIAMOND_SWORD);
    tool.set_enchantments(&[(vanilla_enchantments::LOOTING.key.clone(), 1)], false);
    for with_attacker in [false, true] {
        let mut rng = random(12345);
        let mut ctx = LootContext::new(&mut rng).with_tool(&tool);
        ctx.tables = Some(&tables);
        if with_attacker {
            ctx = ctx.with_killer_entity(entity_loot_ref(&attacker));
        }
        assert_eq!(
            ctx.attacker_enchantment_level(&vanilla_enchantments::LOOTING.key),
            if with_attacker { 4 } else { 0 }
        );
        let items = fixtures::ATTACKER_LOOTING
            .get_random_items(&mut ctx)
            .expect("looting output");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].count(), if with_attacker { 3 } else { 1 });
    }
}

#[test]
fn chance_provider_is_evaluated_before_the_condition_random_draw() {
    let tables = tables();
    for seed in 0..20 {
        let mut expected = random(seed);
        let probability = expected.next_f32();
        let passes = expected.next_f32() < probability;
        let mut rng = random(seed);
        let mut ctx = LootContext::new(&mut rng);
        ctx.tables = Some(&tables);
        let items = fixtures::PROVIDER_CHANCE
            .get_random_items(&mut ctx)
            .expect("chance output");
        assert_eq!(items.len(), usize::from(passes));
        assert_eq!(rng.next_i64(), expected.next_i64());
    }
}

#[test]
fn missing_enchantment_component_skips_provider_and_component_creation() {
    use steel_registry::data_components::vanilla_components::ENCHANTMENTS;
    fn without_enchantments(
        ctx: &mut LootContext<'_>,
        output: &mut LootOutput<'_>,
    ) -> LootResult<()> {
        let mut item = ItemStack::new(&vanilla_items::DIAMOND);
        item.remove(ENCHANTMENTS);
        output(ctx, item)
    }
    let mut tables = tables();
    tables.register(
        &vanilla_loot_tables::BLOCKS_DIAMOND_ORE,
        without_enchantments,
    );
    let mut rng = random(12345);
    let mut ctx = LootContext::new(&mut rng);
    ctx.tables = Some(&tables);
    let items = fixtures::ENCHANT_MISSING_COMPONENT
        .get_random_items(&mut ctx)
        .expect("unmodifiable item");
    assert_eq!(items.len(), 1);
    assert!(!items[0].has(ENCHANTMENTS));
    assert_eq!(rng.next_i64(), random(12345).next_i64());
}

#[test]
fn count_rounding_matches_java_at_half_boundaries_and_nonfinite_values() {
    for (value, expected) in [
        (f32::from_bits(0x3eff_ffff), 0),
        (0.5, 1),
        (-0.5, 0),
        (-1.5, -1),
        (f32::NAN, 0),
        (f32::INFINITY, i32::MAX),
        (f32::NEG_INFINITY, i32::MIN),
    ] {
        assert_eq!(modifiers::round_count(value), expected);
    }
}
