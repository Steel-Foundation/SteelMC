//! Trapdoor block behavior implementation.
//!
//! Redstone signal queries are isolated in `has_neighbor_signal`
//! until Steel has a redstone power graph.

use crate::{
    behavior::{
        BlockBehavior, BlockHitResult, BlockPlaceContext, InteractionResult, InventoryAccess,
        blocks::{WeatherState, WeatheringCopper},
    },
    entity::Entity,
    entity::ai::path::PathComputationType,
    player::Player,
    world::{LevelReader, ScheduledTickAccess, World, game_event_context::GameEventContext},
};
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt as _,
        properties::{BlockStateProperties, BoolProperty, Direction, EnumProperty, Half},
    },
    sound_event::SoundEventRef,
    vanilla_fluids, vanilla_game_events,
};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

/// Behavior for vanilla trapdoor blocks.
#[block_behavior]
pub struct TrapDoorBlock {
    block: BlockRef,
    #[json_arg(value, json = "type_can_open_by_hand")]
    can_open_by_hand: bool,
    #[json_arg(sound_events, json = "type_trapdoor_open")]
    sound_open: SoundEventRef,
    #[json_arg(sound_events, json = "type_trapdoor_close")]
    sound_close: SoundEventRef,
}
/// Behavior for vanilla copper trapdoor blocks.
#[block_behavior]
pub struct WeatheringCopperTrapDoorBlock {
    block: BlockRef,
    /// Weathering state of the block
    #[json_arg(r#enum = "WeatherState", json = "weather_state")]
    pub weathering: WeatheringCopper,
    #[json_arg(value, json = "type_can_open_by_hand")]
    can_open_by_hand: bool,
    #[json_arg(sound_events, json = "type_trapdoor_open")]
    sound_open: SoundEventRef,
    #[json_arg(sound_events, json = "type_trapdoor_close")]
    sound_close: SoundEventRef,
}

const OPEN: &BoolProperty = &BlockStateProperties::OPEN;
const HALF: &EnumProperty<Half> = &BlockStateProperties::HALF;
const POWERED: &BoolProperty = &BlockStateProperties::POWERED;
const FACING: &EnumProperty<Direction> = &BlockStateProperties::FACING;
const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;

impl TrapDoorBlock {
    /// Creates a new trapdoor block behavior.
    #[must_use]
    pub const fn new(
        block: BlockRef,
        can_open_by_hand: bool,
        sound_open: SoundEventRef,
        sound_close: SoundEventRef,
    ) -> Self {
        Self {
            block,
            can_open_by_hand,
            sound_open,
            sound_close,
        }
    }

    const fn has_neighbor_signal<L: LevelReader + ?Sized>(_world: &L, _pos: BlockPos) -> bool {
        // TODO: Query redstone neighbor signal once Steel has redstone power propagation.
        false
    }

    fn play_sound(&self, player: Option<&Player>, world: &Arc<World>, pos: BlockPos, open: bool) {
        let sound = if open {
            self.sound_open
        } else {
            self.sound_close
        };
        let pitch = rand::random::<f32>() * 0.1 + 0.9;
        world.play_block_sound(sound, pos, 1.0, pitch, player.map(Entity::id));
        world.game_event(
            if open {
                &vanilla_game_events::BLOCK_OPEN
            } else {
                &vanilla_game_events::BLOCK_CLOSE
            },
            pos,
            &GameEventContext::new(
                if let Some(player) = player {
                    Some(player)
                } else {
                    None
                },
                None,
            ),
        );
    }

    fn toggle(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos, player: &Player) {
        let block_state = state.set_value(OPEN, !state.get_value(OPEN));
        world.set_block(pos, block_state, UpdateFlags::UPDATE_CLIENTS);
        if block_state.get_value(WATERLOGGED) {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }
        self.play_sound(Some(player), world, pos, block_state.get_value(OPEN));
    }
}

