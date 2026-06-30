use std::sync::Arc;

use glam::DVec3;
use steel_macros::block_behavior;
use steel_registry::blocks::{BlockRef, block_state_ext::BlockStateExt as _, shapes::VoxelShape};
use steel_registry::{sound_events, vanilla_damage_types, vanilla_entities};
use steel_utils::entity_events::EntityStatus;
use steel_utils::{BlockLocalAabb, BlockPos, BlockStateId};

use crate::{
    behavior::{
        BlockBehavior, BlockCollisionContext, BlockPlaceContext, EntityFallDamage,
        EntityFallOnContext,
    },
    entity::{Entity, InsideBlockEffectCollector, damage::DamageSource},
    world::{LevelReader, World},
};

const SLIDE_STARTS_WHEN_VERTICAL_SPEED_IS_AT_LEAST: f64 = 0.13;
const MIN_FALL_SPEED_TO_BE_CONSIDERED_SLIDING: f64 = 0.08;
const THROTTLE_SLIDE_SPEED_TO: f64 = 0.05;
const HONEY_BLOCK_TOP: f64 = 0.9375;
const HONEY_EDGE_OVERLAP_BASE: f64 = 0.4375;
const SLIDE_EPSILON: f64 = 1.0e-7;
const VANILLA_ENTITY_DRAG: f64 = 0.980_000_019_073_486_3;
const SHAPE_BOXES: &[BlockLocalAabb] = &[BlockLocalAabb::new(
    0.0625, 0.0, 0.0625, 0.9375, 0.9375, 0.9375,
)];
const SHAPE: VoxelShape = VoxelShape::from_boxes(SHAPE_BOXES);

/// Behavior for honey blocks.
#[block_behavior]
pub struct HoneyBlock {
    block: BlockRef,
}

impl HoneyBlock {
    /// Creates a honey block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    #[must_use]
    fn old_delta_y(delta_y: f64) -> f64 {
        delta_y / VANILLA_ENTITY_DRAG + 0.08
    }

    #[must_use]
    fn new_delta_y(delta_y: f64) -> f64 {
        (delta_y - 0.08) * VANILLA_ENTITY_DRAG
    }

    #[must_use]
    fn is_sliding_down_at(
        pos: BlockPos,
        entity_position: DVec3,
        velocity: DVec3,
        bounding_box_width: f64,
        on_ground: bool,
    ) -> bool {
        if on_ground {
            return false;
        }
        if entity_position.y > f64::from(pos.y()) + HONEY_BLOCK_TOP - SLIDE_EPSILON {
            return false;
        }
        if Self::old_delta_y(velocity.y) >= -MIN_FALL_SPEED_TO_BE_CONSIDERED_SLIDING {
            return false;
        }

        let dx = (f64::from(pos.x()) + 0.5 - entity_position.x).abs();
        let dz = (f64::from(pos.z()) + 0.5 - entity_position.z).abs();
        let overlap_distance = HONEY_EDGE_OVERLAP_BASE + bounding_box_width / 2.0;
        dx + SLIDE_EPSILON > overlap_distance || dz + SLIDE_EPSILON > overlap_distance
    }

    #[must_use]
    fn is_sliding_down(pos: BlockPos, entity: &dyn Entity) -> bool {
        Self::is_sliding_down_at(
            pos,
            entity.position(),
            entity.velocity(),
            entity.bounding_box().width(),
            entity.on_ground(),
        )
    }

    #[must_use]
    fn velocity_after_slide(velocity: DVec3) -> DVec3 {
        let old_delta_y = Self::old_delta_y(velocity.y);
        let y = Self::new_delta_y(-THROTTLE_SLIDE_SPEED_TO);
        if old_delta_y < -SLIDE_STARTS_WHEN_VERTICAL_SPEED_IS_AT_LEAST {
            let horizontal_reduction_factor = -THROTTLE_SLIDE_SPEED_TO / old_delta_y;
            return DVec3::new(
                velocity.x * horizontal_reduction_factor,
                y,
                velocity.z * horizontal_reduction_factor,
            );
        }

        DVec3::new(velocity.x, y, velocity.z)
    }

    #[must_use]
    fn does_entity_do_slide_effects(entity: &dyn Entity) -> bool {
        if entity.is_living_entity()
            || entity.entity_type().is_abstract_boat
            || entity.entity_type().is_abstract_minecart
        {
            return true;
        }

        entity.entity_type() == &vanilla_entities::TNT
    }

    fn do_slide_movement(entity: &dyn Entity) {
        entity.set_velocity(Self::velocity_after_slide(entity.velocity()));
        entity.reset_fall_distance();
    }

