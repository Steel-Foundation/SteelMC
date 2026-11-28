use std::sync::Arc;

use rand::RngExt;
use steel_macros::item_behavior;
use steel_registry::{
    REGISTRY, TaggedRegistryExt as _,
    blocks::{
        block_state_ext::BlockStateExt, properties::BlockStateProperties,
        shapes::is_offset_shape_full_block,
    },
    data_components::vanilla_components::USE_EFFECTS,
    item_stack::ItemStack,
    level_events,
    vanilla_biome_tags::BiomeTag,
    vanilla_block_tags::BlockTag,
    vanilla_blocks, vanilla_game_events,
};
use steel_utils::{BlockPos, Direction, types::UpdateFlags};

use crate::{
    behavior::{BLOCK_BEHAVIORS, InteractionResult, ItemBehavior, UseOnContext},
    entity::Entity,
    world::{LevelReader as _, World},
};

/// Behavior for the Bonemeal item.
#[item_behavior]
pub struct BoneMealItem;

impl BoneMealItem {
    fn cause_finish_use_vibration(context: &UseOnContext<'_>) {
        let interact_vibrations = context.inv.with_item(|item| {
            item.get(USE_EFFECTS)
                .is_some_and(|effects| effects.interact_vibrations)
        });
        if interact_vibrations {
            context
                .player
                .game_event(&vanilla_game_events::ITEM_INTERACT_FINISH);
        }
    }

    fn grow(world: &Arc<World>, pos: BlockPos) -> bool {
        let state = world.get_block_state(pos);
        let Some(behavior) = BLOCK_BEHAVIORS.get_behavior_for_state(state) else {
            return false;
        };
        if let Some(bonemealable) = behavior.as_bonemealable() {
            if !bonemealable.is_valid_bonemeal_target(state, world.as_ref(), pos) {
                return false;
            }

            let mut rng = rand::rng();
            if bonemealable.is_bonemeal_success(state, world, &mut rng, pos) {
                bonemealable.perform_bonemeal(state, world, &mut rng, pos);
            }

            return true;
        }
        false
    }

    fn grow_water_plant(world: &Arc<World>, pos: BlockPos, clicked_face: Direction) -> bool {
        let state = world.get_block_state(pos);
        if state.get_block() != &vanilla_blocks::WATER || !state.get_fluid_state().is_full() {
            return false;
        }

        let Some(bonemealable) = BLOCK_BEHAVIORS
            .get_behavior(&vanilla_blocks::SEAGRASS)
            .as_bonemealable()
        else {
            return false;
        };

        let mut rng = rand::rng();

        'outer: for i in 0..128 {
            let mut new_pos = pos;
            let mut state_to_grow = vanilla_blocks::SEAGRASS.default_state();

            for _ in 0..(i / 16) {
                new_pos = new_pos.offset(
                    rng.random_range(0i32..3) - 1,
                    (rng.random_range(0i32..3) - 1) * rng.random_range(0i32..3) / 2,
                    rng.random_range(0i32..3) - 1,
                );

                if is_offset_shape_full_block(
                    world
                        .get_block_state(new_pos)
                        .get_collision_shape_at(new_pos),
                ) {
                    continue 'outer;
                }
            }

            if world
                .biome_at(new_pos)
                .is_some_and(|biome| biome.has_tag(&BiomeTag::PRODUCES_CORALS_FROM_BONEMEAL))
            {
                if i == 0 && clicked_face.is_horizontal() {
                    if let Some(corals) = REGISTRY.blocks.get_tag(&BlockTag::WALL_CORALS) {
                        let mut state = corals[rng.random_range(0..corals.len())].default_state();
                        if state
                            .try_get_value(&BlockStateProperties::HORIZONTAL_FACING)
                            .is_some()
                        {
                            state = state
                                .set_value(&BlockStateProperties::HORIZONTAL_FACING, clicked_face);
                        }
                        state_to_grow = state;
                    }
                } else if rng.random_range(0..4) == 0 {
                    if let Some(plants) = REGISTRY.blocks.get_tag(&BlockTag::UNDERWATER_BONEMEALS) {
                        state_to_grow = plants[rng.random_range(0..plants.len())].default_state();
                    }
                }
            }

            if state_to_grow.get_block().has_tag(&BlockTag::WALL_CORALS)
                && state_to_grow
                    .try_get_value(&BlockStateProperties::HORIZONTAL_FACING)
                    .is_some()
            {
                for _ in 0..4 {
                    let Some(behavior) = BLOCK_BEHAVIORS.get_behavior_for_state(state_to_grow)
                    else {
                        break;
                    };
                    if behavior.can_survive(state_to_grow, world, new_pos) {
                        break;
                    }
                    let direction = [
                        Direction::North,
                        Direction::East,
                        Direction::South,
                        Direction::West,
                    ][rng.random_range(0..4)];
                    state_to_grow = state_to_grow
                        .set_value(&BlockStateProperties::HORIZONTAL_FACING, direction);
                }
            }

            let Some(behavior) = BLOCK_BEHAVIORS.get_behavior_for_state(state_to_grow) else {
                return false;
            };

            if behavior.can_survive(state_to_grow, world, new_pos) {
                let current_state = world.get_block_state(new_pos);
                if current_state.get_block() == &vanilla_blocks::WATER
                    && current_state.get_fluid_state().is_full()
                {
                    world.set_block(new_pos, state_to_grow, UpdateFlags::UPDATE_ALL);
                } else if current_state.get_block() == &vanilla_blocks::SEAGRASS
                    && bonemealable.is_valid_bonemeal_target(current_state, world.as_ref(), new_pos)
                    && rng.random_range(0..10) == 0
                {
                    bonemealable.perform_bonemeal(current_state, world, &mut rng, new_pos);
                }
            }
        }

        true
    }
}

impl ItemBehavior for BoneMealItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        if Self::grow(context.world, context.hit_result.block_pos) {
            context.inv.with_item(ItemStack::shrink_one);
            Self::cause_finish_use_vibration(context);
            context.world.level_event(
                level_events::PARTICLES_AND_SOUND_PLANT_GROWTH,
                context.hit_result.block_pos,
                15,
                None,
            );
            return InteractionResult::Success;
        }
        let state = context.world.get_block_state(context.hit_result.block_pos);
        let is_clicked_face_sturdy = context.world.is_face_sturdy(
            state,
            context.hit_result.block_pos,
            context.hit_result.direction,
        );
        if is_clicked_face_sturdy
            && Self::grow_water_plant(
                context.world,
                context
                    .hit_result
                    .block_pos
                    .relative(context.hit_result.direction),
                context.hit_result.direction,
            )
        {
            context.inv.with_item(ItemStack::shrink_one);
            Self::cause_finish_use_vibration(context);
            context.world.level_event(
                level_events::PARTICLES_AND_SOUND_PLANT_GROWTH,
                context
                    .hit_result
                    .block_pos
                    .relative(context.hit_result.direction),
                15,
                None,
            );
            return InteractionResult::Success;
        }
        InteractionResult::Pass
    }
}
