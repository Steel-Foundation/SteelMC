use crate::behavior::block::drop_from_block_interact_loot_table;
use crate::behavior::{
    BlockBehavior, BlockPlaceContext, BlockRef, InteractionResult, InventoryAccess,
};
use crate::entity::Entity;
use crate::player::Player;
use crate::world::World;
use crate::world::game_event::GameEventContext;
use glam::DVec3;
use rand::RngExt;
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, EnumProperty};
use steel_registry::items::item::BlockHitResult;
use steel_registry::{
    sound_events, vanilla_blocks, vanilla_game_events, vanilla_items, vanilla_loot_tables,
};
use steel_utils::axis::Axis;
use steel_utils::types::{InteractionHand, UpdateFlags};
use steel_utils::{BlockPos, BlockStateId};

/// Behavior for pumpkins.
#[block_behavior]
pub struct PumpkinBlock {
    block: BlockRef,
}

const HORIZONTAL_FACING: &EnumProperty<Direction> = &BlockStateProperties::HORIZONTAL_FACING;

impl PumpkinBlock {
    /// Creates a pumpkin block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for PumpkinBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn use_item_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hand: InteractionHand,
        hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let mut rng = rand::rng();

        let Some(drops) = inv.with_item(|item_stack| {
            if !item_stack.is(&vanilla_items::SHEARS) {
                return None;
            }

            Some(drop_from_block_interact_loot_table(
                &vanilla_loot_tables::CARVE_PUMPKIN,
                state,
                world.get_block_entity(pos),
                Some(item_stack),
                Some(player),
                &mut rng,
            ))
        }) else {
            return InteractionResult::TryEmptyHandInteraction;
        };

        let clicked_direction = hit_result.direction;
        let direction = if clicked_direction.axis() == Axis::Y {
            player.direction_yaw().opposite()
        } else {
            clicked_direction
        };

        let (x_offset, z_offset) = {
            let (x, _, z) = direction.offset();
            (f64::from(x), f64::from(z))
        };

        for drop in drops {
            world.spawn_item_with_velocity(
                DVec3::new(
                    f64::from(pos.x()) + 0.5 + x_offset * 0.65,
                    f64::from(pos.y()) + 0.1,
                    f64::from(pos.z()) + 0.5 + z_offset * 0.65,
                ),
                drop,
                DVec3::new(
                    0.05 * x_offset + rng.random::<f64>() * 0.02,
                    0.05,
                    0.05 * z_offset + rng.random::<f64>() * 0.02,
                ),
            );
        }

        world.play_block_sound(&sound_events::BLOCK_PUMPKIN_CARVE, pos, 1.0, 1.0, None);
        world.set_block(
            pos,
            vanilla_blocks::CARVED_PUMPKIN
                .default_state()
                // TODO: Once CarvedPumpkinBlock is implemented, replace HORIZONTAL_FACING with FACING of CarvedPumpkinBlock.
                .set_value(HORIZONTAL_FACING, direction),
            UpdateFlags::UPDATE_IMMEDIATE
                | UpdateFlags::UPDATE_CLIENTS
                | UpdateFlags::UPDATE_NEIGHBORS,
        );
        let has_infinite_materials = player.has_infinite_materials();
        inv.with_item(|item_stack| item_stack.hurt_and_break(1, has_infinite_materials));

        world.game_event(
            &vanilla_game_events::SHEAR,
            pos,
            &GameEventContext::new(Some(player), None),
        );

        // TODO: Award statistic ITEM_USED with SHEARS.

        InteractionResult::Success
    }
}
