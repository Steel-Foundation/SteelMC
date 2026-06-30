//! Block behavior implementations for crops and feature-placed vegetation.

mod azalea_block;
mod bamboo;
mod bamboo_sapling;
mod base_coral_fan_block;
mod base_coral_plant_block;
mod base_coral_wall_fan_block;
mod beetroots;
mod big_dripleaf_block;
mod big_dripleaf_stem_block;
pub mod bonemealable;
mod bush_block;
mod cactus_block;
mod cactus_flower_block;
mod carpet_block;
mod carrot;
mod cave_vines_block;
mod cave_vines_plant_block;
mod chorus_flower_block;
mod chorus_plant_block;
mod cocoa_block;
mod coral_fan_block;
mod coral_plant_block;
mod coral_wall_fan_block;
mod crop_block;
mod double_plant_block;
mod dry_vegetation_block;
mod eyeblossom_block;
mod farmland_block;
mod firefly_bush_block;
mod flower_bed_block;
mod flower_block;
mod glow_lichen_block;
mod hanging_moss_block;
mod hanging_roots_block;
mod kelp_block;
mod kelp_plant_block;
mod leaf_litter_block;
mod lily_pad_block;
mod mangrove_propagule_block;
mod mossy_carpet_block;
mod mushroom_block;
mod nether_fungus_block;
mod nether_roots_block;
mod nether_sprouts;
mod nether_wart;
mod pitcher_crop;
mod pointed_dripstone_block;
mod potato;
mod rooted_dirt_block;
mod sapling_block;
mod sculk_vein_block;
mod sea_pickle_block;
mod seagrass_block;
mod short_dry_grass_block;
mod small_dripleaf_block;
mod snow_layer_block;
mod spore_blossom_block;
mod sugar_cane;
mod sweet_berry_bush;
mod tall_dry_grass_block;
mod tall_flower_block;
mod tall_grass_block;
mod tall_seagrass_block;
mod torchflower;
mod twisting_vines_block;
mod twisting_vines_plant_block;
mod vegetation_block;
mod vine_block;
mod weeping_vines_block;
mod weeping_vines_plant_block;
mod wither_rose_block;

pub use azalea_block::AzaleaBlock;
pub use bamboo::BambooStalkBlock;
pub use bamboo_sapling::BambooSaplingBlock;
pub use base_coral_fan_block::BaseCoralFanBlock;
pub use base_coral_plant_block::BaseCoralPlantBlock;
pub use base_coral_wall_fan_block::BaseCoralWallFanBlock;
pub use beetroots::BeetrootBlock;
pub use big_dripleaf_block::BigDripleafBlock;
pub use big_dripleaf_stem_block::BigDripleafStemBlock;
pub use bush_block::BushBlock;
pub use cactus_block::CactusBlock;
pub use cactus_flower_block::CactusFlowerBlock;
pub use carpet_block::CarpetBlock;
pub use carrot::CarrotBlock;
pub use cave_vines_block::CaveVinesBlock;
pub use cave_vines_plant_block::CaveVinesPlantBlock;
pub use chorus_flower_block::ChorusFlowerBlock;
pub use chorus_plant_block::ChorusPlantBlock;
pub use cocoa_block::CocoaBlock;
pub use coral_fan_block::CoralFanBlock;
pub use coral_plant_block::CoralPlantBlock;
pub use coral_wall_fan_block::CoralWallFanBlock;
pub use crop_block::CropBlock;
pub use double_plant_block::DoublePlantBlock;
pub use dry_vegetation_block::DryVegetationBlock;
pub use eyeblossom_block::{EyeblossomBlock, EyeblossomType};
pub use farmland_block::FarmlandBlock;
pub use firefly_bush_block::FireflyBushBlock;
pub use flower_bed_block::FlowerBedBlock;
pub use flower_block::FlowerBlock;
pub use glow_lichen_block::GlowLichenBlock;
pub use hanging_moss_block::HangingMossBlock;
pub use hanging_roots_block::HangingRootsBlock;
pub use kelp_block::KelpBlock;
pub use kelp_plant_block::KelpPlantBlock;
pub use leaf_litter_block::LeafLitterBlock;
pub use lily_pad_block::LilyPadBlock;
pub use mangrove_propagule_block::MangrovePropaguleBlock;
pub use mossy_carpet_block::MossyCarpetBlock;
pub use mushroom_block::MushroomBlock;
pub use nether_fungus_block::NetherFungusBlock;
pub use nether_roots_block::NetherRootsBlock;
pub use nether_sprouts::NetherSproutsBlock;
pub use nether_wart::NetherWartBlock;
pub use pitcher_crop::PitcherCropBlock;
pub use pointed_dripstone_block::{PointedDripstoneBlock, SulfurSpikeBlock};
pub use potato::PotatoBlock;
pub use rooted_dirt_block::RootedDirtBlock;
pub use sapling_block::SaplingBlock;
pub use sculk_vein_block::SculkVeinBlock;
pub use sea_pickle_block::SeaPickleBlock;
pub use seagrass_block::SeagrassBlock;
pub use short_dry_grass_block::ShortDryGrassBlock;
pub use small_dripleaf_block::SmallDripleafBlock;
pub use snow_layer_block::SnowLayerBlock;
pub use spore_blossom_block::SporeBlossomBlock;
pub use sugar_cane::SugarCaneBlock;
pub use sweet_berry_bush::SweetBerryBushBlock;
pub use tall_dry_grass_block::TallDryGrassBlock;
pub use tall_flower_block::TallFlowerBlock;
pub use tall_grass_block::TallGrassBlock;
pub use tall_seagrass_block::TallSeagrassBlock;
pub use torchflower::TorchflowerCropBlock;
pub use twisting_vines_block::TwistingVinesBlock;
pub use twisting_vines_plant_block::TwistingVinesPlantBlock;
pub use vegetation_block::Vegetation;
pub use vine_block::VineBlock;
pub use weeping_vines_block::WeepingVinesBlock;
pub use weeping_vines_plant_block::WeepingVinesPlantBlock;
pub use wither_rose_block::WitherRoseBlock;

