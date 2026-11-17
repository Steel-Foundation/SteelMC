use crate::test_support::TestLevel;

use super::*;

use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, SlabType};
use steel_registry::init_vanilla_registry;
use steel_registry::sound_events;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_fluids;
use steel_registry::vanilla_items;

use crate::behavior::init_behaviors;

#[test]
fn default_spawn_rule_requires_full_up_support_and_low_light_emission() {
    init_vanilla_registry();
    init_behaviors();
    let level = TestLevel::default();
    let pos = BlockPos::new(3, 64, 5);
    let entity_type = &vanilla_entities::ZOMBIE;

    let stone = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::STONE);
    assert!(stone.is_valid_spawn(
        vanilla_blocks::STONE.default_state(),
        &level,
        pos,
        entity_type
    ));

    let glowstone = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::GLOWSTONE);
    assert!(!glowstone.is_valid_spawn(
        vanilla_blocks::GLOWSTONE.default_state(),
        &level,
        pos,
        entity_type
    ));

    let torch = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::TORCH);
    assert!(!torch.is_valid_spawn(
        vanilla_blocks::TORCH.default_state(),
        &level,
        pos,
        entity_type
    ));
}

#[test]
fn clone_item_stack_uses_registered_block_item_association() {
    init_vanilla_registry();
    init_behaviors();

    for (block, expected_item) in [
        (&vanilla_blocks::REDSTONE_WIRE, &*vanilla_items::REDSTONE),
        (&vanilla_blocks::WALL_TORCH, &*vanilla_items::TORCH),
        (
            &vanilla_blocks::BIG_DRIPLEAF_STEM,
            &*vanilla_items::BIG_DRIPLEAF,
        ),
    ] {
        let clone_item = BLOCK_BEHAVIORS
            .get_behavior(block)
            .get_clone_item_stack(block, block.default_state(), None, false)
            .map(|stack| stack.item());

        assert_eq!(clone_item, Some(expected_item));
    }

    let block = &vanilla_blocks::FIRE;
    let clone_item = BLOCK_BEHAVIORS.get_behavior(block).get_clone_item_stack(
        block,
        block.default_state(),
        None,
        false,
    );

    assert!(clone_item.is_some_and(|stack| stack.is_empty()));
}

#[test]
fn drained_waterlogged_state_clears_waterlogged_property() {
    init_vanilla_registry();
    let state = vanilla_blocks::OAK_SLAB
        .default_state()
        .set_value(&BlockStateProperties::WATERLOGGED, true);

    let drained = drained_waterlogged_state(state);

    assert_eq!(
        drained,
        Some(state.set_value(&BlockStateProperties::WATERLOGGED, false))
    );
}

#[test]
fn drained_waterlogged_state_ignores_dry_or_non_waterloggable_blocks() {
    init_vanilla_registry();
    let dry_slab = vanilla_blocks::OAK_SLAB
        .default_state()
        .set_value(&BlockStateProperties::WATERLOGGED, false);
    let stone = vanilla_blocks::STONE.default_state();

    assert_eq!(drained_waterlogged_state(dry_slab), None);
    assert_eq!(drained_waterlogged_state(stone), None);
}

#[test]
fn waterlogged_barrier_pickup_requires_player_context() {
    init_vanilla_registry();
    let waterlogged_slab = vanilla_blocks::OAK_SLAB
        .default_state()
        .set_value(&BlockStateProperties::WATERLOGGED, true);
    let waterlogged_barrier = vanilla_blocks::BARRIER
        .default_state()
        .set_value(&BlockStateProperties::WATERLOGGED, true);

    assert!(can_pick_up_drained_waterlogged_state(
        waterlogged_slab,
        None
    ));
    assert!(!can_pick_up_drained_waterlogged_state(
        waterlogged_barrier,
        None
    ));
}

#[test]
fn default_fluid_replacement_does_not_use_waterloggable_property_alone() {
    init_vanilla_registry();
    let double_slab = vanilla_blocks::OAK_SLAB
        .default_state()
        .set_value(&BlockStateProperties::SLAB_TYPE, SlabType::Double)
        .set_value(&BlockStateProperties::WATERLOGGED, false);
    let behavior = DefaultBlockBehavior::new(&vanilla_blocks::OAK_SLAB);

    assert!(!behavior.can_be_replaced_by_fluid(double_slab, &vanilla_fluids::WATER));
}

#[test]
fn default_behavior_preserves_unported_simple_waterlogged_blocks() {
    init_vanilla_registry();
    let dry_ladder = vanilla_blocks::LADDER
        .default_state()
        .set_value(&BlockStateProperties::WATERLOGGED, false);
    let wet_ladder = dry_ladder.set_value(&BlockStateProperties::WATERLOGGED, true);
    let level = TestLevel::default();

    assert_eq!(
        wet_ladder.get_fluid_state(),
        FluidState::source(&vanilla_fluids::WATER)
    );
    let behavior = DefaultBlockBehavior::new(&vanilla_blocks::LADDER);
    assert!(behavior.is_liquid_container(dry_ladder));
    assert!(behavior.can_place_liquid(dry_ladder, &vanilla_fluids::WATER));
    assert!(behavior.place_liquid(
        &level,
        BlockPos::ZERO,
        dry_ladder,
        FluidState::source(&vanilla_fluids::WATER),
    ));
    assert_eq!(level.last_placed_state(), Some(wet_ladder));
    assert!(level.scheduled_water_tick());
}

