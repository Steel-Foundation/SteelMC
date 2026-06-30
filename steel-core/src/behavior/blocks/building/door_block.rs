//! Door block behavior implementation.
//!
//! Doors keep their upper and lower halves synchronized through vanilla
//! neighbor-shape updates. Redstone signal queries are isolated in
//! `has_neighbor_signal` until Steel has a redstone power graph.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt as _,
        properties::{BlockStateProperties, Direction, DoorHingeSide, DoubleBlockHalf},
        shapes,
    },
    sound_event::SoundEventRef,
    vanilla_blocks, vanilla_game_events,
};
use steel_utils::{
    BlockPos, BlockStateId,
    axis::Axis,
    types::{InteractionHand, UpdateFlags},
};

use super::weathering_block::{WeatherState, WeatheringCopper};
use crate::{
    behavior::{
        BlockBehavior, BlockHitResult, BlockPlaceContext, BlockStateBehaviorExt, InteractionResult,
        InventoryAccess,
    },
    entity::Entity,
    entity::ai::path::PathComputationType,
    fluid::fluid_state_to_block,
    player::Player,
    world::{LevelReader, ScheduledTickAccess, World, game_event_context::GameEventContext},
};

/// Behavior for vanilla door blocks.
#[block_behavior]
pub struct DoorBlock {
    block: BlockRef,
    #[json_arg(value, json = "type_can_open_by_hand")]
    can_open_by_hand: bool,
    #[json_arg(sound_events, json = "type_door_open")]
    sound_open: SoundEventRef,
    #[json_arg(sound_events, json = "type_door_close")]
    sound_close: SoundEventRef,
}

impl DoorBlock {
    const USE_UPDATE_FLAGS: UpdateFlags =
        UpdateFlags::UPDATE_CLIENTS.union(UpdateFlags::UPDATE_IMMEDIATE);

    /// Creates a new door block behavior.
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

    fn is_door(state: BlockStateId) -> bool {
        state
            .try_get_value(&BlockStateProperties::DOOR_HINGE)
            .is_some()
            && state
                .try_get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF)
                .is_some()
    }

    fn is_lower_door(state: BlockStateId) -> bool {
        Self::is_door(state)
            && state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF) == DoubleBlockHalf::Lower
    }

    fn hinge_for_placement(context: &BlockPlaceContext<'_>) -> DoorHingeSide {
        let pos = context.relative_pos;
        let above_pos = pos.above();
        let place_direction = context.horizontal_direction;

        let left_direction = place_direction.rotate_y_counter_clockwise();
        let left_pos = left_direction.relative(pos);
        let left_state = context.world.get_block_state(left_pos);
        let left_above_pos = left_direction.relative(above_pos);
        let left_above_state = context.world.get_block_state(left_above_pos);

        let right_direction = place_direction.rotate_y_clockwise();
        let right_pos = right_direction.relative(pos);
        let right_state = context.world.get_block_state(right_pos);
        let right_above_pos = right_direction.relative(above_pos);
        let right_above_state = context.world.get_block_state(right_above_pos);

        let solid_block_balance = i32::from(shapes::is_offset_shape_full_block(
            right_state.get_collision_shape_at(right_pos),
        )) + i32::from(shapes::is_offset_shape_full_block(
            right_above_state.get_collision_shape_at(right_above_pos),
        )) - i32::from(shapes::is_offset_shape_full_block(
            left_state.get_collision_shape_at(left_pos),
        )) - i32::from(shapes::is_offset_shape_full_block(
            left_above_state.get_collision_shape_at(left_above_pos),
        ));

        let door_left = Self::is_lower_door(left_state);
        let door_right = Self::is_lower_door(right_state);

        if (!door_left || door_right) && solid_block_balance <= 0 {
            if (!door_right || door_left) && solid_block_balance >= 0 {
                let (step_x, step_z) = place_direction.offset_xz();
                let click_x = context.click_location.x - f64::from(pos.x());
                let click_z = context.click_location.z - f64::from(pos.z());

                if (step_x >= 0 || click_z >= 0.5)
                    && (step_x <= 0 || click_z <= 0.5)
                    && (step_z >= 0 || click_x <= 0.5)
                    && (step_z <= 0 || click_x >= 0.5)
                {
                    DoorHingeSide::Left
                } else {
                    DoorHingeSide::Right
                }
            } else {
                DoorHingeSide::Left
            }
        } else {
            DoorHingeSide::Right
        }
    }

    const fn has_neighbor_signal<L: LevelReader + ?Sized>(_world: &L, _pos: BlockPos) -> bool {
        // TODO: Query redstone neighbor signal once Steel has redstone power propagation.
        false
    }

    fn has_correct_tool_for_drops(player: &Player, state: BlockStateId) -> bool {
        let inv = player.inventory.lock();
        let main_hand = inv.get_item_in_hand(InteractionHand::MainHand);
        main_hand.is_correct_tool_for_drops(state)
            || !state.get_block().config.requires_correct_tool_for_drops
    }

    fn prevent_drop_from_bottom_part(
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        player: &Player,
    ) {
        if state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF) != DoubleBlockHalf::Upper {
            return;
        }

        let bottom_pos = pos.below();
        let bottom_state = world.get_block_state(bottom_pos);
        if bottom_state.get_block() != state.get_block()
            || bottom_state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF)
                != DoubleBlockHalf::Lower
        {
            return;
        }

        let replacement = fluid_state_to_block(bottom_state.get_fluid_state());
        world.set_block(
            bottom_pos,
            replacement,
            UpdateFlags::UPDATE_ALL | UpdateFlags::UPDATE_SUPPRESS_DROPS,
        );
        world.destroy_block_effect(bottom_pos, u32::from(bottom_state.0), Some(player.id()));
    }

    fn play_sound(&self, world: &Arc<World>, pos: BlockPos, open: bool, exclude: Option<i32>) {
        let sound = if open {
            self.sound_open
        } else {
            self.sound_close
        };
        world.play_block_sound(sound, pos, 1.0, 1.0, exclude);
    }
}

