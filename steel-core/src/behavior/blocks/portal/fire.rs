//! Fire block behavior implementation.
//!
//! Vanilla splits fire into `BaseFireBlock` (portal logic, placement checks) and `FireBlock`
//! (spreading, aging). This combines the portal-relevant parts from `BaseFireBlock`.

use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_dimension_types;
use steel_utils::math::Axis;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::portal::portal_shape::{PortalShape, nether_portal_config};
use crate::world::World;

/// Behavior for fire blocks.
#[block_behavior]
pub struct FireBlock {
    block: BlockRef,
}

impl FireBlock {
    /// Creates a new fire block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Returns true if the dimension supports nether portal creation (Overworld or Nether).
    pub(crate) fn in_portal_dimension(world: &World) -> bool {
        world.dimension == vanilla_dimension_types::OVERWORLD
            || world.dimension == vanilla_dimension_types::THE_NETHER
    }

    /// Checks if fire can be placed at `pos`, matching vanilla's `BaseFireBlock.canBePlacedAt`.
    /// Position must be air AND (fire can survive there OR it's a valid portal location).
    pub(crate) fn can_be_placed_at(
        world: &Arc<World>,
        pos: BlockPos,
        forward_dir: Direction,
    ) -> bool {
        if !world.get_block_state(pos).is_air() {
            return false;
        }
        Self::can_survive_at(world, pos) || Self::is_portal(world, pos, forward_dir)
    }

    /// Matches vanilla's `FireBlock.canSurvive`: block below has a sturdy top face,
    /// or an adjacent block is flammable.
    fn can_survive_at(world: &Arc<World>, pos: BlockPos) -> bool {
        world
            .get_block_state(pos.below())
            .is_face_sturdy(Direction::Up)
        // TODO: || is_valid_fire_location (check adjacent flammable blocks once flammability exists)
    }

    /// Matches vanilla's `BaseFireBlock.isPortal`: checks if placing fire here could form a portal.
    /// Requires portal dimension, adjacent obsidian, and a valid empty portal shape.
    fn is_portal(world: &Arc<World>, pos: BlockPos, forward_dir: Direction) -> bool {
        if !Self::in_portal_dimension(world) {
            return false;
        }

        let has_obsidian = Direction::ALL.iter().any(|&dir| {
            world.get_block_state(pos.relative(dir)).get_block() == vanilla_blocks::OBSIDIAN
        });
        if !has_obsidian {
            return false;
        }

        let preferred_axis = if forward_dir.get_axis().is_horizontal() {
            forward_dir.rotate_y_counter_clockwise().get_axis()
        } else if rand::random::<bool>() {
            Axis::X
        } else {
            Axis::Z
        };

        let config = nether_portal_config();
        PortalShape::find_empty_portal_shape_with_axis(world, pos, preferred_axis, &config)
            .is_some()
    }
}

impl BlockBehavior for FireBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn can_survive(&self, _state: BlockStateId, world: &Arc<World>, pos: BlockPos) -> bool {
        Self::can_survive_at(world, pos)
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        // Only attempt portal creation when fire is newly placed, not when replacing itself
        if old_state.get_block() == state.get_block() {
            return;
        }

        if Self::in_portal_dimension(world)
            && let Some(shape) =
                PortalShape::find_empty_portal_shape(world, pos, &nether_portal_config())
        {
            shape.place_portal_blocks(world);
            return;
        }

        if !self.can_survive(state, world, pos) {
            world.set_block(
                pos,
                vanilla_blocks::AIR.default_state(),
                UpdateFlags::UPDATE_ALL,
            );
        }
    }
}
