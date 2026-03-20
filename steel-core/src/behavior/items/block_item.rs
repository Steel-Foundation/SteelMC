//! Block item behavior implementation.

use steel_macros::item_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_utils::types::UpdateFlags;

use crate::behavior::context::{InteractionResult, UseOnContext};
use crate::behavior::{BLOCK_BEHAVIORS, ItemBehavior};

/// Behavior for items that place blocks.
#[item_behavior(class = "BlockItem")]
pub struct BlockItemBehavior {
    /// The block this item places.
    #[json_arg(vanilla_blocks, json = "block")]
    pub block: BlockRef,
}

impl BlockItemBehavior {
    /// Creates a new block item behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl ItemBehavior for BlockItemBehavior {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let Some(place_context) = context.build_place_context() else {
            return InteractionResult::Fail;
        };
        let place_pos = place_context.relative_pos;

        let behavior = BLOCK_BEHAVIORS.get_behavior(self.block);
        let Some(new_state) = behavior.get_state_for_placement(&place_context) else {
            return InteractionResult::Fail;
        };

        // Check if the block placement would intersect with any entity (vanilla: Level.isUnobstructed)
        let collision_shape = new_state.get_collision_shape();
        if !context.world.is_unobstructed(collision_shape, place_pos) {
            return InteractionResult::Fail;
        }

        if !context
            .world
            .set_block(place_pos, new_state, UpdateFlags::UPDATE_ALL_IMMEDIATE)
        {
            return InteractionResult::Fail;
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

        context.item().shrink(1);

        // TODO: Call behavior.on_place() — triggers neighbor updates (redstone, etc.)
        InteractionResult::Success
    }
}

/// Behavior for double-high block items (doors, tall flowers, etc.).
///
/// Vanilla's `DoubleHighBlockItem` extends `BlockItem` and overrides `placeBlock`
/// to place the upper half block above the lower half.
///
/// The `_block` field is read by the build script via `#[json_arg]` to generate constructor
/// calls from `classes.json`. The actual value is forwarded into `base`.
#[item_behavior(class = "DoubleHighBlockItem")]
pub struct DoubleHighBlockItemBehavior {
    #[json_arg(vanilla_blocks, json = "block")]
    _block: BlockRef,
    base: BlockItemBehavior,
}

impl DoubleHighBlockItemBehavior {
    /// Creates a new double-high block item behavior for the given block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            _block: block,
            base: BlockItemBehavior::new(block),
        }
    }
}

impl ItemBehavior for DoubleHighBlockItemBehavior {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        // TODO: Implement vanilla's double-high placement (place upper half block above)
        self.base.use_on(context)
    }
}