use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty, Direction};
use steel_registry::blocks::shapes;
use steel_registry::blocks::{BlockRef, block_state_ext::BlockStateExt};
use steel_registry::fluid::{FluidState, FluidStateExt as _};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_fluids;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::context::BlockPlaceContext;
use crate::behavior::{BlockStateBehaviorExt as _, block::BlockBehavior};
use crate::world::{LevelReader, ScheduledTickAccess};

pub(super) type BlockTagRef<'a> = &'a steel_utils::Identifier;

pub(super) fn survives_on_tag(
    world: &dyn LevelReader,
    pos: BlockPos,
    tag: BlockTagRef<'_>,
) -> bool {
    let below = world.get_block_state(pos.below());
    below.get_block().has_tag(tag)
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

/// Vanilla `MultifaceBlock.canAttachTo(level, directionTowardsNeighbor, neighborPos, neighborState)`.
///
/// Returns whether the block at `neighbor_pos` has a full face on the side
/// facing back toward us. Checks the support shape first, then the collision
/// shape, matching vanilla's `Block.isFaceFull` OR pattern.
pub(super) fn can_attach_to_multiface(
    world: &dyn LevelReader,
    neighbor_pos: BlockPos,
    direction_to_neighbor: Direction,
) -> bool {
    let neighbor_state = world.get_block_state(neighbor_pos);
    let support_direction = direction_to_neighbor.opposite();
    shapes::is_offset_face_full(
        neighbor_state.get_support_shape_at(neighbor_pos),
        support_direction,
    ) || shapes::is_offset_face_full(
        neighbor_state.get_collision_shape_at(neighbor_pos),
        support_direction,
    )
}

/// Vanilla `MultifaceBlock.getFaceProperty(faceDirection)`.
pub(super) const fn multiface_face_property(direction: Direction) -> &'static BoolProperty {
    match direction {
        Direction::Up => &BlockStateProperties::UP,
        Direction::Down => &BlockStateProperties::DOWN,
        Direction::North => &BlockStateProperties::NORTH,
        Direction::South => &BlockStateProperties::SOUTH,
        Direction::East => &BlockStateProperties::EAST,
        Direction::West => &BlockStateProperties::WEST,
    }
}

