//! Block behavior implementations for crops and feature-placed vegetation.

use std::sync::Arc;

mod attached_stem_block;
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
mod grass_block;
mod growing_plant_block;
mod growing_plant_body_block;
mod growing_plant_head_block;
mod hanging_moss_block;
mod hanging_roots_block;
mod huge_mushroom_block;
mod kelp_block;
mod kelp_plant_block;
mod leaf_litter_block;
mod leaves_block;
mod lily_pad_block;
mod mangrove_propagule_block;
mod mangrove_roots_block;
mod mossy_carpet_block;
pub(crate) mod multiface_block;
mod mushroom_block;
mod mycelium_block;
mod nether_fungus_block;
mod nether_roots_block;
mod nether_sprouts;
mod nether_vines;
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
mod snowy_block;
mod spore_blossom_block;
mod stem_block;
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

pub use attached_stem_block::AttachedStemBlock;
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
pub use grass_block::GrassBlock;
pub use hanging_moss_block::HangingMossBlock;
pub use hanging_roots_block::HangingRootsBlock;
pub use huge_mushroom_block::HugeMushroomBlock;
pub use kelp_block::KelpBlock;
pub use kelp_plant_block::KelpPlantBlock;
pub use leaf_litter_block::LeafLitterBlock;
pub use leaves_block::{
    MangroveLeavesBlock, TintedParticleLeavesBlock, UntintedParticleLeavesBlock,
};
pub use lily_pad_block::LilyPadBlock;
pub use mangrove_propagule_block::MangrovePropaguleBlock;
pub use mangrove_roots_block::MangroveRootsBlock;
pub use mossy_carpet_block::MossyCarpetBlock;
pub use multiface_block::MultifaceBlock;
pub(crate) use multiface_block::{MultifaceSpreadPos, MultifaceSpreadType, multiface_spread_pos};
pub use mushroom_block::MushroomBlock;
pub use mycelium_block::MyceliumBlock;
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
pub use snowy_block::SnowyBlock;
pub use spore_blossom_block::SporeBlossomBlock;
pub use stem_block::StemBlock;
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

use steel_registry::blocks::properties::Direction;
use steel_registry::blocks::{BlockRef, block_state_ext::BlockStateExt};
use steel_registry::fluid::FluidState;
use steel_registry::vanilla_fluids;
use steel_registry::{vanilla_blocks, vanilla_game_events};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::block::BlockBehavior;
use crate::behavior::block::push_entities_up;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::Entity;
use crate::world::game_event::GameEventContext;
use crate::world::{LevelReader, World};

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

#[cfg(test)]
mod tests {
    use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
    use steel_registry::init_vanilla_registry;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::TestLevel;

    const NORTH: &BoolProperty = &BlockStateProperties::NORTH;
    const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;

    #[test]
    fn multiface_update_uses_supplied_neighbor_state_and_schedules_water_first() {
        init_vanilla_registry();
        init_behaviors();
        let pos = BlockPos::new(0, 64, 0);
        let state = vanilla_blocks::GLOW_LICHEN
            .default_state()
            .set_value(NORTH, true)
            .set_value(WATERLOGGED, true);
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
