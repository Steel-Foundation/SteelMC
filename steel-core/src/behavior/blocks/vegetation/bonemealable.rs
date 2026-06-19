//! Bonemeal-related traits and helpers for block behaviors.

use std::sync::Arc;

use rand::Rng;
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::{
    behavior::blocks::vegetation::crop_block::CropLike,
    world::{LevelReader, World},
};

/// Blocks that react to bonemeal.
pub trait Bonemealable {
    /// Returns the age increase from bonemeal.
    fn get_bonemeal_age_increase(&self, _world: &Arc<World>, _rng: &mut dyn Rng) -> u8 {
        0
    }

    /// Returns whether this block is a valid bonemeal target.
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool;

    /// Returns whether bonemeal succeeds after the target check passes.
    fn is_bonemeal_success(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _rng: &mut dyn Rng,
        _pos: BlockPos,
    ) -> bool {
        true
    }

    /// Applies the bonemeal effect.
    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn Rng,
        pos: BlockPos,
    );

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
    /// Default `perform_bonemeal` implementation for all crops
    fn default_perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        let new_age = self
            .get_age(state)
            .saturating_add(self.get_bonemeal_age_increase(world, rng))
            .min(self.max_age());

        world.set_block(
            pos,
            self.get_state_for_age(new_age),
            UpdateFlags::UPDATE_ALL,
        );
    }
}

impl<T: CropLike + Bonemealable> CropBonemealExt for T {}
