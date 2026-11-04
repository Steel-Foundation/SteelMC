use std::sync::Arc;

use glam::DVec3;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, ChestType, DoubleBlockHalf};
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_items};
use steel_utils::types::{InteractionHand, UpdateFlags};
use steel_utils::{BlockPos, BlockStateId, ChunkPos, Direction, Downcast as _};

use crate::behavior::{
    BlockHitResult, ITEM_BEHAVIORS, InteractionResult, UseOnContext, init_behaviors,
};
use crate::entity::{Entity as _, entities::ItemEntity};
use crate::inventory::container::Container as _;
use crate::player::Player;
use crate::test_support::{
    TestPlayerBuilder, TestWorld, fresh_test_world, insert_ready_full_chunk,
};

struct Interaction {
    world_fixture: TestWorld,
    player: Arc<Player>,
    pos: BlockPos,
}

impl Interaction {
    fn new(name: &'static str) -> Self {
        init_vanilla_registry();
        init_behaviors();
        let world_fixture = fresh_test_world(name);
        let world = &world_fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        let pos = BlockPos::new(8, 64, 8);
        let player = TestPlayerBuilder::new(Arc::clone(world), "Transformer", 1).build();
        Self {
            world_fixture,
            player,
            pos,
        }
    }

    fn block(&self, pos: BlockPos, state: BlockStateId) {
        self.world_fixture
            .world
            .set_block(pos, state, UpdateFlags::UPDATE_KNOWN_SHAPE);
    }

    fn tool(&self, item: ItemRef) {
        self.player
            .inventory
            .lock()
            .set_item(0, ItemStack::new(item));
    }

    fn use_on(&self, face: Direction, hand: InteractionHand) -> InteractionResult {
        let item = self.player.inventory.lock().get_item_in_hand(hand).item();
        let mut context = UseOnContext::new(
            &self.player,
            hand,
            BlockHitResult {
                block_pos: self.pos,
                direction: face,
                location: DVec3::new(8.5, 64.5, 8.5),
                miss: false,
                inside: false,
                world_border_hit: false,
            },
            &self.world_fixture.world,
            Arc::clone(&self.player.inventory),
        );
        ITEM_BEHAVIORS.get_behavior(item).use_on(&mut context)
    }
}

#[test]
fn rooted_dirt_drops_loot_from_each_clicked_face_even_with_a_block_above() {
    let interaction = Interaction::new("transformer_rooted_dirt");
    interaction.block(
        interaction.pos.above(),
        vanilla_blocks::STONE.default_state(),
    );
    for face in Direction::ALL {
        interaction.block(interaction.pos, vanilla_blocks::ROOTED_DIRT.default_state());
        interaction.tool(&vanilla_items::WOODEN_HOE);
        let before = interaction
            .world_fixture
            .world
            .entity_manager()
            .get_accessible_entities();
        assert_eq!(
            interaction.use_on(face, InteractionHand::MainHand),
            InteractionResult::Success
        );
        assert_eq!(
            interaction
                .world_fixture
                .world
                .get_block_state(interaction.pos),
            vanilla_blocks::DIRT.default_state()
        );
        let entities = interaction
            .world_fixture
            .world
            .entity_manager()
            .get_accessible_entities();
        let dropped: Vec<_> = entities
            .iter()
            .filter(|entity| !before.iter().any(|old| old.id() == entity.id()))
            .collect();
        assert_eq!(dropped.len(), 1);
        let drop = dropped[0].downcast_ref::<ItemEntity>().expect("item drop");
        assert!(drop.get_item().is(&vanilla_items::HANGING_ROOTS));
        assert_eq!(drop.get_item().count(), 1);
        let position = drop.position();
        match face {
            Direction::Down => assert!(position.y < f64::from(interaction.pos.y())),
            Direction::Up => assert!(position.y >= f64::from(interaction.pos.y() + 1)),
            Direction::North => assert!(position.z < f64::from(interaction.pos.z())),
            Direction::South => assert!(position.z > f64::from(interaction.pos.z() + 1)),
            Direction::West => assert!(position.x < f64::from(interaction.pos.x())),
            Direction::East => assert!(position.x > f64::from(interaction.pos.x() + 1)),
        }
    }
}

