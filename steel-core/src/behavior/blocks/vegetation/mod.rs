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
mod carved_pumpkin_block;
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
mod shelf_mushroom_block;
mod short_dry_grass_block;
mod small_dripleaf_block;
mod snowy_block;
mod spore_blossom_block;
mod spreading_grass_block;
mod stem_block;
mod sugar_cane;
mod sweet_berry_bush;
mod tall_dry_grass_block;
mod tall_flower_block;
mod tall_grass_block;
mod tall_seagrass_block;
mod torchflower;
mod turtle_egg_block;
mod twisting_vines_block;
mod twisting_vines_plant_block;
mod vegetation_block;
mod vine_block;
mod weeping_vines_block;
mod weeping_vines_plant_block;
mod wither_rose_block;

pub use attached_stem_block::*;
pub use azalea_block::*;
pub use bamboo::*;
pub use bamboo_sapling::*;
pub use base_coral_fan_block::*;
pub use base_coral_plant_block::*;
pub use base_coral_wall_fan_block::*;
pub use beetroots::*;
pub use big_dripleaf_block::*;
pub use big_dripleaf_stem_block::*;
pub use bush_block::*;
pub use cactus_block::*;
pub use cactus_flower_block::*;
pub use carpet_block::*;
pub use carrot::*;
pub use carved_pumpkin_block::*;
pub use cave_vines_block::*;
pub use cave_vines_plant_block::*;
pub use chorus_flower_block::*;
pub use chorus_plant_block::*;
pub use cocoa_block::*;
pub use coral_block::*;
pub use coral_fan_block::*;
pub use coral_plant_block::*;
pub use coral_wall_fan_block::*;
pub use crop_block::*;
pub use dirt_path_block::*;
pub use double_plant_block::*;
pub use dry_vegetation_block::*;
pub use eyeblossom_block::*;
pub use farmland_block::*;
pub use firefly_bush_block::*;
pub use flower_bed_block::*;
pub use flower_block::*;
pub use glow_lichen_block::*;
pub use grass_block::*;
pub use growing_plant_head_block::*;
pub use hanging_moss_block::*;
pub use hanging_roots_block::*;
pub use huge_mushroom_block::*;
pub use kelp_block::*;
pub use kelp_plant_block::*;
pub use leaf_litter_block::*;
pub use leaves_block::*;
pub use lily_pad_block::*;
pub use mangrove_propagule_block::*;
pub use mangrove_roots_block::*;
pub use mossy_carpet_block::*;
pub use multiface_block::*;
pub use mushroom_block::*;
pub use mycelium_block::*;
pub use nether_fungus_block::*;
pub use nether_roots_block::*;
pub use nether_sprouts::*;
pub use nether_wart::*;
pub use pitcher_crop::*;
pub use pointed_dripstone_block::*;
pub use potato::*;
pub use pumpkin_block::*;
pub use rooted_dirt_block::*;
pub use sapling_block::*;
pub use sculk_vein_block::*;
pub use sea_pickle_block::*;
pub use seagrass_block::*;
pub use shelf_mushroom_block::*;
pub use short_dry_grass_block::*;
pub use small_dripleaf_block::*;
pub use snowy_block::*;
pub use spore_blossom_block::*;
pub use spreading_grass_block::*;
pub use stem_block::*;
pub use sugar_cane::*;
pub use sweet_berry_bush::*;
pub use tall_dry_grass_block::*;
pub use tall_flower_block::*;
pub use tall_grass_block::*;
pub use tall_seagrass_block::*;
pub use torchflower::*;
pub use turtle_egg_block::*;
pub use twisting_vines_block::*;
pub use twisting_vines_plant_block::*;
pub use vegetation_block::*;
pub use vine_block::*;
pub use weeping_vines_block::*;
pub use weeping_vines_plant_block::*;
pub use wither_rose_block::*;

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
