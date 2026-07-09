use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, EnumProperty, IntProperty};
use steel_registry::items::ItemRef;
use steel_registry::vanilla_blocks;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::{BlockBehavior, BlockPlaceContext, InteractionResult};
use crate::world::LevelReader;

pub const MAX_SEGMENT_AMOUNT: u8 = 4;
const FACING_PROPERTY: EnumProperty<Direction> = BlockStateProperties::HORIZONTAL_FACING;

pub trait SegmentableBlock: BlockBehavior {
    fn block_ref(&self) -> &BlockRef;
    fn segment_property(&self) -> &IntProperty;

    fn segmentable_update_shape(
        &self,
        state: BlockStateId,
        world: &dyn crate::world::ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if self.can_survive(state, world, pos) {
            state
        } else {
            vanilla_blocks::AIR.default_state()
        }
    }

    fn segmentable_use_item_on(
        &self,
        state: BlockStateId,
        world: &std::sync::Arc<crate::world::World>,
        pos: BlockPos,
        _player: &crate::player::Player,
        _hand: steel_utils::types::InteractionHand,
        _hit_result: &steel_registry::items::item::BlockHitResult,
        inv: &mut crate::behavior::InventoryAccess,
    ) -> InteractionResult {
        if inv.with_item(|item_stack| item_stack.item().key == state.get_block().key) {
            let current_amount = state.get_value(self.segment_property());

            if current_amount < MAX_SEGMENT_AMOUNT {
                let block_state = state.set_value(self.segment_property(), current_amount + 1);
                world.set_block(pos, block_state, UpdateFlags::UPDATE_CLIENTS);

                return InteractionResult::Consume;
            };
        };

        InteractionResult::Fail
    }

    fn segmentable_get_state_for_placement(
        &self,
        context: &BlockPlaceContext<'_>,
    ) -> Option<BlockStateId> {
        for direction in context.get_nearest_looking_directions() {
            if !direction.is_horizontal() {
                continue;
            }

            let existing_state = context.world.get_block_state(context.place_pos);
            let state = self
                .block_ref()
                .default_state()
                .set_value(&FACING_PROPERTY, direction.opposite());

            if existing_state.get_block() == *self.block_ref() {
                let current_amount = existing_state.get_value(self.segment_property());
                if current_amount < MAX_SEGMENT_AMOUNT {
                    let new_state =
                        existing_state.set_value(self.segment_property(), current_amount + 1);
                    return Some(new_state);
                }
                return None;
            }

            if self.can_survive(state, context.world, context.place_pos) {
                return Some(state);
            }
        }

        None
    }

    fn segmentable_can_be_replaced(
        &self,
        state: BlockStateId,
        held_item: ItemRef,
        is_secondary_use_active: bool,
    ) -> bool {
        if !is_secondary_use_active && held_item.key == state.get_block().key {
            let current_amount = state.get_value(self.segment_property());
            return current_amount < MAX_SEGMENT_AMOUNT;
        }

        state.get_block().config.replaceable
    }
}