#[test]
fn copper_door_transforms_both_halves_without_restoring_the_old_neighbor_shape() {
    let interaction = Interaction::new("transformer_copper_door");
    interaction.block(
        interaction.pos.below(),
        vanilla_blocks::STONE.default_state(),
    );
    interaction.tool(&vanilla_items::WOODEN_AXE);
    for clicked_half in [DoubleBlockHalf::Lower, DoubleBlockHalf::Upper] {
        let lower = vanilla_blocks::WEATHERED_COPPER_DOOR
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, Direction::West)
            .set_value(&BlockStateProperties::OPEN, true);
        let upper = lower.set_value(
            &BlockStateProperties::DOUBLE_BLOCK_HALF,
            DoubleBlockHalf::Upper,
        );
        let lower_pos = if clicked_half == DoubleBlockHalf::Upper {
            interaction.pos.below()
        } else {
            interaction.pos
        };
        interaction.block(lower_pos.below(), vanilla_blocks::STONE.default_state());
        interaction.block(lower_pos, lower);
        interaction.block(lower_pos.above(), upper);
        assert_eq!(
            interaction.use_on(Direction::North, InteractionHand::MainHand),
            InteractionResult::Success
        );
        for (pos, half) in [
            (lower_pos, DoubleBlockHalf::Lower),
            (lower_pos.above(), DoubleBlockHalf::Upper),
        ] {
            let state = interaction.world_fixture.world.get_block_state(pos);
            assert_eq!(state.get_block(), &vanilla_blocks::EXPOSED_COPPER_DOOR);
            assert_eq!(
                state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF),
                half
            );
            assert_eq!(
                state.get_value(&BlockStateProperties::HORIZONTAL_FACING),
                Direction::West
            );
            assert!(state.get_value(&BlockStateProperties::OPEN));
        }
    }
}

#[test]
fn copper_chest_transforms_both_halves_and_preserves_independent_properties() {
    let interaction = Interaction::new("transformer_copper_chest");
    for (facing, clicked_is_left) in Direction::HORIZONTAL
        .into_iter()
        .flat_map(|facing| [false, true].map(|left| (facing, left)))
    {
        let connected_direction = if clicked_is_left {
            facing.rotate_y_clockwise()
        } else {
            facing.rotate_y_counter_clockwise()
        };
        let neighbor_pos = interaction.pos.relative(connected_direction);
        for (source, target, tool) in [
            (
                &vanilla_blocks::WEATHERED_COPPER_CHEST,
                &vanilla_blocks::EXPOSED_COPPER_CHEST,
                &vanilla_items::WOODEN_AXE,
            ),
            (
                &vanilla_blocks::WAXED_WEATHERED_COPPER_CHEST,
                &vanilla_blocks::WEATHERED_COPPER_CHEST,
                &vanilla_items::WOODEN_AXE,
            ),
            (
                &vanilla_blocks::WEATHERED_COPPER_CHEST,
                &vanilla_blocks::WAXED_WEATHERED_COPPER_CHEST,
                &vanilla_items::HONEYCOMB,
            ),
        ] {
            for direction in Direction::HORIZONTAL {
                interaction.block(
                    interaction.pos.relative(direction),
                    vanilla_blocks::AIR.default_state(),
                );
            }
            let clicked = source
                .default_state()
                .set_value(&BlockStateProperties::FACING, facing)
                .set_value(
                    &BlockStateProperties::CHEST_TYPE,
                    if clicked_is_left {
                        ChestType::Left
                    } else {
                        ChestType::Right
                    },
                )
                .set_value(&BlockStateProperties::WATERLOGGED, true);
            let neighbor = source
                .default_state()
                .set_value(&BlockStateProperties::FACING, facing)
                .set_value(
                    &BlockStateProperties::CHEST_TYPE,
                    if clicked_is_left {
                        ChestType::Right
                    } else {
                        ChestType::Left
                    },
                )
                .set_value(&BlockStateProperties::WATERLOGGED, false);
            interaction.block(interaction.pos, clicked);
            interaction.block(neighbor_pos, neighbor);
            interaction.tool(tool);
            assert_eq!(
                interaction.use_on(Direction::Up, InteractionHand::MainHand),
                InteractionResult::Success
            );
            let expected_clicked = target.default_state().with_properties_of(clicked);
            let expected_neighbor = target.default_state().with_properties_of(neighbor);
            assert_eq!(
                interaction
                    .world_fixture
                    .world
                    .get_block_state(interaction.pos),
                expected_clicked
            );
            assert_eq!(
                interaction
                    .world_fixture
                    .world
                    .get_block_state(neighbor_pos),
                expected_neighbor
            );
        }
    }
}