/// Vanilla `MultifaceBlock.canSurvive`.
///
/// Every direction whose face property is `true` must have a neighbor that
/// allows attachment, and at least one face must be set. Subclasses without a
/// face property for a given direction (vanilla's `isFaceSupported`) treat that
/// face as not set, hence `try_get_value(...).unwrap_or(false)`.
pub(super) fn multiface_can_survive(
    state: BlockStateId,
    world: &dyn LevelReader,
    pos: BlockPos,
) -> bool {
    let mut has_face = false;
    for direction in Direction::ALL {
        let property = multiface_face_property(direction);
        if state.try_get_value(property).unwrap_or(false) {
            if !can_attach_to_multiface(world, pos.relative(direction), direction) {
                return false;
            }
            has_face = true;
        }
    }
    has_face
}

/// Vanilla `BaseCoralPlantTypeBlock.canSurvive` (also `BaseCoralFanBlock`,
/// `CoralPlantBlock`, `CoralFanBlock`).
///
/// The block below must be face-sturdy on its UP face.
pub(super) fn coral_plant_can_survive(world: &dyn LevelReader, pos: BlockPos) -> bool {
    let below_pos = pos.below();
    let below = world.get_block_state(below_pos);
    below.is_face_sturdy_at(below_pos, Direction::Up)
}

/// Vanilla `BaseCoralWallFanBlock.canSurvive`.
///
/// The block behind the wall fan (`pos.relative(facing.opposite())`) must be
/// face-sturdy on the face pointing toward us (i.e. `facing`).
pub(super) fn coral_wall_fan_can_survive(
    world: &dyn LevelReader,
    pos: BlockPos,
    facing: Direction,
) -> bool {
    let relative_pos = pos.relative(facing.opposite());
    let relative_state = world.get_block_state(relative_pos);
    relative_state.is_face_sturdy_at(relative_pos, facing)
}

/// Vanilla `BaseCoralPlantTypeBlock.scanForWater`.
pub(super) fn coral_scan_for_water(
    state: BlockStateId,
    world: &dyn LevelReader,
    pos: BlockPos,
) -> bool {
    if state.try_get_value(&BlockStateProperties::WATERLOGGED) == Some(true) {
        return true;
    }

    Direction::ALL.iter().any(|direction| {
        world
            .get_block_state(pos.relative(*direction))
            .get_fluid_state()
            .is_water()
    })
}

/// Vanilla `BaseCoralPlantTypeBlock.tryScheduleDieTick`.
pub(super) fn schedule_coral_die_tick(
    state: BlockStateId,
    world: &dyn ScheduledTickAccess,
    pos: BlockPos,
    block: BlockRef,
) {
    if coral_scan_for_water(state, world, pos) {
        return;
    }

    // Intentional Steel divergence: incidental runtime timing does not use world RNG.
    let delay = 60 + rand::random_range(0..40);
    let _ = world.schedule_block_tick_default(pos, block, delay);
}

/// Vanilla `GrowingPlantBlock.canSurvive`.
///
/// The block opposite the growth direction must be the head, the body, or
/// face-sturdy on the face pointing toward us (i.e. `growth_direction`).
pub(super) fn growing_plant_can_survive(
    world: &dyn LevelReader,
    pos: BlockPos,
    growth_direction: Direction,
    head: BlockRef,
    body: BlockRef,
) -> bool {
    let attached_pos = pos.relative(growth_direction.opposite());
    let attached_state = world.get_block_state(attached_pos);
    let attached_block = attached_state.get_block();
    attached_block == head
        || attached_block == body
        || attached_state.is_face_sturdy_at(attached_pos, growth_direction)
}

pub(super) fn kelp_can_survive(world: &dyn LevelReader, pos: BlockPos) -> bool {
    let attached_pos = pos.below();
    let attached_state = world.get_block_state(attached_pos);
    if attached_state
        .get_block()
        .has_tag(&BlockTag::CANNOT_SUPPORT_KELP)
    {
        return false;
    }

    attached_state.get_block() == &vanilla_blocks::KELP
        || attached_state.get_block() == &vanilla_blocks::KELP_PLANT
        || attached_state.is_face_sturdy_at(attached_pos, Direction::Up)
}
