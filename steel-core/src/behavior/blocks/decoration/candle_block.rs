use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{
    REGISTRY,
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, BoolProperty, IntProperty},
        shapes::SupportType,
    },
    entity_data::Direction,
    fluid::FluidState,
    items::item::BlockHitResult,
    sound_events, vanilla_blocks, vanilla_fluids, vanilla_game_events,
};
use steel_utils::{
    BlockPos,
    types::{self, UpdateFlags},
};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext, InteractionResult, InventoryAccess,
        block::{
            default_can_be_replaced, schedule_placed_liquid_tick,
            schedule_water_tick_if_waterlogged,
        },
    },
    entity::projectile::Projectile,
    player,
    world::{
        ClipHitResult, LevelAccessor, LevelReader, ScheduledTickAccess, World,
        game_event::GameEventContext,
    },
};

const CANDLES_PROPERTY: &IntProperty = &BlockStateProperties::CANDLES;
const LIT_PROPERTY: &BoolProperty = &BlockStateProperties::LIT;
const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;
const MAX_CANDLES: u8 = 4;

/// Behavior for all Candle type blocks
#[block_behavior]
pub struct CandleBlock {
    block: BlockRef,
}

impl CandleBlock {
    /// Creates a new candle block behavior for the given block
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    pub(super) fn projectile_lit_state(
        state: steel_utils::BlockStateId,
        projectile_is_on_fire: bool,
    ) -> Option<steel_utils::BlockStateId> {
        (projectile_is_on_fire
            && state.try_get_value(WATERLOGGED) != Some(true)
            && !state.get_value(LIT_PROPERTY))
        .then(|| state.set_value(LIT_PROPERTY, true))
    }
}

impl BlockBehavior for CandleBlock {
    /// Checks if the candle block can survive at the given position.
    fn can_survive(
        &self,
        _state: steel_utils::BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        let below_pos = pos.below();
        world.is_face_sturdy_for(
            world.get_block_state(below_pos),
            below_pos,
            Direction::Up,
            SupportType::Center,
        )
    }

    fn can_be_replaced(
        &self,
        state: steel_utils::BlockStateId,
        context: &BlockPlaceContext<'_>,
    ) -> bool {
        (!context.is_secondary_use_active()
            && context.with_item(|item| item.item() == REGISTRY.items.by_block(self.block))
            && state.get_value(CANDLES_PROPERTY) < MAX_CANDLES)
            || default_can_be_replaced(state, context)
    }

    fn get_state_for_placement(
        &self,
        context: &BlockPlaceContext<'_>,
    ) -> Option<steel_utils::BlockStateId> {
        let state = context.world.get_block_state(context.place_pos());
        if state.get_block() == self.block {
            return Some(state.set_value(
                CANDLES_PROPERTY,
                (state.get_value(CANDLES_PROPERTY) + 1).min(MAX_CANDLES),
            ));
        }

        let default_state = self.block.default_state();
        self.can_survive(default_state, context.world.as_ref(), context.place_pos())
            .then(|| default_state.set_value(WATERLOGGED, context.is_water_source()))
    }

