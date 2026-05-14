//! Survival-focused block behaviors for feature-placed vegetation.
//!
//! These are intentionally narrow: worldgen needs vanilla `canSurvive` parity before
//! concrete vegetation features are enabled. Interaction, bonemeal, ticking, and entity
//! effects remain TODOs on the individual partial implementations.

mod azalea_block;
mod bamboo_stalk_block;
mod bush_block;
mod carpet_block;
mod double_plant_block;
mod dry_vegetation_block;
mod firefly_bush_block;
mod flower_bed_block;
mod flower_block;
mod kelp_block;
mod kelp_plant_block;
mod leaf_litter_block;
mod lily_pad_block;
mod mossy_carpet_block;
mod mushroom_block;
mod nether_fungus_block;
mod nether_roots_block;
mod nether_sprouts_block;
mod sea_pickle_block;
mod seagrass_block;
mod short_dry_grass_block;
mod small_dripleaf_block;
mod spore_blossom_block;
mod sweet_berry_bush_block;
mod tall_dry_grass_block;
mod tall_flower_block;
mod tall_grass_block;
mod tall_seagrass_block;
mod wither_rose_block;

pub use azalea_block::AzaleaBlock;
pub use bamboo_stalk_block::BambooStalkBlock;
pub use bush_block::BushBlock;
pub use carpet_block::CarpetBlock;
pub use double_plant_block::DoublePlantBlock;
pub use dry_vegetation_block::DryVegetationBlock;
pub use firefly_bush_block::FireflyBushBlock;
pub use flower_bed_block::FlowerBedBlock;
pub use flower_block::FlowerBlock;
pub use kelp_block::KelpBlock;
pub use kelp_plant_block::KelpPlantBlock;
pub use leaf_litter_block::LeafLitterBlock;
pub use lily_pad_block::LilyPadBlock;
pub use mossy_carpet_block::MossyCarpetBlock;
pub use mushroom_block::MushroomBlock;
pub use nether_fungus_block::NetherFungusBlock;
pub use nether_roots_block::NetherRootsBlock;
pub use nether_sprouts_block::NetherSproutsBlock;
pub use sea_pickle_block::SeaPickleBlock;
pub use seagrass_block::SeagrassBlock;
pub use short_dry_grass_block::ShortDryGrassBlock;
pub use small_dripleaf_block::SmallDripleafBlock;
pub use spore_blossom_block::SporeBlossomBlock;
pub use sweet_berry_bush_block::SweetBerryBushBlock;
pub use tall_dry_grass_block::TallDryGrassBlock;
pub use tall_flower_block::TallFlowerBlock;
pub use tall_grass_block::TallGrassBlock;
pub use tall_seagrass_block::TallSeagrassBlock;
pub use wither_rose_block::WitherRoseBlock;

use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::fluid::FluidState;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_fluids;
use steel_registry::{REGISTRY, TaggedRegistryExt};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

pub(super) type BlockRef = steel_registry::blocks::BlockRef;
pub(super) type BlockTagRef<'a> = &'a steel_utils::Identifier;

pub(super) fn survives_on_tag(
    world: &dyn LevelReader,
    pos: BlockPos,
    tag: BlockTagRef<'_>,
) -> bool {
    let below = world.get_block_state(pos.below());
    REGISTRY.blocks.is_in_tag(below.get_block(), tag)
}

pub(super) fn default_surviving_state(
    block: BlockRef,
    behavior: &dyn BlockBehavior,
    context: &BlockPlaceContext<'_>,
) -> Option<BlockStateId> {
    let state = block.default_state();
    behavior
        .can_survive(state, context.world, context.relative_pos)
        .then_some(state)
}

pub(super) fn water_source_fluid_state() -> FluidState {
    FluidState::source(&vanilla_fluids::WATER)
}

pub(super) fn kelp_can_survive(world: &dyn LevelReader, pos: BlockPos) -> bool {
    let attached_state = world.get_block_state(pos.below());
    if REGISTRY.blocks.is_in_tag(
        attached_state.get_block(),
        &steel_registry::vanilla_block_tags::CANNOT_SUPPORT_KELP_TAG,
    ) {
        return false;
    }

    attached_state.get_block() == &vanilla_blocks::KELP
        || attached_state.get_block() == &vanilla_blocks::KELP_PLANT
        || attached_state.is_face_sturdy(Direction::Up)
}
