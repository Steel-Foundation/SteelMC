// Vanilla bell behavior.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BellAttachType, BlockStateProperties, Direction,
};
use steel_registry::{sound_events, vanilla_blocks};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{BlockBehavior, BlockHitResult, BlockPlaceContext, InteractionResult, InventoryAccess,
};
use crate::entity::ai::path::PathComputationType;
use crate::player::Player;
use crate::world::{LevelReader, ScheduledTickAccess, SignalGetter as _, World};

const RING_EVENT: i32 = 1;
const MAX_HIT_HEIGHT: f64 = 0.8125;

#[block_behavior]
pub struct BellBlock {
    block: BlockRef,
}

impl BellBlock {
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    const fn direction_legacy_id(direction: Direction) -> i32 {
        match direction {
            Direction::Down => 0,
            Direction::Up => 1,
            Direction::North => 2,
            Direction::South => 3,
            Direction::West => 4,
            Direction::East => 5,
        }
    }

    fn connected_direction(state: BlockStateId) -> Direction {
        match state.get_value(&BlockStateProperties::BELL_ATTACHMENT) {
            BellAttachType::Floor => Direction::Down,
            BellAttachType::Ceiling => Direction::Up,
            BellAttachType::SingleWall | BellAttachType::DoubleWall => {
                state.get_value(&BlockStateProperties::FACING).opposite()
            }
        }
    }

    fn has_support(world: &dyn LevelReader, pos: BlockPos, direction: Direction) -> bool {
        let support_pos = pos.relative(direction);
        let support = world.get_block_state(support_pos);

        world.is_face_sturdy(support, support_pos, direction.opposite())
    }

    fn ring(&self, world: &Arc<World>, pos: BlockPos, direction: Direction) {
        world.block_event(
            pos,
            self.block,
            RING_EVENT,
            Self::direction_legacy_id(direction),
        );

        world.play_sound(
            &sound_events::BLOCK_BELL_USE,
            SoundSource::Blocks,
            pos,
            2.0,
            1.0,
            None,
        );
    }

    fn is_proper_hit(state: BlockStateId, hit: &BlockHitResult, pos: BlockPos) -> bool {
        if hit.direction.axis().is_vertical() {
            return false;
        }

        let height = hit.location.y - f64::from(pos.y());
        if height > MAX_HIT_HEIGHT {
            return false;
        }

        let facing = state.get_value(&BlockStateProperties::FACING);
        let attachment = state.get_value(&BlockStateProperties::BELL_ATTACHMENT);

        match attachment {
            BellAttachType::Floor => facing.axis() == hit.direction.axis(),
            BellAttachType::Ceiling => true,
            BellAttachType::SingleWall | BellAttachType::DoubleWall => {
                facing.axis() != hit.direction.axis()
            }
        }
    }

    fn update_powered(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let powered = world.has_neighbor_signal(pos);
        let old_powered = state.get_value(&BlockStateProperties::POWERED);

        if powered == old_powered {
            return;
        }

        if powered {
            let direction = state.get_value(&BlockStateProperties::FACING);
            self.ring(world, pos, direction);
        }

        let new_state = state.set_value(&BlockStateProperties::POWERED, powered);
        world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL);
    }
}

impl BlockBehavior for BellBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let pos = context.place_pos();
        let clicked_face = context.clicked_face();
        let player_facing = context.horizontal_direction();

        let mut state = self.block.default_state();
        state = state.set_value(&BlockStateProperties::FACING, player_facing);

        if clicked_face == Direction::Up {
            state = state.set_value(
                &BlockStateProperties::BELL_ATTACHMENT,
                BellAttachType::Floor,
            );
        } else if clicked_face == Direction::Down {
            state = state.set_value(
                &BlockStateProperties::BELL_ATTACHMENT,
                BellAttachType::Ceiling,
            );
        } else {
            let wall_facing = clicked_face.opposite();

            state = state.set_value(
                &BlockStateProperties::FACING,
                wall_facing,
            );

            let opposite = wall_facing.opposite();

            let double_wall = Self::has_support(context.world, pos, wall_facing)
                && Self::has_support(context.world, pos, opposite);

            let attachment = if double_wall {
                BellAttachType::DoubleWall
            } else {
                BellAttachType::SingleWall
            };

            state = state.set_value(
                &BlockStateProperties::BELL_ATTACHMENT,
                attachment,
            );
        }

        let powered = context.world.has_neighbor_signal(pos);
        state = state.set_value(&BlockStateProperties::POWERED, powered);

        self.can_survive(state, context.world, pos).then_some(state)
    }

    fn can_survive(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        let direction = Self::connected_direction(state);
        Self::has_support(world, pos, direction)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let attachment = state.get_value(&BlockStateProperties::BELL_ATTACHMENT);

        if attachment != BellAttachType::DoubleWall {
            let support_direction = Self::connected_direction(state);

            if direction != support_direction {
                return state;
            }

            if Self::has_support(world, pos, support_direction) {
                return state;
            }

            return vanilla_blocks::AIR.default_state();
        }

        let facing = state.get_value(&BlockStateProperties::FACING);
        let opposite = facing.opposite();

        let first_support = Self::has_support(world, pos, facing);
        let second_support = Self::has_support(world, pos, opposite);

        if first_support && second_support {
            return state;
        }

        if first_support {
            return state.set_value(
                &BlockStateProperties::BELL_ATTACHMENT,
                BellAttachType::SingleWall,
            );
        }

        if second_support {
            let new_state = state.set_value(
                &BlockStateProperties::FACING,
                opposite,
            );

            return new_state.set_value(
                &BlockStateProperties::BELL_ATTACHMENT,
                BellAttachType::SingleWall,
            );
        }

        vanilla_blocks::AIR.default_state()
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        self.update_powered(state, world, pos);
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _player: &Player,
        hit: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        if !Self::is_proper_hit(state, hit, pos) {
            return InteractionResult::Pass;
        }

        self.ring(world, pos, hit.direction);
        InteractionResult::Success
    }

    fn trigger_event(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        event: i32,
        _data: i32,
    ) -> bool {
        event == RING_EVENT
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }
}
