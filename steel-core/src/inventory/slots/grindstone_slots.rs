//! Grindstone Menu
use std::sync::Arc;

use steel_registry::vanilla_enchantment_tags::EnchantmentTag;
use steel_registry::{REGISTRY, RegistryExt, TaggedRegistryExt};
use steel_registry::{item_stack::ItemStack, level_events};
use steel_utils::{BlockPos, locks::Shared};

use crate::{
    entity::entities::ExperienceOrbEntity,
    inventory::{
        container::{ResultContainer, SimpleContainer},
        prelude::*,
    },
    world::World,
};

/// Result slot handler for an grindstone.
#[derive(Clone)]
pub struct GrindstoneResultHandler {
    input_container: Shared<SimpleContainer>,
    result_container: Shared<ResultContainer>,
    block_pos: BlockPos,
    world: Arc<World>,
}

impl GrindstoneResultHandler {
    /// Creates a new handler.
    pub const fn new(
        input_container: Shared<SimpleContainer>,
        result_container: Shared<ResultContainer>,
        block_pos: BlockPos,
        world: Arc<World>,
    ) -> Self {
        Self {
            input_container,
            result_container,
            block_pos,
            world,
        }
    }

    /// Experience dropped when the result is taken: roughly half the enchanting
    /// cost of both inputs, randomised upward. Must be called before the inputs
    /// are cleared.
    fn get_experience_amount(&self, guard: &ContainerLockGuard) -> i32 {
        let Some(input) = guard.get(ContainerId::from_arc(&self.input_container)) else {
            log::warn!("input container not locked while awarding grindstone experience");
            return 0;
        };

        let amount = Self::get_experience_from_item(input.get_item(0))
            + Self::get_experience_from_item(input.get_item(1));

        if amount > 0 {
            // Ceiling division; `amount` is positive here so truncation cannot bite.
            let half_amount = (amount + 1) / 2;
            half_amount + rand::random_range(0..half_amount)
        } else {
            0
        }
    }

    fn get_experience_from_item(item: &ItemStack) -> i32 {
        let mut amount = 0;
        let Some(enchantments) = item.get_enchantments_for_crafting() else {
            return 0;
        };

        enchantments.iter().for_each(|(id, level)| {
            let Some(enchantment) = REGISTRY.enchantments.by_key(id) else {
                return;
            };

            if REGISTRY
                .enchantments
                .is_in_tag(enchantment, &EnchantmentTag::CURSE)
            {
                return;
            }

            amount += enchantment.min_cost.base
                + enchantment.min_cost.per_level_above_first * (*level as i32 - 1);
        });

        amount
    }
}

impl ResultHandler for GrindstoneResultHandler {
    fn result_container(&self) -> ContainerRef {
        ContainerRef::from(self.result_container.clone())
    }

    fn dependencies(&self) -> Vec<ContainerRef> {
        vec![ContainerRef::from(self.input_container.clone())]
    }

    fn update_result(&self, _guard: &mut ContainerLockGuard) {}

    fn on_result_taken(
        &self,
        guard: &mut ContainerLockGuard,
        _player: &Player,
    ) -> Option<ItemStack> {
        // Read before the inputs are cleared below.
        let experience = self.get_experience_amount(guard);
        ExperienceOrbEntity::award(&self.world, self.block_pos.get_center().into(), experience);

        self.world
            .level_event(level_events::SOUND_GRINDSTONE_USED, self.block_pos, 0, None);

        let id = ContainerId::from_arc(&self.input_container);
        let Some(input) = guard.get_mut(id) else {
            log::warn!("Couldn't get lock for grindstone.");
            return None;
        };

        input.set_item(0, ItemStack::empty());
        input.set_item(1, ItemStack::empty());

        None
    }

    fn is_result_valid(&self, _guard: &ContainerLockGuard, _player: &Player) -> bool {
        true
    }
}