#[test]
fn default_behavior_schedules_shape_ticks_only_for_waterlogged_states() {
    init_vanilla_registry();
    let behavior = DefaultBlockBehavior::new(&vanilla_blocks::OAK_SLAB);
    let level = TestLevel::default();
    let pos = BlockPos::new(3, 64, 5);
    let dry_state = vanilla_blocks::OAK_SLAB
        .default_state()
        .set_value(&BlockStateProperties::WATERLOGGED, false);
    let wet_state = dry_state.set_value(&BlockStateProperties::WATERLOGGED, true);

    assert_eq!(
        behavior.update_shape(
            dry_state,
            &level,
            pos,
            Direction::North,
            pos.north(),
            vanilla_blocks::STONE.default_state(),
        ),
        dry_state
    );
    assert!(level.scheduled_fluid_ticks.borrow().is_empty());

    assert_eq!(
        behavior.update_shape(
            wet_state,
            &level,
            pos,
            Direction::North,
            pos.north(),
            vanilla_blocks::STONE.default_state(),
        ),
        wet_state
    );
    assert_eq!(
        level
            .scheduled_fluid_ticks
            .borrow()
            .iter()
            .map(|tick| (tick.pos, tick.fluid, tick.delay))
            .collect::<Vec<_>>(),
        vec![(pos, &vanilla_fluids::WATER, 5)]
    );
}

#[test]
fn fall_on_facts_use_vanilla_width_squared_height_formula() {
    let facts = EntityFallOnFacts::new(
        &vanilla_entities::PLAYER,
        true,
        0.6,
        1.8,
        (
            &sound_events::ENTITY_PLAYER_SMALL_FALL,
            &sound_events::ENTITY_PLAYER_BIG_FALL,
        ),
    );

    assert!(facts.is_player());
    assert!(facts.is_living_entity);
    assert!((facts.bounding_box_width_squared_height() - 0.648).abs() < f64::EPSILON);
}

#[test]
fn world_aabb_bounds_contains_all_boxes() {
    let bounds = world_aabb_bounds(&[
        WorldAabb::new(1.0, 2.0, 3.0, 2.0, 3.0, 4.0),
        WorldAabb::new(-1.0, 4.0, 2.0, 0.0, 5.0, 6.0),
    ])
    .expect("non-empty boxes should have bounds");

    assert_eq!(bounds, WorldAabb::new(-1.0, 2.0, 2.0, 2.0, 5.0, 6.0));
}

#[test]
fn block_drops_and_harvesting_continue_their_named_sequences() {
    use crate::{
        behavior::{BlockLootContext, init_behaviors},
        loot::{LootContext, random_sequences::RandomSequences, vanilla_loot_tables},
        test_support::{fresh_test_world, insert_ready_full_chunk},
    };
    use steel_registry::{item_stack::ItemStack, vanilla_blocks};
    use steel_utils::random::Random;
    use steel_utils::{BlockPos, ChunkPos};

    steel_registry::init_vanilla_registry();
    init_behaviors();
    let fixture = fresh_test_world("block_loot_sequences");
    let world = &fixture.world;
    let expected = RandomSequences::new_for_test(world.level_data.read().seed());
    let pos = BlockPos::new(8, 64, 8);
    insert_ready_full_chunk(world, ChunkPos::from_block_pos(pos));
    let tool = ItemStack::empty();
    for (table, state, harvest) in [
        (
            &vanilla_loot_tables::BLOCKS_MELON,
            vanilla_blocks::MELON.default_state(),
            false,
        ),
        (
            &vanilla_loot_tables::HARVEST_SWEET_BERRY_BUSH,
            vanilla_blocks::SWEET_BERRY_BUSH.default_state(),
            true,
        ),
    ] {
        let sequence = table.random_sequence.as_ref().expect("named sequence");
        for _ in 0..3 {
            let expected_items = expected
                .with_random(sequence, |random| {
                    let mut context = LootContext::new(random)
                        .with_world(world)
                        .with_origin(pos.get_center())
                        .with_block_state(state)
                        .with_tool(&tool);
                    table.get_random_items(&mut context)
                })
                .expect("expected block loot");
            let items = if harvest {
                drop_from_block_interact_loot_table(table, world, pos, state, Some(&tool), None)
            } else {
                BlockLootContext::new(world, pos).get_drops(state)
            };
            assert_eq!(items, expected_items);
            let actual = world
                .with_loot_random(Some(table), 0, |random| Ok(random.next_i64()))
                .expect("actual draw");
            let expected = expected
                .with_random(sequence, |random| Ok(random.next_i64()))
                .expect("expected draw");
            assert_eq!(actual, expected);
        }
    }
}
