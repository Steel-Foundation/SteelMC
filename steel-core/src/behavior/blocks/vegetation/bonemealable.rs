//! Bonemeal-related traits and helpers for block behaviors.

use std::sync::Arc;

use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::{behavior::blocks::vegetation::crop_block::CropLike, world::World};

/// Blocks that react to bonemeal.
pub trait Bonemealable {
    /// Returns the age increase from bonemeal.
    fn get_age_increase(&self, _world: &Arc<World>) -> u8 {
        0
    }

    /// Returns whether bonemeal can be applied.
    fn is_bonemealable(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) -> bool;

    /// Applies the bonemeal effect.
    fn apply_bonemeal(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos);

    /// Returns with a random chance whether the bonemeal should by applied or not
    /// use `rand::random_bool(probability_of_success)`
    fn random_success(&self) -> bool {
        true
    }

    /// Returns how this block uses bonemeal.
    fn bonemeal_action_type(&self) -> BonemealAction {
        BonemealAction::Grower
    }
}

/// How bonemeal affects the block.
pub enum BonemealAction {
    /// Spreads growth to nearby blocks.
    NeighborSpreader,
    /// Grows this block directly.
    Grower,
}

impl BonemealAction {
    /// Returns the particle position for this bonemeal action.
    #[expect(dead_code, reason = "used later for spawning the particles")]
    const fn particle_pos(&self, pos: BlockPos) -> BlockPos {
        match self {
            BonemealAction::NeighborSpreader => pos.above(),
            BonemealAction::Grower => pos,
        }
    }
}

/// Default Bonemeal implementation for all crops
pub trait CropBonemealExt: CropLike + Bonemealable {
    /// Default `apply_bonemeal` implementation for all crops
    fn default_apply_bonemeal(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let new_age = self
            .get_age(state)
            .saturating_add(self.get_age_increase(world))
            .min(self.max_age());

        world.set_block(
            pos,
            state.set_value(self.age_property(), new_age),
            UpdateFlags::UPDATE_ALL,
        );
    }
}

impl<T: CropLike + Bonemealable> CropBonemealExt for T {}
