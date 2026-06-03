use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::{BlockRef, shapes::VoxelShape};
use steel_utils::{BlockLocalAabb, BlockPos, BlockStateId};

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext},
    entity::{Entity, InsideBlockEffectCollector, InsideBlockEffectType},
    world::{LevelReader, World},
};

const CAULDRON_FILLED_ENTITY_INSIDE_BOXES: &[BlockLocalAabb] = &[
    BlockLocalAabb::new(0.0, 0.0, 0.0, 0.1875, 0.5625, 0.1875),
    BlockLocalAabb::new(0.8125, 0.0, 0.0, 1.0, 0.5625, 0.1875),
    BlockLocalAabb::new(0.0, 0.1875, 0.1875, 1.0, 0.5625, 1.0),
    BlockLocalAabb::new(0.1875, 0.1875, 0.0, 0.8125, 0.5625, 0.1875),
    BlockLocalAabb::new(0.0, 0.0, 0.8125, 0.1875, 0.5625, 1.0),
    BlockLocalAabb::new(0.8125, 0.0, 0.8125, 1.0, 0.5625, 1.0),
    BlockLocalAabb::new(0.0, 0.1875, 0.0, 1.0, 0.5625, 0.8125),
    BlockLocalAabb::new(0.1875, 0.1875, 0.8125, 0.8125, 0.5625, 1.0),
    BlockLocalAabb::new(0.125, 0.25, 0.125, 0.875, 0.9375, 0.875),
];
const CAULDRON_FILLED_ENTITY_INSIDE_SHAPE: VoxelShape =
    VoxelShape::from_boxes(CAULDRON_FILLED_ENTITY_INSIDE_BOXES);

/// Behavior for lava cauldrons.
///
/// TODO: Add shared cauldron interaction, drip-fill, and comparator behavior with the cauldron family.
#[block_behavior]
pub struct LavaCauldronBlock {
    block: BlockRef,
}

impl LavaCauldronBlock {
    /// Creates a lava cauldron block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for LavaCauldronBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn get_entity_inside_collision_shape(
        &self,
        _state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _entity: &dyn Entity,
    ) -> VoxelShape {
        CAULDRON_FILLED_ENTITY_INSIDE_SHAPE
    }

    fn entity_inside(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _pos: BlockPos,
        _entity: &dyn Entity,
        effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        effect_collector.apply(InsideBlockEffectType::ClearFreeze);
        effect_collector.apply(InsideBlockEffectType::LavaIgnite);
        effect_collector.run_after(
            InsideBlockEffectType::LavaIgnite,
            Box::new(|entity| entity.lava_hurt()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_f64_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= f64::EPSILON,
            "expected {actual} to equal {expected}"
        );
    }

    #[test]
    fn filled_entity_inside_shape_includes_lava_column() {
        let Some(bounds) = CAULDRON_FILLED_ENTITY_INSIDE_SHAPE.bounds() else {
            panic!("lava cauldron entity-inside shape is non-empty");
        };

        assert_f64_close(bounds.min_x(), 0.0);
        assert_f64_close(bounds.min_y(), 0.0);
        assert_f64_close(bounds.min_z(), 0.0);
        assert_f64_close(bounds.max_x(), 1.0);
        assert_f64_close(bounds.max_y(), 0.9375);
        assert_f64_close(bounds.max_z(), 1.0);
        assert_eq!(CAULDRON_FILLED_ENTITY_INSIDE_SHAPE.len(), 9);
    }
}