    fn maybe_do_slide_effects(_world: &World, entity: &dyn Entity) {
        if !Self::does_entity_do_slide_effects(entity) {
            return;
        }

        let play_sound = rand::random_range(0..5) == 0;
        let broadcast_particles = rand::random_range(0..5) == 0;
        if play_sound {
            entity.play_sound(&sound_events::BLOCK_HONEY_BLOCK_SLIDE, 1.0, 1.0);
        }
        if broadcast_particles {
            entity.broadcast_entity_event(EntityStatus::HoneySlide);
        }
    }
}

impl BlockBehavior for HoneyBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn get_collision_shape(
        &self,
        _state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _context: BlockCollisionContext,
    ) -> VoxelShape {
        SHAPE
    }

    fn fall_on(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        context: EntityFallOnContext<'_>,
    ) -> Option<EntityFallDamage> {
        if let Some(entity) = context.source_entity() {
            entity.play_sound(&sound_events::BLOCK_HONEY_BLOCK_SLIDE, 1.0, 1.0);
            entity.broadcast_entity_event(EntityStatus::HoneyJump);
        }

        Some(EntityFallDamage::new(
            context.fall_distance,
            0.2,
            DamageSource::environment(&vanilla_damage_types::FALL),
        ))
    }

    fn after_fall_on_damage(
        &self,
        state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        entity: &dyn Entity,
        _fall_damage: &EntityFallDamage,
        damage_applied: bool,
    ) {
        if damage_applied {
            let sound_type = state.get_block().config.sound_type;
            entity.play_sound(
                sound_type.fall_sound,
                sound_type.volume * 0.5,
                sound_type.pitch * 0.75,
            );
        }
    }

    fn entity_inside(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
        effect_collector: &mut InsideBlockEffectCollector,
        is_precise: bool,
    ) {
        if Self::is_sliding_down(pos, entity) {
            // TODO: Award the honey-block slide advancement once advancements exist.
            Self::do_slide_movement(entity);
            Self::maybe_do_slide_effects(world, entity);
        }

        self.default_entity_inside(state, world, pos, entity, effect_collector, is_precise);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos() -> BlockPos {
        BlockPos::new(10, 64, 20)
    }

    #[test]
    fn slide_requires_entity_to_be_airborne() {
        assert!(!HoneyBlock::is_sliding_down_at(
            pos(),
            DVec3::new(10.95, 64.5, 20.5),
            DVec3::new(0.0, -0.3, 0.0),
            0.6,
            true,
        ));
    }

    #[test]
    fn slide_requires_entity_below_honey_top() {
        assert!(!HoneyBlock::is_sliding_down_at(
            pos(),
            DVec3::new(10.95, 64.9375, 20.5),
            DVec3::new(0.0, -0.3, 0.0),
            0.6,
            false,
        ));
    }

    #[test]
    fn slide_requires_fast_enough_downward_motion() {
        assert!(!HoneyBlock::is_sliding_down_at(
            pos(),
            DVec3::new(10.95, 64.5, 20.5),
            DVec3::new(0.0, -0.1, 0.0),
            0.6,
            false,
        ));
    }

    #[test]
    fn slide_requires_entity_overlapping_honey_edge() {
        assert!(!HoneyBlock::is_sliding_down_at(
            pos(),
            DVec3::new(10.5, 64.5, 20.5),
            DVec3::new(0.0, -0.3, 0.0),
            0.6,
            false,
        ));
        assert!(HoneyBlock::is_sliding_down_at(
            pos(),
            DVec3::new(11.25, 64.5, 20.5),
            DVec3::new(0.0, -0.3, 0.0),
            0.6,
            false,
        ));
    }

    #[test]
    fn slide_throttles_fast_fall_horizontal_velocity() {
        let velocity = HoneyBlock::velocity_after_slide(DVec3::new(0.8, -0.3, -0.4));
        let old_delta_y = HoneyBlock::old_delta_y(-0.3);
        let scale = -0.05 / old_delta_y;

        assert_eq!(
            velocity,
            DVec3::new(0.8 * scale, HoneyBlock::new_delta_y(-0.05), -0.4 * scale,)
        );
    }

    #[test]
    fn slide_preserves_horizontal_velocity_when_fall_is_not_fast() {
        let velocity = HoneyBlock::velocity_after_slide(DVec3::new(0.8, -0.18, -0.4));

        assert_eq!(
            velocity,
            DVec3::new(0.8, HoneyBlock::new_delta_y(-0.05), -0.4)
        );
    }
}
