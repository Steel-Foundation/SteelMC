//! Hanging sign item behavior implementation.
//! Wall and standing signs are handled by `StandingAndWallBlockItem`, not here.

use std::sync::Arc;
use steel_macros::item_behavior;
use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::blocks::shapes::SupportType;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::context::{InteractionResult, UseOnContext};
use crate::behavior::items::block_item::SurvivalCheck;
use crate::behavior::{BlockItem, ItemBehavior, StandingAndWallBlockItem};
use crate::world::{LevelReader as _, World};

/// Behavior for hanging sign items that place hanging sign blocks.
///
/// Hanging signs can be placed as ceiling hanging signs or wall hanging signs.
#[item_behavior]
pub struct HangingSignItem {
    /// The ceiling hanging sign block.
    #[json_arg(vanilla_blocks, json = "block")]
    pub ceiling_block: BlockRef,
    /// The wall hanging sign block.
    #[json_arg(vanilla_blocks, json = "wall_block")]
    pub wall_block: BlockRef,

    base: StandingAndWallBlockItem,
}

impl HangingSignItem {
    /// Creates a new hanging sign item behavior.
    #[must_use]
    pub const fn new(ceiling_block: BlockRef, wall_block: BlockRef) -> Self {
        Self {
            ceiling_block,
            wall_block,
            base: StandingAndWallBlockItem::new(ceiling_block, wall_block, Direction::Up),
        }
    }
}

/// Checks if a wall hanging sign can attach to a neighboring block.
///
/// This matches vanilla's `WallHangingSignBlock.canAttachTo`.
fn can_attach_to(
    world: &Arc<World>,
    sign_facing: Direction,
    attach_pos: BlockPos,
    attach_face: Direction,
) -> bool {
    let attach_state = world.get_block_state(attach_pos);
    let attach_block = REGISTRY.blocks.by_state_id(attach_state);

    if let Some(block) = attach_block
        && block.has_tag(&BlockTag::WALL_HANGING_SIGNS)
    {
        // Wall hanging signs can chain if they're on the same axis
        if let Some(neighbor_facing) =
            attach_state.try_get_value(&BlockStateProperties::HORIZONTAL_FACING)
        {
            return neighbor_facing.axis() == sign_facing.axis();
        }
    }

    // Otherwise, check for sturdy face with FULL support
    world.is_face_sturdy_for(attach_state, attach_pos, attach_face, SupportType::Full)
}

/// Checks if a wall hanging sign can be placed at the given position.
///
/// This matches vanilla's `WallHangingSignBlock.canPlace` which is called
/// from `HangingSignItem.canPlace` in addition to `canSurvive`.
fn can_wall_hanging_sign_place(world: &Arc<World>, state: BlockStateId, pos: BlockPos) -> bool {
    let Some(facing) = state.try_get_value(&BlockStateProperties::HORIZONTAL_FACING) else {
        return false;
    };

    let clockwise = facing.rotate_y_clockwise();
    let counter_clockwise = facing.rotate_y_counter_clockwise();

    let can_attach_clockwise = {
        let attach_pos = clockwise.relative(pos);
        can_attach_to(world, facing, attach_pos, counter_clockwise)
    };

    let can_attach_counter = {
        let attach_pos = counter_clockwise.relative(pos);
        can_attach_to(world, facing, attach_pos, clockwise)
    };

    can_attach_clockwise || can_attach_counter
}

/// Checks if a wall hanging sign block state can be placed.
///
/// This matches vanilla's `HangingSignItem.canPlace` override which adds
/// an additional check for `WallHangingSignBlock.canPlace`.
///
/// This assumes that `can_survive` has already returned `true`.
fn can_place_hanging_sign(world: &Arc<World>, state: BlockStateId, pos: BlockPos) -> bool {
    let block = REGISTRY.blocks.by_state_id(state);

    // If it's a wall hanging sign, we need the additional canPlace check
    if let Some(block) = block
        && block.has_tag(&BlockTag::WALL_HANGING_SIGNS)
        && !can_wall_hanging_sign_place(world, state, pos)
    {
        return false;
    }

    // Defer to super function
    StandingAndWallBlockItem::can_place(world, state, pos)
}

impl ItemBehavior for HangingSignItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        self.base.base.place_with_policy(
            context.build_place_context(),
            Some,
            SurvivalCheck::Required,
            BlockItem::place_block,
            self.ceiling_block.config.sound_type.place_sound,
            |context| {
                self.base
                    .get_placement_state(context, can_place_hanging_sign)
            },
        )
    }
}