    fn update_shape(
        &self,
        state: steel_utils::BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: steel_utils::BlockStateId,
    ) -> steel_utils::BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);

        if !self.can_survive(state, world, pos) {
            return REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        }
        state
    }

    fn on_projectile_hit(
        &self,
        state: steel_utils::BlockStateId,
        world: &Arc<World>,
        hit: &ClipHitResult,
        projectile: &dyn Projectile,
    ) {
        let Some(lit_state) = Self::projectile_lit_state(state, projectile.is_on_fire()) else {
            return;
        };
        world.set_block(hit.block_pos, lit_state, UpdateFlags::UPDATE_ALL_IMMEDIATE);
    }

    fn use_item_on(
        &self,
        state: steel_utils::BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &player::Player,
        _hand: types::InteractionHand,
        _hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let item_is_empty = inv.with_item(|item_stack| item_stack.is_empty());
        if item_is_empty && player.abilities.lock().may_build && state.get_value(LIT_PROPERTY) {
            let new_state = state.set_value(LIT_PROPERTY, false);
            world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL_IMMEDIATE);
            world.play_block_sound(&sound_events::BLOCK_CANDLE_EXTINGUISH, pos, 1.0, 1.0, None);
            world.game_event(
                &vanilla_game_events::BLOCK_CHANGE,
                pos,
                &GameEventContext::new(Some(player), None),
            );
            return InteractionResult::Success;
        }

        InteractionResult::TryEmptyHandInteraction
    }

    fn place_liquid(
        &self,
        level: &dyn LevelAccessor,
        pos: BlockPos,
        state: steel_utils::BlockStateId,
        fluid_state: FluidState,
    ) -> bool {
        if state.try_get_value(WATERLOGGED) != Some(false)
            || fluid_state.fluid_id != &vanilla_fluids::WATER
        {
            return false;
        }

        let waterlogged = state.set_value(WATERLOGGED, true);
        if state.get_value(LIT_PROPERTY) {
            let extinguished = waterlogged.set_value(LIT_PROPERTY, false);
            level.set_block_state(pos, extinguished, UpdateFlags::UPDATE_ALL_IMMEDIATE);
            level.play_block_sound(&sound_events::BLOCK_CANDLE_EXTINGUISH, pos, 1.0, 1.0, None);
            level.game_event(
                &vanilla_game_events::BLOCK_CHANGE,
                pos,
                &GameEventContext::new(None, Some(extinguished)),
            );
        } else {
            level.set_block_state(pos, waterlogged, UpdateFlags::UPDATE_ALL);
        }

        schedule_placed_liquid_tick(level, pos, fluid_state);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, item_stack::ItemStack, vanilla_items};
    use steel_utils::ChunkPos;

    use crate::{
        behavior::{PlacementOrientation, PlacementSource, init_behaviors},
        test_support::{TestLevel, fresh_test_world, insert_ready_full_chunk, test_world},
    };

    fn supporting_level() -> TestLevel {
        TestLevel::default().with_block(
            BlockPos::ZERO.below(),
            vanilla_blocks::STONE.default_state(),
        )
    }

    fn place_context<'a>(
        world: &'a Arc<World>,
        hit_pos: BlockPos,
        direction: Direction,
        item_in_hand: &'a mut ItemStack,
        is_secondary_use_active: bool,
    ) -> BlockPlaceContext<'a> {
        let hit_result = BlockHitResult {
            location: DVec3::ZERO,
            direction,
            block_pos: hit_pos,
            miss: false,
            inside: false,
            world_border_hit: false,
        };
        let source = PlacementSource::direct(
            None,
            types::InteractionHand::MainHand,
            item_in_hand,
            PlacementOrientation::Player {
                rotation: 0.0,
                pitch: 0.0,
            },
            is_secondary_use_active,
        );
        BlockPlaceContext::new(world, source, &hit_result)
    }

    #[test]
    fn waterlogged_candle_update_shape_schedules_water_tick() {
        init_vanilla_registry();

        let candle = CandleBlock::new(&vanilla_blocks::CANDLE);
        let state = vanilla_blocks::CANDLE
            .default_state()
            .set_value(WATERLOGGED, true);
        let level = supporting_level();

        assert_eq!(
            candle.update_shape(
                state,
                &level,
                BlockPos::ZERO,
                Direction::North,
                Direction::North.relative(BlockPos::ZERO),
                vanilla_blocks::AIR.default_state(),
            ),
            state
        );
        assert_eq!(
            level
                .scheduled_fluid_ticks
                .borrow()
                .iter()
                .map(|tick| tick.fluid)
                .collect::<Vec<_>>(),
            vec![&vanilla_fluids::WATER]
        );
    }

    #[test]
    fn burning_projectile_lights_only_unlit_candles() {
        init_vanilla_registry();

        let unlit = vanilla_blocks::CANDLE
            .default_state()
            .set_value(LIT_PROPERTY, false)
            .set_value(WATERLOGGED, false);
        let lit = unlit.set_value(LIT_PROPERTY, true);
        let waterlogged = unlit.set_value(WATERLOGGED, true);

        assert_eq!(CandleBlock::projectile_lit_state(unlit, true), Some(lit));
        assert_eq!(CandleBlock::projectile_lit_state(unlit, false), None);
        assert_eq!(CandleBlock::projectile_lit_state(lit, true), None);
        assert_eq!(CandleBlock::projectile_lit_state(waterlogged, true), None);
    }

    #[test]
    fn water_placement_on_lit_candle_emits_block_change_event() {
        init_vanilla_registry();

        let candle = CandleBlock::new(&vanilla_blocks::CANDLE);
        let state = vanilla_blocks::CANDLE
            .default_state()
            .set_value(WATERLOGGED, false)
            .set_value(LIT_PROPERTY, true);
        let level = supporting_level();

        assert!(candle.place_liquid(
            &level,
            BlockPos::ZERO,
            state,
            FluidState::source(&vanilla_fluids::WATER),
        ));

        assert_eq!(
            level
                .block_sounds
                .borrow()
                .iter()
                .map(|sound| sound.sound)
                .collect::<Vec<_>>(),
            vec![&sound_events::BLOCK_CANDLE_EXTINGUISH]
        );
        assert_eq!(
            level
                .game_events
                .borrow()
                .iter()
                .map(|event| event.event)
                .collect::<Vec<_>>(),
            vec![&vanilla_game_events::BLOCK_CHANGE]
        );
        assert!(
            level
                .last_placed_state()
                .expect("candle should be waterlogged")
                .get_value(WATERLOGGED)
        );
    }

    #[test]
    fn candle_can_be_replaced_matching_stack() {
        init_vanilla_registry();
        init_behaviors();
        let world = test_world();
        let candle = CandleBlock::new(&vanilla_blocks::CANDLE);
        let state_1 = vanilla_blocks::CANDLE.default_state();
        let state_4 = state_1.set_value(CANDLES_PROPERTY, 4);

        let mut candle_item = ItemStack::new(&vanilla_items::CANDLE);
        let ctx = place_context(
            world,
            BlockPos::ZERO,
            Direction::Up,
            &mut candle_item,
            false,
        );
        assert!(candle.can_be_replaced(state_1, &ctx));

        let mut candle_item_sneak = ItemStack::new(&vanilla_items::CANDLE);
        let ctx_sneak = place_context(
            world,
            BlockPos::ZERO,
            Direction::Up,
            &mut candle_item_sneak,
            true,
        );
        assert!(!candle.can_be_replaced(state_1, &ctx_sneak));

        let mut candle_item_full = ItemStack::new(&vanilla_items::CANDLE);
        let ctx_full = place_context(
            world,
            BlockPos::ZERO,
            Direction::Up,
            &mut candle_item_full,
            false,
        );
        assert!(!candle.can_be_replaced(state_4, &ctx_full));

        let mut diff_item = ItemStack::new(&vanilla_items::STONE);
        let ctx_diff = place_context(world, BlockPos::ZERO, Direction::Up, &mut diff_item, false);
        assert!(!candle.can_be_replaced(state_1, &ctx_diff));

        let mut other_candle = ItemStack::new(&vanilla_items::RED_CANDLE);
        let ctx_other = place_context(
            world,
            BlockPos::ZERO,
            Direction::Up,
            &mut other_candle,
            false,
        );
        assert!(!candle.can_be_replaced(state_1, &ctx_other));
    }

    #[test]
    fn candle_placement_state_increments_and_preserves_properties() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("candle_placement_increments");
        let candle = CandleBlock::new(&vanilla_blocks::CANDLE);

        let pos = BlockPos::new(0, 10, 0);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        let existing = vanilla_blocks::CANDLE
            .default_state()
            .set_value(CANDLES_PROPERTY, 2)
            .set_value(LIT_PROPERTY, true)
            .set_value(WATERLOGGED, false);
        world.set_block(pos, existing, UpdateFlags::UPDATE_ALL_IMMEDIATE);

        let mut item = ItemStack::new(&vanilla_items::CANDLE);
        let ctx = place_context(&world, pos, Direction::Up, &mut item, false);
        let placed_state = candle
            .get_state_for_placement(&ctx)
            .expect("placement state should be present");
        assert_eq!(placed_state.get_value(CANDLES_PROPERTY), 3);
        assert!(placed_state.get_value(LIT_PROPERTY));
        assert!(!placed_state.get_value(WATERLOGGED));
    }

    #[test]
    fn candle_can_place_when_clicking_block_below() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("candle_click_block_below");
        let stone_pos = BlockPos::new(0, 10, 0);
        let candle_pos = stone_pos.above();

        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(stone_pos));

        world.set_block(
            stone_pos,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_ALL_IMMEDIATE,
        );
        world.set_block(
            candle_pos,
            vanilla_blocks::CANDLE.default_state(),
            UpdateFlags::UPDATE_ALL_IMMEDIATE,
        );

        let mut candle_item = ItemStack::new(&vanilla_items::CANDLE);
        let ctx = place_context(&world, stone_pos, Direction::Up, &mut candle_item, false);

        assert_eq!(ctx.hit_pos(), stone_pos);
        assert_eq!(ctx.place_pos(), candle_pos);
        assert!(!ctx.replaces_clicked_block());
        assert!(ctx.can_place());

        let candle = CandleBlock::new(&vanilla_blocks::CANDLE);
        let new_state = candle
            .get_state_for_placement(&ctx)
            .expect("placement state should be present");
        assert_eq!(new_state.get_value(CANDLES_PROPERTY), 2);
    }
}
