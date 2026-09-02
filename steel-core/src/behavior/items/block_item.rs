//! Block item behavior implementation.

use std::ptr;
use std::sync::Arc;

use steel_macros::item_behavior;
use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt, shapes::OffsetVoxelShape},
    data_components::vanilla_components::BLOCK_ENTITY_DATA,
    item_stack::ItemStack,
    sound_event::SoundEventRef,
    vanilla_blocks, vanilla_game_events,
};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::context::{BlockPlaceContext, InteractionResult, UseOnContext};
use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionContext, ItemBehavior};
use crate::block_entity::BlockEntityComponentsExt as _;
use crate::entity::Entity;
use crate::fluid::{FluidStateExt as _, get_fluid_state};
use crate::player::Player;
use crate::world::World;
use crate::world::game_event::GameEventContext;

pub(super) enum SurvivalCheck {
    Required,
    Skipped,
}

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

    pub(super) fn place_with(
        &self,
        context: BlockPlaceContext<'_>,
        place_block: impl FnOnce(&BlockPlaceContext<'_>, BlockStateId) -> bool,
    ) -> InteractionResult {
        self.place_with_policy(
            context,
            Some,
            SurvivalCheck::Required,
            place_block,
            self.block.config.sound_type.place_sound,
        )
    }

    pub(super) fn place_with_sound_and_block(
        &self,
        context: BlockPlaceContext<'_>,
        place_block: impl FnOnce(&BlockPlaceContext<'_>, BlockStateId) -> bool,
        place_sound: SoundEventRef,
    ) -> InteractionResult {
        self.place_with_policy(
            context,
            Some,
            SurvivalCheck::Required,
            place_block,
            place_sound,
        )
    }

    #[expect(
        clippy::manual_midpoint,
        reason = "Matches vanilla BlockItem::place's sound volume formula"
    )]
    pub(super) fn place_with_policy<'a>(
        &self,
        context: BlockPlaceContext<'a>,
        update_context: impl FnOnce(BlockPlaceContext<'a>) -> Option<BlockPlaceContext<'a>>,
        survival_check: SurvivalCheck,
        place_block: impl FnOnce(&BlockPlaceContext<'a>, BlockStateId) -> bool,
        place_sound: SoundEventRef,
    ) -> InteractionResult {
        if !context.can_place() {
            return InteractionResult::Fail;
        }
        let Some(mut context) = update_context(context) else {
            return InteractionResult::Fail;
        };
        let place_pos = context.place_pos();

        let behavior = BLOCK_BEHAVIORS.get_behavior(self.block);
        let Some(new_state) = behavior.get_state_for_placement(&context) else {
            return InteractionResult::Fail;
        };

        if matches!(survival_check, SurvivalCheck::Required)
            && !behavior.can_survive(new_state, context.world.as_ref(), place_pos)
        {
            return InteractionResult::Fail;
        }

        let collision_context = context.player().map_or_else(
            BlockCollisionContext::placement_without_entity,
            |player| {
                BlockCollisionContext::with_position(player.position().y, player.is_descending())
            },
        );
        let collision_shape = OffsetVoxelShape::new(
            behavior.get_collision_shape(
                new_state,
                context.world.as_ref(),
                place_pos,
                collision_context,
            ),
            behavior.get_collision_shape_offset(
                new_state,
                context.world.as_ref(),
                place_pos,
                collision_context,
            ),
        );
        if !context.world.is_unobstructed(collision_shape, place_pos) {
            return InteractionResult::Fail;
        }

        if !place_block(&context, new_state) {
            return InteractionResult::Fail;
        }

        let placed_state = context.world.get_block_state(place_pos);
        if placed_state.get_block() == self.block {
            // TODO: Apply the `BLOCK_STATE` component (Vanilla `updateBlockStateFromTag`)
            // once Steel can resolve block-state properties from their serialized names.
            // Block-entity callbacks must not run under the inventory lock, so the
            // live stack is snapshotted first.
            let stack = context.with_item(|item| item.copy_with_count(item.count()));
            Self::update_custom_block_entity_tag(
                context.world,
                context.player(),
                place_pos,
                &stack,
            );
            Self::update_block_entity_components(context.world, place_pos, &stack);
            let placed_behavior = BLOCK_BEHAVIORS.get_behavior(placed_state.get_block());
            placed_behavior.set_placed_by(placed_state, context.world, place_pos, context.source());
        }

        // Play place sound (exclude the placing player, they hear it client-side)
        context.world.play_block_sound(
            place_sound,
            place_pos,
            (self.block.config.sound_type.volume + 1.0) / 2.0,
            self.block.config.sound_type.pitch * 0.8,
            context.player().map(Entity::id),
        );
        context.world.game_event(
            &vanilla_game_events::BLOCK_PLACE,
            place_pos,
            &GameEventContext::new(
                context.player().map(|player| player as &dyn Entity),
                Some(placed_state),
            ),
        );

        context.with_item_mut(|item| item.shrink(1));

        InteractionResult::Success
    }

    /// Places this block using an already constructed placement context.
    pub fn place(&self, context: BlockPlaceContext<'_>) -> InteractionResult {
        self.place_with(context, Self::place_block)
    }

    pub(super) fn place_block(context: &BlockPlaceContext<'_>, state: BlockStateId) -> bool {
        context
            .world
            .set_block(context.place_pos(), state, Self::PLACE_BLOCK_FLAGS)
    }

    /// Loads the stack's `BLOCK_ENTITY_DATA` into the placed block entity.
    ///
    /// Mirrors Vanilla `BlockItem.updateCustomBlockEntityTag`, including its
    /// game-master restriction for op-only block-entity types.
    fn update_custom_block_entity_tag(
        world: &Arc<World>,
        player: Option<&Player>,
        pos: BlockPos,
        stack: &ItemStack,
    ) -> bool {
        let Some(custom_data) = stack.get(BLOCK_ENTITY_DATA) else {
            return false;
        };
        let Some(block_entity) = world.get_block_entity(pos) else {
            return false;
        };
        let block_entity_type = block_entity.get_type();
        if !ptr::eq(block_entity_type, custom_data.block_entity_type()) {
            return false;
        }
        if block_entity_type.only_op_can_set_nbt()
            && !player.is_some_and(Player::can_use_game_master_blocks)
        {
            return false;
        }
        block_entity.load_custom_data(custom_data.data())
    }

    /// Mirrors Vanilla `BlockItem.updateBlockEntityComponents`.
    fn update_block_entity_components(world: &Arc<World>, pos: BlockPos, stack: &ItemStack) {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return;
        };
        block_entity.apply_components_from_item_stack(stack);
        block_entity.set_changed();
    }
}

impl ItemBehavior for BlockItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        self.place(context.build_place_context())
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
        let above = context.place_pos().above();
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
        self.base
            .place_with(context.build_place_context(), Self::place_block)
    }
}
