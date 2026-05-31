//! Block item behavior implementation.

use steel_macros::item_behavior;
use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    vanilla_blocks, vanilla_game_events,
};
use steel_utils::{BlockStateId, types::UpdateFlags};

use crate::behavior::context::{BlockPlaceContext, InteractionResult, UseOnContext};
use crate::behavior::{BLOCK_BEHAVIORS, ItemBehavior};
use crate::fluid::{FluidStateExt as _, get_fluid_state};
use crate::world::game_event_context::GameEventContext;

/// Behavior for items that place blocks.
#[item_behavior]
pub struct BlockItem {
    /// The block this item places.
    #[json_arg(vanilla_blocks, json = "block")]
    pub block: BlockRef,
}

impl BlockItem {
    const PLACE_BLOCK_FLAGS: UpdateFlags = UpdateFlags::UPDATE_ALL_IMMEDIATE;

    /// Creates a new block item behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn place_with(
        &self,
        context: &mut UseOnContext<'_>,
        place_block: impl FnOnce(&BlockPlaceContext<'_>, BlockStateId) -> bool,
    ) -> InteractionResult {
        let Some(place_context) = context.build_place_context() else {
            return InteractionResult::Fail;
        };
        let place_pos = place_context.relative_pos;

        let behavior = BLOCK_BEHAVIORS.get_behavior(self.block);
        let Some(new_state) = behavior.get_state_for_placement(&place_context) else {
            return InteractionResult::Fail;
        };

        if !behavior.can_survive(new_state, place_context.world, place_pos) {
            return InteractionResult::Fail;
        }

        let collision_shape = new_state.get_collision_shape();
        if !context.world.is_unobstructed(collision_shape, place_pos) {
            return InteractionResult::Fail;
        }

        if !place_block(&place_context, new_state) {
            return InteractionResult::Fail;
        }

        let placed_state = context.world.get_block_state(place_pos);
        if placed_state.get_block() == self.block {
            let placed_behavior = BLOCK_BEHAVIORS.get_behavior(placed_state.get_block());
            placed_behavior.set_placed_by(
                placed_state,
                context.world,
                place_pos,
                Some(context.player),
                &context.inv,
            );
        }

        // Play place sound (exclude the placing player, they hear it client-side)
        let sound_type = &self.block.config.sound_type;
        context.world.play_block_sound(
            sound_type.place_sound,
            place_pos,
            sound_type.volume,
            sound_type.pitch,
            Some(context.player.id),
        );
        context.world.game_event(
            &vanilla_game_events::BLOCK_PLACE,
            place_pos,
            &GameEventContext::new(Some(context.player), Some(placed_state)),
        );

        context.inv.with_item(|item| item.shrink(1));

        InteractionResult::Success
    }

    fn place_block(context: &BlockPlaceContext<'_>, state: BlockStateId) -> bool {
        context
            .world
            .set_block(context.relative_pos, state, Self::PLACE_BLOCK_FLAGS)
    }
}

impl ItemBehavior for BlockItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        self.place_with(context, |place_context, state| {
            Self::place_block(place_context, state)
        })
    }
}

/// Behavior for double-high block items (doors, tall flowers, etc.).
///
/// Vanilla's `DoubleHighBlockItem` extends `BlockItem` and overrides `placeBlock`
/// to place the upper half block above the lower half.
///
/// The `_block` field is read by the build script via `#[json_arg]` to generate constructor
/// calls from `classes.json`. The actual value is forwarded into `base`.
#[item_behavior]
pub struct DoubleHighBlockItem {
    #[json_arg(vanilla_blocks, json = "block")]
    _block: BlockRef,
    base: BlockItem,
}

impl DoubleHighBlockItem {
    const PREPARE_UPPER_FLAGS: UpdateFlags =
        UpdateFlags::UPDATE_ALL_IMMEDIATE.union(UpdateFlags::UPDATE_KNOWN_SHAPE);

    /// Creates a new double-high block item behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            _block: block,
            base: BlockItem::new(block),
        }
    }

    fn place_block(context: &BlockPlaceContext<'_>, state: BlockStateId) -> bool {
        let above = context.relative_pos.above();
        let above_state = if get_fluid_state(context.world, above).is_water() {
            vanilla_blocks::WATER.default_state()
        } else {
            vanilla_blocks::AIR.default_state()
        };
        let _ = context
            .world
            .set_block(above, above_state, Self::PREPARE_UPPER_FLAGS);

        BlockItem::place_block(context, state)
    }
}

impl ItemBehavior for DoubleHighBlockItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        self.base.place_with(context, |place_context, state| {
            Self::place_block(place_context, state)
        })
    }
}