impl BlockBehavior for DoorBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let pos = context.relative_pos;
        if pos.y() >= context.world.max_y_exclusive() - 1 {
            return None;
        }
        if !context.world.get_block_state(pos.above()).is_replaceable() {
            return None;
        }

        let powered = Self::has_neighbor_signal(context.world, pos)
            || Self::has_neighbor_signal(context.world, pos.above());
        Some(
            self.block
                .default_state()
                .set_value(
                    &BlockStateProperties::HORIZONTAL_FACING,
                    context.horizontal_direction,
                )
                .set_value(
                    &BlockStateProperties::DOOR_HINGE,
                    Self::hinge_for_placement(context),
                )
                .set_value(&BlockStateProperties::POWERED, powered)
                .set_value(&BlockStateProperties::OPEN, powered)
                .set_value(
                    &BlockStateProperties::DOUBLE_BLOCK_HALF,
                    DoubleBlockHalf::Lower,
                ),
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
        let half = state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF);
        if direction.get_axis() == Axis::Y
            && (half == DoubleBlockHalf::Lower) == (direction == Direction::Up)
        {
            if Self::is_door(neighbor_state)
                && neighbor_state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF) != half
            {
                return neighbor_state.set_value(&BlockStateProperties::DOUBLE_BLOCK_HALF, half);
            }
            return vanilla_blocks::AIR.default_state();
        }

        if half == DoubleBlockHalf::Lower
            && direction == Direction::Down
            && !self.can_survive(state, world, pos)
        {
            return vanilla_blocks::AIR.default_state();
        }

        state
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        let below_state = world.get_block_state(below_pos);
        if state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF) == DoubleBlockHalf::Lower {
            below_state.is_face_sturdy_at(below_pos, Direction::Up)
        } else {
            below_state.get_block() == self.block
        }
    }

    fn is_pathfindable(&self, state: BlockStateId, computation_type: PathComputationType) -> bool {
        match computation_type {
            PathComputationType::Land | PathComputationType::Air => {
                state.get_value(&BlockStateProperties::OPEN)
            }
            PathComputationType::Water => false,
        }
    }

    fn is_wooden_door(&self, state: BlockStateId) -> bool {
        self.can_open_by_hand && Self::is_door(state)
    }

    fn set_door_open(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        source_entity: Option<&dyn Entity>,
        open: bool,
    ) -> bool {
        if !Self::is_door(state) || state.get_value(&BlockStateProperties::OPEN) == open {
            return false;
        }

        let new_state = state.set_value(&BlockStateProperties::OPEN, open);
        if !world.set_block(pos, new_state, Self::USE_UPDATE_FLAGS) {
            return false;
        }

        self.play_sound(world, pos, open, source_entity.map(Entity::id));
        let event = if open {
            &vanilla_game_events::BLOCK_OPEN
        } else {
            &vanilla_game_events::BLOCK_CLOSE
        };
        world.game_event(event, pos, &GameEventContext::new(source_entity, None));
        true
    }

    fn set_placed_by(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _player: Option<&Player>,
        _inv: &InventoryAccess,
    ) {
        world.set_block(
            pos.above(),
            state.set_value(
                &BlockStateProperties::DOUBLE_BLOCK_HALF,
                DoubleBlockHalf::Upper,
            ),
            UpdateFlags::UPDATE_ALL,
        );
    }

    fn player_will_destroy(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
    ) -> BlockStateId {
        if player.has_infinite_materials() || !Self::has_correct_tool_for_drops(player, state) {
            Self::prevent_drop_from_bottom_part(world, pos, state, player);
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
        if !self.can_open_by_hand {
            return InteractionResult::Pass;
        }

        let open = !state.get_value(&BlockStateProperties::OPEN);
        let new_state = state.set_value(&BlockStateProperties::OPEN, open);
        world.set_block(pos, new_state, Self::USE_UPDATE_FLAGS);
        self.play_sound(world, pos, open, Some(player.id()));
        let event = if open {
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
        source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        if source_block == self.block {
            return;
        }

        let half = state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF);
        let other_half_pos = if half == DoubleBlockHalf::Lower {
            pos.above()
        } else {
            pos.below()
        };
        let signal = Self::has_neighbor_signal(world, pos)
            || Self::has_neighbor_signal(world, other_half_pos);
        if signal == state.get_value(&BlockStateProperties::POWERED) {
            return;
        }

        if signal != state.get_value(&BlockStateProperties::OPEN) {
            self.play_sound(world, pos, signal, None);
            let event = if signal {
                &vanilla_game_events::BLOCK_OPEN
            } else {
                &vanilla_game_events::BLOCK_CLOSE
            };
            world.game_event(event, pos, &GameEventContext::default());
        }

        let new_state = state
            .set_value(&BlockStateProperties::POWERED, signal)
            .set_value(&BlockStateProperties::OPEN, signal);
        world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
    }
}

