//! Grindstone block behavior.
//!
//! Opens the grindstone menu when right-clicked. The menu repairs a tool by
//! combining it with a second copy or strips every non-curse enchantment off an
//! item, returning the lost levels as experience. Mirrors vanilla
//! `GrindstoneBlock`.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{
    AttachFace, BlockStateProperties, Direction, EnumProperty,
};
use steel_registry::vanilla_custom_stats;
use steel_utils::axis::Axis;
use steel_utils::{BlockPos, BlockStateId, translations};
use text_components::TextComponent;

use crate::behavior::InventoryAccess;
use crate::behavior::block::BlockBehavior;
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::entity::ai::path::PathComputationType;
use crate::inventory::menu::kinds::grindstone;
use crate::player::Player;
use crate::world::World;

const ATTACH_FACE: &EnumProperty<AttachFace> = &BlockStateProperties::ATTACH_FACE;
const HORIZONTAL_FACING: &EnumProperty<Direction> = &BlockStateProperties::HORIZONTAL_FACING;

/// Behavior for the grindstone block.
#[block_behavior]
pub struct GrindstoneBlock {
    block: BlockRef,
}

impl GrindstoneBlock {
    /// Creates a new grindstone block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for GrindstoneBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // Vanilla `GrindstoneBlock` overrides `canSurvive` to always return true,
        // so `FaceAttachedHorizontalDirectionalBlock`'s placement loop stops on
        // its first candidate: the direction facing away from the clicked face.
        // Clicking the top of a block therefore stands the grindstone upright on
        // it, and a grindstone never needs a supporting block or pops off.
        let direction = context.get_nearest_looking_directions()[0];
        let state = if direction.get_axis() == Axis::Y {
            self.block
                .default_state()
                .set_value(
                    ATTACH_FACE,
                    if direction == Direction::Up {
                        AttachFace::Ceiling
                    } else {
                        AttachFace::Floor
                    },
                )
                .set_value(HORIZONTAL_FACING, context.horizontal_direction())
        } else {
            self.block
                .default_state()
                .set_value(ATTACH_FACE, AttachFace::Wall)
                .set_value(HORIZONTAL_FACING, direction.opposite())
        };

        Some(state)
    }

    fn use_without_item(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let inventory = player.inventory.clone();
        player.open_menu(
            TextComponent::translated(translations::CONTAINER_GRINDSTONE_TITLE.msg()),
            move |context| grindstone(inventory, context.container_id, pos, context.world),
        );
        player.award_custom_stat(&vanilla_custom_stats::INTERACT_WITH_GRINDSTONE);
        InteractionResult::Success
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_registry::{
        init_vanilla_registry, item_stack::ItemStack, vanilla_blocks, vanilla_items,
    };
    use steel_utils::{ChunkPos, types::InteractionHand, types::UpdateFlags};

    use super::*;
    use crate::behavior::{PlacementOrientation, PlacementSource, init_behaviors};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    /// Places a grindstone against a solid block by clicking `clicked_face`,
    /// while the player looks horizontally (pitch 0). Vanilla prioritizes the
    /// clicked face over the look direction, so the result must not depend on
    /// the near-horizontal look.
    fn placement_state(clicked_face: Direction, key: &'static str) -> BlockStateId {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world(key);
        let hit_pos = BlockPos::new(0, 64, 0);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(hit_pos));
        assert!(world.set_block(
            hit_pos,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));

        let mut item = ItemStack::new(&vanilla_items::GRINDSTONE);
        let hit_result = BlockHitResult {
            location: DVec3::ZERO,
            direction: clicked_face,
            block_pos: hit_pos,
            miss: false,
            inside: false,
            world_border_hit: false,
        };
        let source = PlacementSource::direct(
            None,
            InteractionHand::MainHand,
            &mut item,
            PlacementOrientation::Player {
                rotation: 0.0,
                pitch: 0.0,
            },
            false,
        );
        let context = BlockPlaceContext::new(&world, source, &hit_result);
        GrindstoneBlock::new(&vanilla_blocks::GRINDSTONE)
            .get_state_for_placement(&context)
            .expect("grindstone always has a placement state")
    }

    #[test]
    fn clicking_the_top_of_a_block_stands_the_grindstone_upright() {
        let state = placement_state(Direction::Up, "grindstone_place_floor");
        assert_eq!(state.get_value(ATTACH_FACE), AttachFace::Floor);
    }

    #[test]
    fn clicking_the_underside_hangs_the_grindstone_from_the_ceiling() {
        let state = placement_state(Direction::Down, "grindstone_place_ceiling");
        assert_eq!(state.get_value(ATTACH_FACE), AttachFace::Ceiling);
    }

    #[test]
    fn clicking_a_side_mounts_the_grindstone_on_the_wall_facing_the_player() {
        let state = placement_state(Direction::North, "grindstone_place_wall");
        assert_eq!(state.get_value(ATTACH_FACE), AttachFace::Wall);
        assert_eq!(state.get_value(HORIZONTAL_FACING), Direction::North);
    }
}
