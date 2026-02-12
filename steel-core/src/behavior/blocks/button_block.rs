//! Button block behavior.
//!
//! Buttons are face-attached blocks that emit a redstone signal when pressed.
//! They automatically unpress after a delay via the scheduled tick system.
//!
//! Vanilla equivalent: `ButtonBlock` + `FaceAttachedHorizontalDirectionalBlock`.

use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{AttachFace, BlockStateProperties, Direction};
use steel_registry::vanilla_blocks;
use steel_utils::math::Axis;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehaviour;
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::player::Player;
use crate::world::World;

/// Behavior for all button block variants.
///
/// Stone buttons stay pressed for 20 ticks, wood buttons for 30 ticks.
/// Each variant has its own click on/off sounds determined by the block set type.
pub struct ButtonBlock {
    block: BlockRef,
    ticks_to_stay_pressed: i32,
    sound_click_on: i32,
    sound_click_off: i32,
}

impl ButtonBlock {
    /// Creates a new button block behavior.
    ///
    /// Parameters are provided by the build system from `classes.json`.
    #[must_use]
    pub const fn new(
        block: BlockRef,
        ticks_to_stay_pressed: i32,
        sound_click_on: i32,
        sound_click_off: i32,
    ) -> Self {
        Self {
            block,
            ticks_to_stay_pressed,
            sound_click_on,
            sound_click_off,
        }
    }

    /// Returns the outward direction the button faces (away from the support block).
    ///
    /// Vanilla equivalent: `FaceAttachedHorizontalDirectionalBlock.getConnectedDirection()`.
    fn get_connected_direction(state: BlockStateId) -> Direction {
        let face: AttachFace = state.get_value(&BlockStateProperties::ATTACH_FACE);
        match face {
            AttachFace::Floor => Direction::Up,
            AttachFace::Ceiling => Direction::Down,
            AttachFace::Wall => state.get_value(&BlockStateProperties::HORIZONTAL_FACING),
        }
    }

    /// Checks if a button with the given state can survive at the given position.
    fn can_survive(world: &World, pos: BlockPos, state: BlockStateId) -> bool {
        let support_dir = Self::get_connected_direction(state).opposite();
        let support_pos = support_dir.relative(&pos);
        let support_state = world.get_block_state(&support_pos);
        support_state.is_face_sturdy(support_dir.opposite())
    }

    /// Updates neighbors at both the button position and the support block position.
    ///
    /// Vanilla equivalent: `ButtonBlock.updateNeighbours()`.
    fn update_button_neighbors(&self, state: BlockStateId, world: &World, pos: BlockPos) {
        world.update_neighbors_at(&pos, self.block);
        let support_dir = Self::get_connected_direction(state).opposite();
        let support_pos = support_dir.relative(&pos);
        world.update_neighbors_at(&support_pos, self.block);
    }

    /// Presses the button: sets POWERED=true, updates neighbors, schedules unpress tick,
    /// and plays the click sound.
    fn press(&self, state: BlockStateId, world: &World, pos: BlockPos, player: &Player) {
        let powered_state = state.set_value(&BlockStateProperties::POWERED, true);
        world.set_block(pos, powered_state, UpdateFlags::UPDATE_ALL);
        self.update_button_neighbors(powered_state, world, pos);
        world.schedule_block_tick_default(pos, self.block, self.ticks_to_stay_pressed);
        world.play_block_sound(self.sound_click_on, pos, 1.0, 1.0, Some(player.id));
        // TODO: GameEvent.BLOCK_ACTIVATE when game event system exists
    }
}

impl BlockBehaviour for ButtonBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        for direction in context.get_nearest_looking_directions() {
            let state = if direction.get_axis() == Axis::Y {
                let face = if direction == Direction::Up {
                    AttachFace::Ceiling
                } else {
                    AttachFace::Floor
                };
                self.block
                    .default_state()
                    .set_value(&BlockStateProperties::ATTACH_FACE, face)
                    .set_value(
                        &BlockStateProperties::HORIZONTAL_FACING,
                        context.horizontal_direction,
                    )
            } else {
                self.block
                    .default_state()
                    .set_value(&BlockStateProperties::ATTACH_FACE, AttachFace::Wall)
                    .set_value(
                        &BlockStateProperties::HORIZONTAL_FACING,
                        direction.opposite(),
                    )
            };

            if Self::can_survive(context.world, context.relative_pos, state) {
                return Some(state);
            }
        }
        None
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let support_dir = Self::get_connected_direction(state).opposite();
        if direction == support_dir && !Self::can_survive(world, pos, state) {
            return REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR);
        }
        state
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
    ) -> InteractionResult {
        let powered: bool = state.get_value(&BlockStateProperties::POWERED);
        if powered {
            return InteractionResult::Fail;
        }
        self.press(state, world, pos, player);
        InteractionResult::Success
    }

    fn tick(&self, state: BlockStateId, world: &World, pos: BlockPos) {
        let powered: bool = state.get_value(&BlockStateProperties::POWERED);
        if !powered {
            return;
        }
        // TODO: Check for arrows via checkPressed() — wooden buttons should stay
        // pressed while an arrow is touching them and reschedule the tick.
        // Also needs entity_inside() on BlockBehaviour trait for arrows pressing
        // unpowered wooden buttons. Blocked on entity collision system.

        // Unpress the button
        let unpowered_state = state.set_value(&BlockStateProperties::POWERED, false);
        world.set_block(pos, unpowered_state, UpdateFlags::UPDATE_ALL);
        self.update_button_neighbors(state, world, pos);
        world.play_block_sound(self.sound_click_off, pos, 1.0, 1.0, None);
        // TODO: GameEvent.BLOCK_DEACTIVATE when game event system exists
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        if moved_by_piston {
            return;
        }
        let powered: bool = state.get_value(&BlockStateProperties::POWERED);
        if !powered {
            return;
        }
        self.update_button_neighbors(state, world, pos);
    }
}
