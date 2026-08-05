//! Bonemeal-related traits and helpers for block behaviors.

use std::sync::Arc;

use rand::{Rng, RngExt};
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_utils::{BlockPos, BlockStateId, Direction, types::UpdateFlags};

use crate::{
    behavior::BLOCK_BEHAVIORS,
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
    // TODO: Rooted dirt is a GROWER in vanilla but overrides getParticlePos to pos.below().
    // Add a per-block particle-position hook before wiring bonemeal particles.
    #[expect(dead_code, reason = "used later for spawning the particles")]
    const fn particle_pos(&self, pos: BlockPos) -> BlockPos {
        match self {
            BonemealAction::NeighborSpreader => pos.above(),
            BonemealAction::Grower => pos,
        }
    }
}

/// Vanilla spreadable-neighbor target check.
pub fn has_spreadable_neighbor_pos(
    world: &dyn LevelReader,
    pos: BlockPos,
    block_to_place: BlockStateId,
) -> bool {
    get_spreadable_neighbor_pos(Direction::HORIZONTAL, world, pos, block_to_place).is_some()
}

/// Vanilla spreadable-neighbor target selection.
pub fn find_spreadable_neighbor_pos(
    world: &World,
    pos: BlockPos,
    block_to_place: BlockStateId,
) -> Option<BlockPos> {
    let mut directions = Direction::HORIZONTAL;
    shuffle_directions(&mut directions, &mut rand::rng());
    get_spreadable_neighbor_pos(directions, world, pos, block_to_place)
}

fn shuffle_directions(directions: &mut [Direction; 4], random: &mut impl Rng) {
    for i in (1..directions.len()).rev() {
        let j = random.random_range(0..=i);
        directions.swap(i, j);
    }
}

fn get_spreadable_neighbor_pos(
    directions: [Direction; 4],
    world: &dyn LevelReader,
    pos: BlockPos,
    block_to_place: BlockStateId,
) -> Option<BlockPos> {
    let behavior = BLOCK_BEHAVIORS.get_behavior_for_state(block_to_place)?;

    for direction in directions {
        let neighbor_pos = pos.relative(direction);
        if world.get_block_state(neighbor_pos).is_air()
            && behavior.can_survive(block_to_place, world, neighbor_pos)
        {
            return Some(neighbor_pos);
        }
    }

    None
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