/// Weathering copper doors share door behavior and add copper aging.
#[block_behavior]
pub struct WeatheringCopperDoorBlock {
    block: BlockRef,
    #[json_arg(r#enum = "WeatherState", json = "weather_state")]
    weathering: WeatheringCopper,
    #[json_arg(value, json = "type_can_open_by_hand")]
    can_open_by_hand: bool,
    #[json_arg(sound_events, json = "type_door_open")]
    sound_open: SoundEventRef,
    #[json_arg(sound_events, json = "type_door_close")]
    sound_close: SoundEventRef,
}

impl WeatheringCopperDoorBlock {
    /// Creates a new weathering copper door behavior.
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

    const fn door(&self) -> DoorBlock {
        DoorBlock::new(
            self.block,
            self.can_open_by_hand,
            self.sound_open,
            self.sound_close,
        )
    }
}

impl BlockBehavior for WeatheringCopperDoorBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.door().get_state_for_placement(context)
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
        self.door()
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        self.door().can_survive(state, world, pos)
    }

    fn is_pathfindable(&self, state: BlockStateId, computation_type: PathComputationType) -> bool {
        self.door().is_pathfindable(state, computation_type)
    }

    fn is_wooden_door(&self, state: BlockStateId) -> bool {
        self.door().is_wooden_door(state)
    }

    fn set_door_open(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        source_entity: Option<&dyn Entity>,
        open: bool,
    ) -> bool {
        self.door()
            .set_door_open(state, world, pos, source_entity, open)
    }

    fn set_placed_by(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: Option<&Player>,
        inv: &InventoryAccess,
    ) {
        self.door().set_placed_by(state, world, pos, player, inv);
    }

    fn player_will_destroy(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
    ) -> BlockStateId {
        self.door().player_will_destroy(state, world, pos, player)
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
        self.door()
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
        self.door()
            .handle_neighbor_changed(state, world, pos, source_block, moved_by_piston);
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        self.weathering.is_randomly_ticking()
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if state.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF) == DoubleBlockHalf::Lower {
            self.weathering.change_over_time(state, world, pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{sound_events, test_support::init_test_registry, vanilla_blocks};
    use steel_utils::BlockPos;

    use crate::test_support::TestLevel;

    use super::*;

    #[test]
    fn lower_half_copies_transformed_upper_half_state() {
        init_test_registry();
        let behavior = DoorBlock::new(
            &vanilla_blocks::SPRUCE_DOOR,
            true,
            &sound_events::BLOCK_WOODEN_DOOR_OPEN,
            &sound_events::BLOCK_WOODEN_DOOR_CLOSE,
        );
        let lower = vanilla_blocks::SPRUCE_DOOR
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, Direction::West)
            .set_value(&BlockStateProperties::DOOR_HINGE, DoorHingeSide::Right)
            .set_value(
                &BlockStateProperties::DOUBLE_BLOCK_HALF,
                DoubleBlockHalf::Lower,
            )
            .set_value(&BlockStateProperties::OPEN, false)
            .set_value(&BlockStateProperties::POWERED, false);
        let upper = vanilla_blocks::SPRUCE_DOOR
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, Direction::South)
            .set_value(&BlockStateProperties::DOOR_HINGE, DoorHingeSide::Left)
            .set_value(
                &BlockStateProperties::DOUBLE_BLOCK_HALF,
                DoubleBlockHalf::Upper,
            )
            .set_value(&BlockStateProperties::OPEN, false)
            .set_value(&BlockStateProperties::POWERED, false);
        let level = TestLevel::default();

        let updated = behavior.update_shape(
            lower,
            &level,
            BlockPos::ZERO,
            Direction::Up,
            BlockPos::ZERO.above(),
            upper,
        );

        assert_eq!(
            updated.get_value(&BlockStateProperties::DOUBLE_BLOCK_HALF),
            DoubleBlockHalf::Lower
        );
        assert_eq!(
            updated.get_value(&BlockStateProperties::HORIZONTAL_FACING),
            Direction::South
        );
        assert_eq!(
            updated.get_value(&BlockStateProperties::DOOR_HINGE),
            DoorHingeSide::Left
        );
    }

    #[test]
    fn door_wooden_query_uses_can_open_by_hand_like_vanilla() {
        init_test_registry();
        let oak = DoorBlock::new(
            &vanilla_blocks::OAK_DOOR,
            true,
            &sound_events::BLOCK_WOODEN_DOOR_OPEN,
            &sound_events::BLOCK_WOODEN_DOOR_CLOSE,
        );
        let iron = DoorBlock::new(
            &vanilla_blocks::IRON_DOOR,
            false,
            &sound_events::BLOCK_IRON_DOOR_OPEN,
            &sound_events::BLOCK_IRON_DOOR_CLOSE,
        );

        assert!(oak.is_wooden_door(vanilla_blocks::OAK_DOOR.default_state()));
        assert!(!iron.is_wooden_door(vanilla_blocks::IRON_DOOR.default_state()));
        assert!(!oak.is_wooden_door(vanilla_blocks::STONE.default_state()));
    }
}
