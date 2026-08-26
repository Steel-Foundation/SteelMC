use glam::DVec3;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, EnumProperty};
use steel_registry::item_stack::ItemStack;
use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_items};
use steel_utils::BlockPos;
use steel_utils::types::InteractionHand;

use crate::behavior::{
    BLOCK_BEHAVIORS, BlockHitResult, BlockPlaceContext, PlacementOrientation, PlacementSource,
    init_behaviors,
};
use crate::test_support::test_world;

const FACING: &EnumProperty<Direction> = &BlockStateProperties::FACING;

fn assert_faces_away_from_player_look(block: BlockRef) {
    let behavior = BLOCK_BEHAVIORS.get_behavior(block);

    for look_direction in Direction::ALL {
        let rotation = look_direction.to_yaw();
        let pitch = match look_direction {
            Direction::Down => 90.0,
            Direction::Up => -90.0,
            _ => 0.0,
        };
        let mut stack = ItemStack::new(&vanilla_items::STONE);
        let source = PlacementSource::direct(
            None,
            InteractionHand::MainHand,
            &mut stack,
            PlacementOrientation::Player { rotation, pitch },
            false,
        );
        let context = BlockPlaceContext::new(
            test_world(),
            source,
            &BlockHitResult {
                location: DVec3::ZERO,
                direction: Direction::East,
                block_pos: BlockPos::ZERO,
                miss: false,
                inside: false,
                world_border_hit: false,
            },
        );
        let state = behavior
            .get_state_for_placement(&context)
            .expect("dispenser-family placement should produce a state");

        assert_eq!(state.get_block(), block);
        assert_eq!(state.get_value(FACING), look_direction.opposite());
    }
}

#[test]
fn dispenser_and_dropper_face_away_from_player_look() {
    init_vanilla_registry();
    init_behaviors();

    assert_faces_away_from_player_look(&vanilla_blocks::DISPENSER);
    assert_faces_away_from_player_look(&vanilla_blocks::DROPPER);
}