impl BlockBehavior for TrapDoorBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let mut state = self.block.default_state();
        let face = context.clicked_face();
        if !context.replaces_clicked_block() && face.is_horizontal() {
            state = state.set_value(FACING, face).set_value(
                HALF,
                if context.click_location().y - f64::from(context.place_pos().y()) > 0.5 {
                    Half::Top
                } else {
                    Half::Bottom
                },
            );
        } else {
            state = state
                .set_value(FACING, context.horizontal_direction().opposite())
                .set_value(
                    HALF,
                    if face == Direction::Up {
                        Half::Bottom
                    } else {
                        Half::Top
                    },
                );
        }

        if Self::has_neighbor_signal(context.world, context.place_pos()) {
            state = state.set_value(OPEN, true).set_value(POWERED, true);
        }

        Some(state.set_value(WATERLOGGED, context.is_water_source()))
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if state.get_value(WATERLOGGED) {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }
        state
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
        if self.can_open_by_hand {
            self.toggle(state, world, pos, player);
            InteractionResult::Success
        } else {
            InteractionResult::Pass
        }
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        let signal = Self::has_neighbor_signal(world, pos);
        let mut block_state = state;
        if signal != state.get_value(POWERED) && signal != state.get_value(OPEN) {
            block_state = block_state.set_value(OPEN, signal);
            self.play_sound(None, world, pos, signal);
        }
        world.set_block(
            pos,
            block_state.set_value(POWERED, signal),
            UpdateFlags::UPDATE_CLIENTS,
        );
        if state.get_value(WATERLOGGED) {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            let _ = world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }
    }

    fn is_pathfindable(&self, state: BlockStateId, computation_type: PathComputationType) -> bool {
        match computation_type {
            PathComputationType::Land | PathComputationType::Air => state.get_value(OPEN),
            PathComputationType::Water => state.get_value(WATERLOGGED),
        }
    }
}

impl WeatheringCopperTrapDoorBlock {
    /// Creates a new copper trapdoor behavior.
    #[must_use]
    pub const fn new(
        block: BlockRef,
        weather_state: WeatherState,
        can_open_by_hand: bool,
        sound_open: SoundEventRef,
        sound_close: SoundEventRef,
    ) -> Self {
        Self {
            block,
            weathering: WeatheringCopper::new(weather_state),
            can_open_by_hand,
            sound_open,
            sound_close,
        }
    }

    const fn trapdoor(&self) -> TrapDoorBlock {
        TrapDoorBlock::new(
            self.block,
            self.can_open_by_hand,
            self.sound_open,
            self.sound_close,
        )
    }
}

impl BlockBehavior for WeatheringCopperTrapDoorBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.trapdoor().get_state_for_placement(context)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        self.trapdoor()
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        self.trapdoor()
            .use_without_item(state, world, pos, player, hit_result, inv)
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        source_block: BlockRef,
        moved_by_piston: bool,
    ) {
        self.trapdoor()
            .handle_neighbor_changed(state, world, pos, source_block, moved_by_piston);
    }

    fn is_pathfindable(&self, state: BlockStateId, computation_type: PathComputationType) -> bool {
        self.trapdoor().is_pathfindable(state, computation_type)
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        self.weathering.is_randomly_ticking()
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.weathering.change_over_time(state, world, pos);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::{
        blocks::properties::BlockStateProperties, sound_events, test_support::init_test_registry,
        vanilla_blocks,
    };

    #[test]
    fn closed_trapdoor_is_not_land_or_air_pathfindable() {
        init_test_registry();
        let behavior = TrapDoorBlock::new(
            &vanilla_blocks::OAK_TRAPDOOR,
            true,
            &sound_events::BLOCK_WOODEN_TRAPDOOR_OPEN,
            &sound_events::BLOCK_WOODEN_TRAPDOOR_CLOSE,
        );
        let state = vanilla_blocks::OAK_TRAPDOOR
            .default_state()
            .set_value(&BlockStateProperties::OPEN, false)
            .set_value(&BlockStateProperties::WATERLOGGED, false);

        assert!(!behavior.is_pathfindable(state, PathComputationType::Land));
        assert!(!behavior.is_pathfindable(state, PathComputationType::Air));
        assert!(!behavior.is_pathfindable(state, PathComputationType::Water));
    }

    #[test]
    fn open_waterlogged_trapdoor_matches_vanilla_pathfinding() {
        init_test_registry();
        let behavior = TrapDoorBlock::new(
            &vanilla_blocks::OAK_TRAPDOOR,
            true,
            &sound_events::BLOCK_WOODEN_TRAPDOOR_OPEN,
            &sound_events::BLOCK_WOODEN_TRAPDOOR_CLOSE,
        );
        let state = vanilla_blocks::OAK_TRAPDOOR
            .default_state()
            .set_value(&BlockStateProperties::OPEN, true)
            .set_value(&BlockStateProperties::WATERLOGGED, true);

        assert!(behavior.is_pathfindable(state, PathComputationType::Land));
        assert!(behavior.is_pathfindable(state, PathComputationType::Air));
        assert!(behavior.is_pathfindable(state, PathComputationType::Water));
    }
}
