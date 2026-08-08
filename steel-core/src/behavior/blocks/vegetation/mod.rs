//! Block behavior implementations for crops and feature-placed vegetation.

use std::sync::Arc;

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
mod coral_block;
mod coral_fan_block;
mod coral_plant_block;
mod coral_wall_fan_block;
mod crop_block;
mod dirt_path_block;
mod double_plant_block;
mod dry_vegetation_block;
mod eyeblossom_block;
mod farmland_block;
mod firefly_bush_block;
mod flower_bed_block;
mod flower_block;
mod glow_lichen_block;
mod growing_plant_body_block;
mod growing_plant_head_block;
mod hanging_moss_block;
mod hanging_roots_block;
mod kelp_block;
mod kelp_plant_block;
mod leaf_litter_block;
mod leaves_block;
mod lily_pad_block;
mod mangrove_propagule_block;
mod mossy_carpet_block;
mod multiface_block;
mod mushroom_block;
mod nether_fungus_block;
mod nether_roots_block;
mod nether_sprouts;
mod nether_wart;
mod pitcher_crop;
mod pointed_dripstone_block;
mod potato;
mod pumpkin_block;
mod rooted_dirt_block;
mod sapling_block;
mod sculk_vein_block;
mod sea_pickle_block;
mod seagrass_block;
mod segmentable_block;
mod short_dry_grass_block;
mod small_dripleaf_block;
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
pub use carpet_block::{CarpetBlock, WoolCarpetBlock};
pub use carrot::CarrotBlock;
pub use cave_vines_block::CaveVinesBlock;
pub use cave_vines_plant_block::CaveVinesPlantBlock;
pub use chorus_flower_block::ChorusFlowerBlock;
pub use chorus_plant_block::ChorusPlantBlock;
pub use cocoa_block::CocoaBlock;
pub use coral_block::CoralBlock;
pub use coral_fan_block::CoralFanBlock;
pub use coral_plant_block::CoralPlantBlock;
pub use coral_wall_fan_block::CoralWallFanBlock;
pub use crop_block::CropBlock;
pub use dirt_path_block::DirtPathBlock;
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
pub use leaves_block::{
    MangroveLeavesBlock, TintedParticleLeavesBlock, UntintedParticleLeavesBlock,
};
pub use lily_pad_block::LilyPadBlock;
pub use mangrove_propagule_block::MangrovePropaguleBlock;
pub use mossy_carpet_block::MossyCarpetBlock;
pub use multiface_block::MultifaceBlock;
pub use mushroom_block::MushroomBlock;
pub use nether_fungus_block::NetherFungusBlock;
pub use nether_roots_block::NetherRootsBlock;
pub use nether_sprouts::NetherSproutsBlock;
pub use nether_wart::NetherWartBlock;
pub use pitcher_crop::PitcherCropBlock;
pub use pointed_dripstone_block::{
    PointedDripstoneBlock, SulfurSpikeBlock, find_stalactite_tip_above_cauldron,
    get_cauldron_fill_fluid_type,
};
pub use potato::PotatoBlock;
pub use pumpkin_block::PumpkinBlock;
pub use rooted_dirt_block::RootedDirtBlock;
pub use sapling_block::SaplingBlock;
pub use sculk_vein_block::SculkVeinBlock;
pub use sea_pickle_block::SeaPickleBlock;
pub use seagrass_block::SeagrassBlock;
pub use short_dry_grass_block::ShortDryGrassBlock;
pub use small_dripleaf_block::SmallDripleafBlock;
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

use rand::{Rng, RngExt};
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::blocks::{BlockRef, block_state_ext::BlockStateExt};
use steel_registry::fluid::{FluidState, FluidStateExt as _};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_fluids;
use steel_registry::{vanilla_blocks, vanilla_game_events};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::block::BlockBehavior;
use crate::behavior::block::push_entities_up;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::Entity;
use crate::world::game_event::GameEventContext;
use crate::world::{LevelReader, ScheduledTickAccess, World};

pub(super) type BlockTagRef<'a> = &'a steel_utils::Identifier;

/// Turns farmland or a dirt path into dirt.
pub(crate) fn turn_to_dirt(
    state: BlockStateId,
    world: &Arc<World>,
    pos: BlockPos,
    source_entity: Option<&dyn Entity>,
) {
    let dirt_state = push_entities_up(state, vanilla_blocks::DIRT.default_state(), world, pos);
    if world.set_block(pos, dirt_state, UpdateFlags::UPDATE_ALL) {
        world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            pos,
            &GameEventContext::new(source_entity, Some(dirt_state)),
        );
    }
}

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
        .can_survive(state, context.world, context.place_pos())
        .then_some(state)
}

pub(super) fn water_source_fluid_state() -> FluidState {
    FluidState::source(&vanilla_fluids::WATER)
}

/// Vanilla `getBlocksToGrowWhenBonemealed()`
pub fn nether_vines_get_blocks_to_grow_when_bonemealed(rng: &mut dyn Rng) -> i32 {
    let mut grow_probability = 1.0;

    let mut count = 0;

    while rng.random::<f64>() < grow_probability {
        grow_probability *= 0.826;
        count += 1;
    }
    count
}
/// Vanilla equal `getTopConnectedBlock()`
pub fn get_top_connected_block(
    world: &dyn LevelReader,
    pos: BlockPos,
    body_block: BlockRef,
    growth_direction: Direction,
    head_block: BlockRef,
) -> Option<BlockPos> {
    let mut forward_pos = pos;
    let mut forward_state;

    loop {
        forward_pos = forward_pos.relative(growth_direction);
        forward_state = world.get_block_state(forward_pos);

        if forward_state.get_block() != body_block {
            break;
        }
    }

    if forward_state.get_block() == head_block {
        Some(forward_pos)
    } else {
        None
    }
}

/// Vanilla `BaseCoralPlantTypeBlock.canSurvive` (also `BaseCoralFanBlock`,
/// `CoralPlantBlock`, `CoralFanBlock`).
///
/// The block below must be face-sturdy on its UP face.
pub(super) fn coral_plant_can_survive(world: &dyn LevelReader, pos: BlockPos) -> bool {
    let below_pos = pos.below();
    let below = world.get_block_state(below_pos);
    world.is_face_sturdy(below, below_pos, Direction::Up)
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
    world.is_face_sturdy(relative_state, relative_pos, facing)
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
        || world.is_face_sturdy(attached_state, attached_pos, growth_direction)
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
    growing_plant_can_survive(
        world,
        pos,
        Direction::Up,
        &vanilla_blocks::KELP,
        &vanilla_blocks::KELP_PLANT,
    )
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::TestLevel;

    #[test]
    fn multiface_update_uses_supplied_neighbor_state_and_schedules_water_first() {
        init_vanilla_registry();
        init_behaviors();
        let pos = BlockPos::new(0, 64, 0);
        let state = vanilla_blocks::GLOW_LICHEN
            .default_state()
            .set_value(&BlockStateProperties::NORTH, true)
            .set_value(&BlockStateProperties::WATERLOGGED, true);
        let level =
            TestLevel::default().with_block(pos.north(), vanilla_blocks::STONE.default_state());

        let updated = MultifaceBlock::update_shape(
            &MultifaceBlock::new(&vanilla_blocks::GLOW_LICHEN),
            state,
            &level,
            pos,
            Direction::North,
            pos.north(),
            vanilla_blocks::AIR.default_state(),
        );

        assert!(updated.is_air());
        assert!(level.scheduled_water_tick());
    }
}
