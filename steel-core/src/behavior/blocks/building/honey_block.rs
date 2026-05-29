//! Honey block behavior impl

use std::sync::Arc;

use glam::DVec3;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::vanilla_damage_types;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::Entity;
use crate::entity::damage::DamageSource;
use crate::world::World;

/// Behavior for the honey block
/// TODO: Implement HoneyBlock sliding sounds, particles, and advancement events
/// once entity-positioned sounds and entity event broadcasting are available
#[block_behavior]
pub struct HoneyBlock {
    block: BlockRef,
}

impl HoneyBlock {
    const SLIDE_START_Y: f64 = 0.9375 - 1.0E-7;
    const MIN_SLIDE_SPEED: f64 = 0.08;
    const FAST_SLIDE_SPEED: f64 = 0.13;
    const THROTTLED_SLIDE_SPEED: f64 = 0.05;

    /// Creates a new honey block behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn old_delta_y(delta_y: f64) -> f64 {
        delta_y / f64::from(0.98_f32) + 0.08
    }

    fn new_delta_y(delta_y: f64) -> f64 {
        (delta_y - 0.08) * f64::from(0.98_f32)
    }

    fn is_sliding_down(pos: BlockPos, entity: &dyn Entity) -> bool {
        if entity.on_ground() {
            return false;
        }

        let entity_pos = entity.position();
        if entity_pos.y > f64::from(pos.y()) + Self::SLIDE_START_Y {
            return false;
        }

        let old_delta_y = Self::old_delta_y(entity.velocity().y);
        if old_delta_y >= -Self::MIN_SLIDE_SPEED {
            return false;
        }

        let dx = (f64::from(pos.x()) + 0.5 - entity_pos.x).abs();
        let dz = (f64::from(pos.z()) + 0.5 - entity_pos.z).abs();
        let overlap_distance = 0.4375 + f64::from(entity.entity_type().dimensions.width) / 2.0;
        dx + 1.0E-7 > overlap_distance || dz + 1.0E-7 > overlap_distance
    }

    fn do_slide_movement(entity: &dyn Entity) {
        let velocity = entity.velocity();
        let old_delta_y = Self::old_delta_y(velocity.y);
        if old_delta_y < -Self::FAST_SLIDE_SPEED {
            let horizontal_scale = -Self::THROTTLED_SLIDE_SPEED / old_delta_y;
            entity.set_velocity(DVec3::new(
                velocity.x * horizontal_scale,
                Self::new_delta_y(-Self::THROTTLED_SLIDE_SPEED),
                velocity.z * horizontal_scale,
            ));
        } else {
            entity.set_velocity(DVec3::new(
                velocity.x,
                Self::new_delta_y(-Self::THROTTLED_SLIDE_SPEED),
                velocity.z,
            ));
        }

        entity.reset_fall_distance();
    }
}

impl BlockBehavior for HoneyBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn entity_inside(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
    ) {
        if Self::is_sliding_down(pos, entity) {
            Self::do_slide_movement(entity);
        }
    }

    fn fall_on(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        entity: &dyn Entity,
        fall_distance: f32,
    ) {
        entity.cause_fall_damage(
            fall_distance,
            0.2,
            &DamageSource::environment(&vanilla_damage_types::FALL),
        );
    }
}
