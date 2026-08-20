use std::sync::Arc;

use rand::RngExt;
use steel_macros::item_behavior;
use steel_registry::{
    blocks::{block_state_ext::BlockStateExt, shapes::is_offset_shape_full_block},
    data_components::vanilla_components::USE_EFFECTS,
    level_events, vanilla_blocks, vanilla_game_events,
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

    fn grow_water_plant(world: &Arc<World>, pos: BlockPos, _clicked_face: Direction) -> bool {
        let state = world.get_block_state(pos);
        if state.get_block() != &vanilla_blocks::WATER || state.get_fluid_state().amount != 8 {
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
            let new_state = vanilla_blocks::SEAGRASS.default_state();

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

            // TODO: implement coral and underwater bonemeal tag selection.

            let Some(behavior) = BLOCK_BEHAVIORS.get_behavior_for_state(new_state) else {
                return false;
            };

            if behavior.can_survive(new_state, world, new_pos) {
                let current_state = world.get_block_state(new_pos);
                if current_state.get_block() == &vanilla_blocks::WATER
                    && current_state.get_fluid_state().amount == 8
                {
                    world.set_block(new_pos, new_state, UpdateFlags::UPDATE_ALL);
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
            context.inv.with_item(|item| item.shrink(1));
            Self::cause_finish_use_vibration(context);
            context.world.level_event(
                level_events::PARTICLES_AND_SOUND_PLANT_GROWTH,
                context.hit_result.block_pos,
                15,
                None,
            );
            return InteractionResult::SuccessServer;
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
            context.inv.with_item(|item| item.shrink(1));
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

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::items::item::BlockHitResult;
    use steel_registry::{init_vanilla_registry, vanilla_items};
    use steel_utils::{ChunkPos, types::InteractionHand};
    use uuid::Uuid;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn crop_bonemeal_requests_server_swing() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("bonemeal_crop_server_swing");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::DIRT.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        assert!(world.set_block(
            pos,
            vanilla_blocks::OAK_SAPLING.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let player =
            TestPlayerBuilder::new(world.clone(), Uuid::from_u128(1), "TestPlayer", 1).build();
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::BONE_MEAL));
        let mut context = UseOnContext::new(
            &player,
            InteractionHand::MainHand,
            BlockHitResult {
                location: DVec3::new(8.5, 64.5, 8.5),
                direction: Direction::Up,
                block_pos: pos,
                miss: false,
                inside: false,
                world_border_hit: false,
            },
            &world,
            Arc::clone(&player.inventory),
        );

        let result = BoneMealItem.use_on(&mut context);

        assert_eq!(result, InteractionResult::SuccessServer);
        assert!(result.should_swing_server());
    }
}
