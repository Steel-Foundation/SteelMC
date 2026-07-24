//! Vanilla projectile-sensitive target block behavior.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_utils::axis::Axis;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::entity::Entity;
use crate::entity::projectile::Projectile;
use crate::world::{ClipHitResult, LevelReader, SignalQueryContext, World};

const ACTIVATION_TICKS_ARROWS: i32 = 20;
const ACTIVATION_TICKS_OTHER: i32 = 8;
const RESET_ON_PLACE_FLAGS: UpdateFlags =
    UpdateFlags::UPDATE_CLIENTS.union(UpdateFlags::UPDATE_KNOWN_SHAPE);

/// Vanilla `TargetBlock` behavior.
#[block_behavior]
pub struct TargetBlock {
    block: BlockRef,
}

impl TargetBlock {
    /// Creates target block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn distance_from_block_center(coordinate: f64) -> f64 {
        (coordinate - coordinate.floor() - 0.5).abs()
    }

    fn redstone_strength(hit: &ClipHitResult) -> i32 {
        let dist_x = Self::distance_from_block_center(hit.location.x);
        let dist_y = Self::distance_from_block_center(hit.location.y);
        let dist_z = Self::distance_from_block_center(hit.location.z);
        let distance = match hit.direction.axis() {
            Axis::Y => dist_x.max(dist_z),
            Axis::Z => dist_x.max(dist_y),
            Axis::X => dist_y.max(dist_z),
        };
        let centered = ((0.5 - distance) / 0.5).clamp(0.0, 1.0);
        (15.0 * centered).ceil().max(1.0) as i32
    }

    fn update_redstone_output(
        &self,
        world: &Arc<World>,
        state: BlockStateId,
        hit: &ClipHitResult,
        entity: &dyn Entity,
    ) -> i32 {
        let strength = Self::redstone_strength(hit);
        if !world.has_scheduled_block_tick(hit.block_pos, self.block) {
            world.set_block(
                hit.block_pos,
                state.set_value(&BlockStateProperties::POWER, strength as u8),
                UpdateFlags::UPDATE_ALL,
            );
            world.schedule_block_tick_default(
                hit.block_pos,
                self.block,
                if entity.is_abstract_arrow() {
                    ACTIVATION_TICKS_ARROWS
                } else {
                    ACTIVATION_TICKS_OTHER
                },
            );
        }
        strength
    }
}

impl BlockBehavior for TargetBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn on_projectile_hit(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        hit: &ClipHitResult,
        projectile: &dyn Projectile,
    ) {
        let _strength = self.update_redstone_output(world, state, hit, projectile);
        // The owner-facing target-hit stat and advancement criterion await
        // Steel's shared statistics and advancement foundations.
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if state.get_value(&BlockStateProperties::POWER) != 0 {
            world.set_block(
                pos,
                state.set_value(&BlockStateProperties::POWER, 0_u8),
                UpdateFlags::UPDATE_ALL,
            );
        }
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        if old_state.get_block() != state.get_block()
            && state.get_value(&BlockStateProperties::POWER) > 0
            && !world.has_scheduled_block_tick(pos, self.block)
        {
            world.set_block(
                pos,
                state.set_value(&BlockStateProperties::POWER, 0_u8),
                RESET_ON_PLACE_FLAGS,
            );
        }
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
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_registry::blocks::properties::Direction;
    use steel_registry::test_support::init_test_registry;

    use super::*;

    fn hit(location: DVec3, direction: Direction) -> ClipHitResult {
        ClipHitResult {
            location,
            direction,
            block_pos: BlockPos::new(-1, 64, -1),
            miss: false,
            inside: false,
            world_border_hit: false,
        }
    }

    #[test]
    fn hit_strength_uses_the_hit_face_and_floor_based_fraction() {
        init_test_registry();

        assert_eq!(
            TargetBlock::redstone_strength(&hit(DVec3::new(-0.5, 64.99, -0.5), Direction::Up,)),
            15
        );
        assert_eq!(
            TargetBlock::redstone_strength(&hit(DVec3::new(-0.01, 64.5, -0.5), Direction::Up,)),
            1
        );
        assert_eq!(
            TargetBlock::redstone_strength(&hit(DVec3::new(-0.5, 64.5, -0.25), Direction::North,)),
            15
        );
    }
}
