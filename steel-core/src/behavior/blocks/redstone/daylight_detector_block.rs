//! Vanilla daylight-detector signal calculation and inversion.

use std::f32::consts::{PI, TAU};
use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_math::trig;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::{vanilla_block_entity_types, vanilla_game_events};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{
    BlockBehavior, BlockEntityCreation, BlockHitResult, BlockPlaceContext, InteractionResult,
    InventoryAccess,
};
use crate::block_entity::{BlockEntityTicker, entities::DaylightDetectorBlockEntity};
use crate::player::Player;
use crate::world::game_event_context::GameEventContext;
use crate::world::{LevelReader, SignalQueryContext, World};

// `(float) (Math.PI / 180.0)` in vanilla.
const DEGREES_TO_RADIANS: f32 = 0.017_453_292;

/// Vanilla `DaylightDetectorBlock` behavior.
#[block_behavior]
pub struct DaylightDetectorBlock {
    block: BlockRef,
}

impl DaylightDetectorBlock {
    /// Creates daylight-detector behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    pub(crate) fn signal_strength(world: &World, pos: BlockPos, state: BlockStateId) -> u8 {
        let sky_brightness = world.effective_sky_brightness(pos);
        Self::calculate_signal_strength(
            sky_brightness,
            world.sun_angle_degrees(),
            state.get_value(&BlockStateProperties::INVERTED),
        )
    }

    fn calculate_signal_strength(sky_brightness: u8, sun_angle_degrees: f32, inverted: bool) -> u8 {
        if inverted {
            return 15 - sky_brightness;
        }
        if sky_brightness == 0 {
            return 0;
        }

        let mut sun_angle = sun_angle_degrees * DEGREES_TO_RADIANS;
        let offset = if sun_angle < PI { 0.0 } else { TAU };
        sun_angle += (offset - sun_angle) * 0.2;
        java_round(f32::from(sky_brightness) * trig::cos(f64::from(sun_angle))).clamp(0, 15) as u8
    }

    fn update_signal_strength(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        let target = Self::signal_strength(world, pos, state);
        if state.get_value(&BlockStateProperties::POWER) != target {
            world.set_block(
                pos,
                state.set_value(&BlockStateProperties::POWER, target),
                UpdateFlags::UPDATE_ALL,
            );
        }
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "daylight detector input is bounded to [-15, 15] before Java Math.round"
)]
fn java_round(value: f32) -> i32 {
    (value + 0.5).floor() as i32
}

impl BlockBehavior for DaylightDetectorBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
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
        if !player.abilities.lock().may_build {
            return InteractionResult::Pass;
        }

        let new_state = state.set_value(
            &BlockStateProperties::INVERTED,
            !state.get_value(&BlockStateProperties::INVERTED),
        );
        world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
        world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            pos,
            &GameEventContext::new(Some(player), Some(new_state)),
        );
        Self::update_signal_strength(world, pos, new_state);
        InteractionResult::Success
    }

    fn is_signal_source(&self, _state: BlockStateId, _context: SignalQueryContext) -> bool {
        true
    }

    fn get_own_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _context: SignalQueryContext,
    ) -> i32 {
        i32::from(state.get_value(&BlockStateProperties::POWER))
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::Created(Arc::new(DaylightDetectorBlockEntity::new(
            level, pos, state,
        )))
    }

    fn get_block_entity_ticker(
        &self,
        world: &Arc<World>,
        _state: BlockStateId,
        block_entity_type: BlockEntityTypeRef,
    ) -> Option<BlockEntityTicker> {
        if !world.dimension_type.has_skylight {
            return None;
        }
        BlockEntityTicker::for_matching_entity_tick(
            block_entity_type,
            &vanilla_block_entity_types::DAYLIGHT_DETECTOR,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use steel_registry::test_support::init_test_registry;
    use steel_registry::{vanilla_block_entity_types, vanilla_blocks, vanilla_world_clocks};

    use super::*;
    use crate::test_support::fresh_test_world;

    #[test]
    fn daylight_detector_selects_vanilla_server_ticker_in_skylight_dimensions() {
        init_test_registry();
        let world = fresh_test_world("daylight_detector_block_entity");
        let state = vanilla_blocks::DAYLIGHT_DETECTOR.default_state();
        let behavior = DaylightDetectorBlock::new(&vanilla_blocks::DAYLIGHT_DETECTOR);
        let entity = behavior
            .new_block_entity(Arc::downgrade(&world), BlockPos::ZERO, state)
            .into_created()
            .expect("daylight detector should create block entity");
        assert_eq!(
            entity.get_type(),
            &vanilla_block_entity_types::DAYLIGHT_DETECTOR
        );
        assert_eq!(entity.get_block_state(), state);
        assert!(
            behavior
                .get_block_entity_ticker(
                    &world,
                    state,
                    &vanilla_block_entity_types::DAYLIGHT_DETECTOR,
                )
                .is_some()
        );
        assert!(
            behavior
                .get_block_entity_ticker(&world, state, &vanilla_block_entity_types::CHEST)
                .is_none()
        );
    }

    #[test]
    fn java_round_matches_vanilla_half_toward_positive_infinity() {
        assert_eq!(java_round(0.5), 1);
        assert_eq!(java_round(-0.5), 0);
    }

    #[test]
    fn signal_strength_matches_vanilla_sun_angle_adjustment() {
        assert_eq!(
            DaylightDetectorBlock::calculate_signal_strength(15, 0.0, false),
            15
        );
        assert_eq!(
            DaylightDetectorBlock::calculate_signal_strength(15, 77.625_66, false),
            7
        );
        assert_eq!(
            DaylightDetectorBlock::calculate_signal_strength(15, 180.0, false),
            0
        );
        assert_eq!(
            DaylightDetectorBlock::calculate_signal_strength(4, 180.0, true),
            11
        );
    }

    #[test]
    fn vanilla_trig_table_controls_overworld_rounding_boundary() {
        init_test_registry();
        let world = fresh_test_world("daylight_detector_trig_boundary");
        assert_eq!(
            world
                .level_data
                .write()
                .world_clocks_mut()
                .set_total_ticks(&vanilla_world_clocks::OVERWORLD, 680),
            Some(())
        );
        let sun_angle_degrees = world.sun_angle_degrees();

        let mut adjusted_angle = sun_angle_degrees * DEGREES_TO_RADIANS;
        let offset = if adjusted_angle < PI { 0.0 } else { TAU };
        adjusted_angle += (offset - adjusted_angle) * 0.2;
        assert_eq!(java_round(11.0 * adjusted_angle.cos()), 7);
        assert_eq!(
            DaylightDetectorBlock::calculate_signal_strength(11, sun_angle_degrees, false),
            6
        );
    }
}
