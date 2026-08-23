use std::sync::Arc;

use rand::Rng;
use steel_macros::block_behavior;
use steel_registry::blocks::properties::Direction;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::bonemealable::Bonemealable;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::multiface_block::MultifaceSpreader;
use super::{BlockRef, MultifaceBlock};

/// Vanilla glow lichen behavior.
#[block_behavior]
pub struct GlowLichenBlock {
    multiface: MultifaceBlock,
    spreader: MultifaceSpreader,
}

impl GlowLichenBlock {
    /// Creates a new glow lichen block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            multiface: MultifaceBlock::new(block),
            spreader: MultifaceSpreader::new(block),
        }
    }
}

impl BlockBehavior for GlowLichenBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        self.multiface
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        self.multiface.can_survive(state, world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.multiface.get_state_for_placement(context)
    }

    fn can_be_replaced(&self, state: BlockStateId, context: &BlockPlaceContext<'_>) -> bool {
        self.multiface.can_be_replaced(state, context)
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Bonemealable for GlowLichenBlock {
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        Direction::ALL.iter().any(|face| {
            self.spreader
                .can_spread_in_any_direction(state, world, pos, face.opposite())
        })
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        self.spreader
            .spread_from_random_face_toward_random_direction(state, world, pos, rng);
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::blocks::block_state_ext::BlockStateExt;
    use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
    use steel_registry::{init_vanilla_registry, vanilla_blocks};
    use steel_utils::types::UpdateFlags;

    use crate::test_support::TestLevel;

    use super::*;

    const DOWN: &BoolProperty = &BlockStateProperties::DOWN;
    const EAST: &BoolProperty = &BlockStateProperties::EAST;
    const NORTH: &BoolProperty = &BlockStateProperties::NORTH;
    const SOUTH: &BoolProperty = &BlockStateProperties::SOUTH;
    const UP: &BoolProperty = &BlockStateProperties::UP;
    const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;
    const WEST: &BoolProperty = &BlockStateProperties::WEST;

    fn north_facing_state() -> BlockStateId {
        vanilla_blocks::GLOW_LICHEN
            .default_state()
            .set_value(NORTH, true)
    }

    #[test]
    fn spreads_to_an_open_face_at_the_same_position_first() {
        init_vanilla_registry();
        let pos = BlockPos::new(0, 64, 0);
        let state = north_facing_state();
        let level = TestLevel::default()
            .with_block(pos, state)
            .with_block(pos.north(), vanilla_blocks::STONE.default_state())
            .with_block(pos.above(), vanilla_blocks::STONE.default_state());
        let spreader = MultifaceSpreader::new(&vanilla_blocks::GLOW_LICHEN);
        let behavior = GlowLichenBlock::new(&vanilla_blocks::GLOW_LICHEN);

        assert!(behavior.is_valid_bonemeal_target(state, &level, pos));

        let spread = spreader
            .spread_from_face_toward_direction(state, &level, pos, Direction::North, Direction::Up)
            .expect("lichen should spread at the source position");

        assert_eq!(spread.pos, pos);
        assert_eq!(spread.face, Direction::Up);
        assert!(level.get_block_state(pos).get_value(UP));
        assert!(level.get_block_state(pos).get_value(NORTH));
        assert_eq!(
            level.placed_blocks.borrow()[0].flags,
            UpdateFlags::UPDATE_CLIENTS
        );
    }

    #[test]
    fn spreads_in_the_same_plane_when_the_source_has_no_open_face() {
        init_vanilla_registry();
        let pos = BlockPos::new(0, 64, 0);
        let state = north_facing_state();
        let target = pos.above();
        let level = TestLevel::default()
            .with_block(pos, state)
            .with_block(pos.north(), vanilla_blocks::STONE.default_state())
            .with_block(target.north(), vanilla_blocks::STONE.default_state());
        let spreader = MultifaceSpreader::new(&vanilla_blocks::GLOW_LICHEN);

        let spread = spreader
            .spread_from_face_toward_direction(state, &level, pos, Direction::North, Direction::Up)
            .expect("lichen should spread in the same plane");

        assert_eq!(spread.pos, target);
        assert_eq!(spread.face, Direction::North);
        assert!(level.get_block_state(target).get_value(NORTH));
    }

    #[test]
    fn wraps_around_a_supporting_block_when_same_plane_is_blocked() {
        init_vanilla_registry();
        let pos = BlockPos::new(0, 64, 0);
        let state = north_facing_state();
        let target = pos.above().north();
        let level = TestLevel::default()
            .with_block(pos, state)
            .with_block(pos.north(), vanilla_blocks::STONE.default_state());
        let spreader = MultifaceSpreader::new(&vanilla_blocks::GLOW_LICHEN);

        let spread = spreader
            .spread_from_face_toward_direction(state, &level, pos, Direction::North, Direction::Up)
            .expect("lichen should wrap around its support");

        assert_eq!(spread.pos, target);
        assert_eq!(spread.face, Direction::Down);
        assert!(level.get_block_state(target).get_value(DOWN));
    }

    #[test]
    fn spreading_into_source_water_creates_waterlogged_lichen() {
        init_vanilla_registry();
        let pos = BlockPos::new(0, 64, 0);
        let state = north_facing_state();
        let target = pos.above();
        let level = TestLevel::default()
            .with_block(pos, state)
            .with_block(pos.north(), vanilla_blocks::STONE.default_state())
            .with_block(target, vanilla_blocks::WATER.default_state())
            .with_block(target.north(), vanilla_blocks::STONE.default_state());
        let spreader = MultifaceSpreader::new(&vanilla_blocks::GLOW_LICHEN);

        let spread = spreader
            .spread_from_face_toward_direction(state, &level, pos, Direction::North, Direction::Up)
            .expect("lichen should spread into source water");

        assert_eq!(spread.pos, target);
        assert!(level.get_block_state(target).get_value(WATERLOGGED));
    }

    #[test]
    fn bonemeal_requires_a_vacant_spread_direction() {
        init_vanilla_registry();
        let pos = BlockPos::new(0, 64, 0);
        let full_state = vanilla_blocks::GLOW_LICHEN
            .default_state()
            .set_value(DOWN, true)
            .set_value(UP, true)
            .set_value(NORTH, true)
            .set_value(SOUTH, true)
            .set_value(WEST, true)
            .set_value(EAST, true);
        let level = TestLevel::default().with_block(pos, full_state);
        let behavior = GlowLichenBlock::new(&vanilla_blocks::GLOW_LICHEN);

        assert!(!behavior.is_valid_bonemeal_target(full_state, &level, pos));
    }
}
