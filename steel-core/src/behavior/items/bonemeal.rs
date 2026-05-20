use std::sync::Arc;

use rand::RngExt;
use steel_macros::item_behavior;
use steel_registry::{
    blocks::{block_state_ext::BlockStateExt, shapes::is_shape_full_block},
<<<<<<< HEAD
=======
    item_stack::ItemStack,
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
    vanilla_blocks,
};
use steel_utils::{BlockPos, Direction, types::UpdateFlags};

use crate::{
    behavior::{
        BLOCK_BEHAVIORS, BlockStateBehaviorExt, InteractionResult, ItemBehavior, UseOnContext,
    },
    world::World,
};

/// Behavior for the Bonemeal item.
#[item_behavior]
pub struct BoneMealItem;

impl BoneMealItem {
<<<<<<< HEAD
    fn grow(world: &Arc<World>, pos: BlockPos) -> bool {
=======
    fn grow(item_stack: &mut ItemStack, world: &Arc<World>, pos: BlockPos) -> bool {
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
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
<<<<<<< HEAD
=======
            item_stack.shrink(1);
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))

            return true;
        }
        false
    }

<<<<<<< HEAD
    fn grow_water_plant(world: &Arc<World>, pos: BlockPos, _clicked_face: Direction) -> bool {
=======
    fn grow_water_plant(
        item_stack: &mut ItemStack,
        world: &Arc<World>,
        pos: BlockPos,
        _clicked_face: Direction,
    ) -> bool {
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
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

                if is_shape_full_block(world.get_block_state(new_pos).get_collision_shape()) {
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

<<<<<<< HEAD
=======
        item_stack.shrink(1);

>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
        true
    }
}

impl ItemBehavior for BoneMealItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
<<<<<<< HEAD
        if Self::grow(context.world, context.hit_result.block_pos) {
            context.inv.with_item(|item| item.shrink(1));
=======
        if Self::grow(
            context.inv.item(),
            context.world,
            context.hit_result.block_pos,
        ) {
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
            // TODO: particles
            return InteractionResult::Success;
        }
        let state = context.world.get_block_state(context.hit_result.block_pos);
        let is_clicked_face_sturdy = state.is_face_sturdy(context.hit_result.direction);
        if is_clicked_face_sturdy
            && Self::grow_water_plant(
<<<<<<< HEAD
=======
                context.inv.item(),
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
                context.world,
                context
                    .hit_result
                    .block_pos
                    .relative(context.hit_result.direction),
                context.hit_result.direction,
            )
        {
<<<<<<< HEAD
            context.inv.with_item(|item| item.shrink(1));
=======
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
            return InteractionResult::Success;
        }
        InteractionResult::Pass
    }
}
