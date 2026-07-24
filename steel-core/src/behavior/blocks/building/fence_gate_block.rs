//! Fence gate block behavior.
//!
//! Vanilla equivalent: `FenceGateBlock` + `HorizontalDirectionalBlock`.
//!
//! Fence gates open/close on use, sit flush in walls (`IN_WALL`), and react to
//! redstone (`POWERED`/`OPEN` driven by `Level.hasNeighborSignal`).

use crate::behavior::InventoryAccess;
use crate::behavior::block::BlockBehavior;
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::entity::Entity;
use crate::entity::ai::path::PathComputationType;
use crate::player::Player;
use crate::world::game_event_context::GameEventContext;
use crate::world::{ScheduledTickAccess, SignalGetter as _, World};
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, Direction, EnumProperty,
};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_game_events;
use steel_utils::axis::Axis;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

/// Behavior for all fence gate variants.
#[block_behavior]
pub struct FenceGateBlock {
    block: BlockRef,
    #[json_arg(sound_events, json = "type_fence_gate_open")]
    sound_open: SoundEventRef,
    #[json_arg(sound_events, json = "type_fence_gate_close")]
    sound_close: SoundEventRef,
}

/// Horizontal facing of the gate.
const FACING: EnumProperty<Direction> = BlockStateProperties::HORIZONTAL_FACING;
/// Whether the gate is open.
const OPEN: BoolProperty = BlockStateProperties::OPEN;
/// Whether the gate is powered by redstone.
const POWERED: BoolProperty = BlockStateProperties::POWERED;
/// Whether the gate is lowered to sit flush inside a wall.
const IN_WALL: BoolProperty = BlockStateProperties::IN_WALL;

impl FenceGateBlock {
    /// Creates a new fence gate behavior.
    ///
    /// Sound events are provided by the build system from `classes.json`.
    #[must_use]
    pub const fn new(
        block: BlockRef,
        sound_open: SoundEventRef,
        sound_close: SoundEventRef,
    ) -> Self {
        Self {
            block,
            sound_open,
            sound_close,
        }
    }

    /// Vanilla `FenceGateBlock.connectsToDirection`.
    ///
    /// A gate connects perpendicular to its facing, i.e. to a wall/fence whose
    /// connecting axis matches the gate's clockwise-rotated facing axis.
    #[must_use]
    pub fn connects_to_direction(state: BlockStateId, direction: Direction) -> bool {
        state.get_value(&FACING).axis() == direction.rotate_y_clockwise().axis()
    }

    /// Vanilla `FenceGateBlock.isWall`.
    fn is_wall(state: BlockStateId) -> bool {
        state.get_block().has_tag(&BlockTag::WALLS)
    }
}

impl BlockBehavior for FenceGateBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let world = context.world;
        let pos = context.place_pos();
        let direction = context.horizontal_direction();
        let axis = direction.axis();

        let in_wall = match axis {
            Axis::Z => {
                Self::is_wall(world.get_block_state(Direction::West.relative(pos)))
                    || Self::is_wall(world.get_block_state(Direction::East.relative(pos)))
            }
            Axis::X => {
                Self::is_wall(world.get_block_state(Direction::North.relative(pos)))
                    || Self::is_wall(world.get_block_state(Direction::South.relative(pos)))
            }
            Axis::Y => false,
        };

        let is_open = world.has_neighbor_signal(pos);

        Some(
            self.block
                .default_state()
                .set_value(&FACING, direction)
                .set_value(&OPEN, is_open)
                .set_value(&POWERED, is_open)
                .set_value(&IN_WALL, in_wall),
        )
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        // Only the axis perpendicular to the gate (its clockwise facing axis)
        // can change whether it sits in a wall.
        if state.get_value(&FACING).rotate_y_clockwise().axis() != direction.axis() {
            return state;
        }
        let opposite_neighbor = world.get_block_state(direction.opposite().relative(pos));
        let in_wall = Self::is_wall(neighbor_state) || Self::is_wall(opposite_neighbor);
        state.set_value(&IN_WALL, in_wall)
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let mut new_state = state;
        if new_state.get_value(&OPEN) {
            new_state = new_state.set_value(&OPEN, false);
        } else {
            let (yaw, _) = player.rotation();
            let player_direction = Direction::from_yaw(yaw);
            // Re-face the gate toward the player if they opened it from behind.
            if new_state.get_value(&FACING) == player_direction.opposite() {
                new_state = new_state.set_value(&FACING, player_direction);
            }
            new_state = new_state.set_value(&OPEN, true);
        }

        // Vanilla flag 10 = UPDATE_CLIENTS | UPDATE_IMMEDIATE.
        world.set_block(
            pos,
            new_state,
            UpdateFlags::UPDATE_CLIENTS | UpdateFlags::UPDATE_IMMEDIATE,
        );

        let opens = new_state.get_value(&OPEN);
        let sound = if opens {
            self.sound_open
        } else {
            self.sound_close
        };
        let pitch = rand::random::<f32>() * 0.1 + 0.9;
        world.play_block_sound(sound, pos, 1.0, pitch, Some(player.id()));
        let event = if opens {
            &vanilla_game_events::BLOCK_OPEN
        } else {
            &vanilla_game_events::BLOCK_CLOSE
        };
        world.game_event(event, pos, &GameEventContext::new(Some(player), None));
        InteractionResult::Success
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        let has_power = world.has_neighbor_signal(pos);
        if state.get_value(&POWERED) == has_power {
            return;
        }

        world.set_block(
            pos,
            state
                .set_value(&POWERED, has_power)
                .set_value(&OPEN, has_power),
            UpdateFlags::UPDATE_CLIENTS,
        );
        if state.get_value(&OPEN) == has_power {
            return;
        }

        let sound = if has_power {
            self.sound_open
        } else {
            self.sound_close
        };
        let pitch = rand::random::<f32>() * 0.1 + 0.9;
        world.play_block_sound(sound, pos, 1.0, pitch, None);
        let event = if has_power {
            &vanilla_game_events::BLOCK_OPEN
        } else {
            &vanilla_game_events::BLOCK_CLOSE
        };
        world.game_event(event, pos, &GameEventContext::default());
    }

    fn is_pathfindable(&self, state: BlockStateId, computation_type: PathComputationType) -> bool {
        match computation_type {
            PathComputationType::Land | PathComputationType::Air => state.get_value(&OPEN),
            PathComputationType::Water => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{
        blocks::properties::BlockStateProperties, test_support::init_test_registry, vanilla_blocks,
    };
    use steel_utils::{ChunkPos, types::UpdateFlags};

    use super::*;
    use crate::{
        behavior::init_behaviors,
        test_support::{fresh_test_world, insert_ready_full_chunk},
    };

    #[test]
    fn redstone_power_opens_and_closes_fence_gate() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("fence_gate_redstone");
        let pos = BlockPos::new(8, 64, 8);
        let power_pos = pos.west();
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos,
            vanilla_blocks::OAK_FENCE_GATE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        assert!(world.set_block(
            power_pos,
            vanilla_blocks::REDSTONE_BLOCK.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        let powered = world.get_block_state(pos);
        assert!(powered.get_value(&BlockStateProperties::POWERED));
        assert!(powered.get_value(&BlockStateProperties::OPEN));

        assert!(world.remove_block(power_pos, false));
        let unpowered = world.get_block_state(pos);
        assert!(!unpowered.get_value(&BlockStateProperties::POWERED));
        assert!(!unpowered.get_value(&BlockStateProperties::OPEN));
    }
}
