use std::sync::Arc;

use glam::DVec3;
use steel_protocol::packets::game::RandomizationType;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::fluid::FluidState;
use steel_registry::item_stack::ItemStack;
use steel_registry::particle_type::ParticleData;
use steel_registry::{
    sound_events, vanilla_blocks, vanilla_fluids, vanilla_game_events, vanilla_items,
    vanilla_particle_types,
};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use super::{
    DefaultDispenseBehavior, DispenseItemBehavior, DispenseResult, nested_default_dispense,
};
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::behavior::{BLOCK_BEHAVIORS, BlockStateBehaviorExt as _, pickup_waterlogged_block};
use crate::world::{World, game_event::GameEventContext};

const EVAPORATION_SOUND_VOLUME: f32 = 0.5;
const EVAPORATION_SOUND_PITCH: f32 = 2.6;
const EVAPORATION_SOUND_PITCH_VARIATION: f32 = 0.8;
const EVAPORATION_SMOKE_COUNT: i32 = 8;

pub struct EmptyBucketDispenseBehavior;
pub struct FilledBucketDispenseBehavior;

impl DispenseItemBehavior for EmptyBucketDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        item: &mut ItemStack,
    ) -> DispenseResult {
        let target = pos.relative(state.get_value(FACING));
        let state_at_target = world.get_block_state(target);
        let behavior = BLOCK_BEHAVIORS.get_behavior(state_at_target.get_block());
        let result = behavior
            .pickup_block(world, target, state_at_target, None)
            .or_else(|| pickup_waterlogged_block(behavior, world, target, state_at_target, None));

        if let Some(result) = result {
            world.game_event(
                &vanilla_game_events::FLUID_PICKUP,
                target,
                &GameEventContext::default(),
            );
            return DispenseResult::Byproduct(ItemStack::new(result.filled_bucket.item()));
        }

        DefaultDispenseBehavior.dispense(world, pos, state, item)
    }
}

impl DispenseItemBehavior for FilledBucketDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        item: &mut ItemStack,
    ) -> DispenseResult {
        let target = pos.relative(state.get_value(FACING));
        let target_state = world.get_block_state(target);

        if item.item() == &*vanilla_items::POWDER_SNOW_BUCKET {
            if !world.is_in_world_bounds(target) || !target_state.is_air() {
                return nested_default_dispense(world, pos, state, item);
            }

            world.set_block(
                target,
                vanilla_blocks::POWDER_SNOW.default_state(),
                UpdateFlags::UPDATE_ALL,
            );
            world.game_event(
                &vanilla_game_events::FLUID_PLACE,
                target,
                &GameEventContext::default(),
            );
            world.play_block_sound(
                &sound_events::ITEM_BUCKET_EMPTY_POWDER_SNOW,
                target,
                1.0,
                1.0,
                None,
            );

            return DispenseResult::Byproduct(ItemStack::new(&vanilla_items::BUCKET));
        }

        let (fluid, fluid_block, sound) = if item.item() == &*vanilla_items::WATER_BUCKET {
            (
                &vanilla_fluids::WATER,
                &vanilla_blocks::WATER,
                &sound_events::ITEM_BUCKET_EMPTY,
            )
        } else {
            (
                &vanilla_fluids::LAVA,
                &vanilla_blocks::LAVA,
                &sound_events::ITEM_BUCKET_EMPTY_LAVA,
            )
        };
        let water = fluid == &vanilla_fluids::WATER;
        let behavior = BLOCK_BEHAVIORS.get_behavior(target_state.get_block());
        let liquid_container = target_state.is_liquid_container();
        let can_place_liquid =
            liquid_container && behavior.can_place_liquid_with_player(target_state, fluid, None);
        let can_replace = target_state.can_be_replaced_by_fluid(fluid);
        if !target_state.is_air() && !can_replace && !can_place_liquid {
            return nested_default_dispense(world, pos, state, item);
        }

        if water && world.dimension_type.water_evaporates {
            world.play_block_sound(
                &sound_events::BLOCK_FIRE_EXTINGUISH,
                target,
                EVAPORATION_SOUND_VOLUME,
                EVAPORATION_SOUND_PITCH
                    + (rand::random::<f32>() - rand::random::<f32>())
                        * EVAPORATION_SOUND_PITCH_VARIATION,
                None,
            );
            world.send_particles_with_randomization(
                ParticleData::simple(&vanilla_particle_types::LARGE_SMOKE),
                target.0.as_dvec3(),
                EVAPORATION_SMOKE_COUNT,
                DVec3::ONE,
                0.0,
                RandomizationType::Alternative,
            );

            return DispenseResult::Byproduct(ItemStack::new(&vanilla_items::BUCKET));
        }

        let placed = if water && liquid_container {
            behavior.place_liquid(world, target, target_state, FluidState::source(fluid));
            true
        } else {
            if can_replace && !target_state.get_block().config.liquid && !target_state.is_air() {
                world.destroy_block(target, true);
            }

            let fluid_state = target_state.get_fluid_state();
            let placed = world.set_block(
                target,
                fluid_block.default_state(),
                UpdateFlags::UPDATE_ALL_IMMEDIATE,
            );

            placed || fluid_state.is_source()
        };

        if !placed {
            return nested_default_dispense(world, pos, state, item);
        }

        world.play_block_sound(sound, target, 1.0, 1.0, None);
        world.game_event(
            &vanilla_game_events::FLUID_PLACE,
            target,
            &GameEventContext::default(),
        );

        DispenseResult::Byproduct(ItemStack::new(&vanilla_items::BUCKET))
    }
}
