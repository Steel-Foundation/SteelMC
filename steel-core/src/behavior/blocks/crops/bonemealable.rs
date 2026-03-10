use steel_registry::{
    blocks::block_state_ext::BlockStateExt, item_stack::ItemStack, vanilla_items,
};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::{
    behavior::{InteractionResult, blocks::crops::crop_block::CropLike},
    world::World,
};

pub trait Bonemealable {
    fn get_age_increase(&self, world: &World) -> u8;
    fn is_bonemealable(&self, state: BlockStateId, world: &World, pos: BlockPos) -> bool;
    fn apply_bonemeal(&self, state: BlockStateId, world: &World, pos: BlockPos);
}

pub trait CropBonemealExt: CropLike + Bonemealable {
    fn default_apply_bonemeal(&self, state: BlockStateId, world: &World, pos: BlockPos) {
        let new_age = self
            .get_age(state)
            .saturating_add(self.get_age_increase(world))
            .min(self.max_age());

        world.set_block(
            pos,
            state.set_value(self.age_property(), new_age),
            UpdateFlags::UPDATE_ALL,
        );
    }

    fn default_use_item_on(
        &self,
        item_stack: &ItemStack,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
    ) -> InteractionResult {
        if !self.is_bonemealable(state, world, pos)
            || item_stack.item() != &vanilla_items::ITEMS.bone_meal
        {
            return InteractionResult::Pass;
        }

        self.default_apply_bonemeal(state, world, pos);
        InteractionResult::Success
    }
}

impl<T: CropLike + Bonemealable> CropBonemealExt for T {}
