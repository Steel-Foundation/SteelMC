use steel_registry::item_stack::ItemStack;
use steel_utils::locks::Shared;

use crate::inventory::container::{CraftingContainer, ResultContainer};
use crate::{
    inventory::{
        container::Container,
        lock::{ContainerId, ContainerLockGuard, ContainerRef},
        recipe_manager,
        slots::result_handler::ResultHandler,
    },
    player::Player,
};

/// A Recipe Handler for Crafting Recipes
#[derive(Clone)]
pub struct CraftingHandler {
    crafting_container: Shared<CraftingContainer>,
    result_container: Shared<ResultContainer>,
    grid_size: usize,
}

impl CraftingHandler {
    /// Creates a new Crafting Recipe Handler
    pub const fn new(
        crafting_container: Shared<CraftingContainer>,
        result_container: Shared<ResultContainer>,
        grid_size: usize,
    ) -> Self {
        Self {
            crafting_container,
            result_container,
            grid_size,
        }
    }

    /// Whether the grid size of the crafting container is a 2x2
    #[must_use]
    pub const fn is_2x2(&self) -> bool {
        self.grid_size == 2
    }

    /// The `ContainerId` of the crafting container
    #[must_use]
    pub fn crafting_id(&self) -> ContainerId {
        ContainerId::from_arc(&self.crafting_container)
    }

    /// A shared handle to the crafting container.
    #[must_use]
    pub fn crafting_container(&self) -> Shared<CraftingContainer> {
        self.crafting_container.clone()
    }

    /// The `ContainerId` of the result container
    #[must_use]
    pub fn result_id(&self) -> ContainerId {
        ContainerId::from_arc(&self.result_container)
    }
}

impl ResultHandler for CraftingHandler {
    fn result_container(&self) -> ContainerRef {
        ContainerRef::from(self.result_container.clone())
    }

    fn dependencies(&self) -> Vec<ContainerRef> {
        vec![ContainerRef::from(self.crafting_container.clone())]
    }

    fn update_result(&self, guard: &mut ContainerLockGuard) {
        let crafting = guard
            .get_typed::<CraftingContainer>(self.crafting_id())
            .expect("crafting container not locked");

        let result_stack = recipe_manager::find_recipe(crafting, self.is_2x2())
            .map_or_else(ItemStack::empty, |r| r.assemble());

        let result_container = guard
            .get_typed_mut::<ResultContainer>(self.result_id())
            .expect("result container not locked");
        result_container.set_item(0, result_stack);
        result_container.set_changed();
    }

    fn on_result_taken(
        &self,
        guard: &mut ContainerLockGuard,
        player: &Player,
    ) -> Option<ItemStack> {
        let mut remainder_overflow: Vec<ItemStack> = Vec::new();

        let remainders_and_positioned = {
            let crafting = guard
                .get_typed::<CraftingContainer>(self.crafting_id())
                .expect("crafting container not locked");
            recipe_manager::get_remaining_items(crafting, self.is_2x2())
        };

        let Some((remainders, positioned)) = remainders_and_positioned else {
            guard
                .get_typed_mut::<ResultContainer>(self.result_id())
                .expect("result container not locked")
                .set_item(0, ItemStack::empty());
            return None;
        };

        {
            let crafting = guard
                .get_typed_mut::<CraftingContainer>(self.crafting_id())
                .expect("crafting container not locked");

            let input = &positioned.input;

            for y in 0..input.height {
                for x in 0..input.width {
                    let grid_slot = positioned.to_grid_slot(x, y, self.grid_size);
                    let remainder_idx = x + y * input.width;
                    let replacement = if remainder_idx < remainders.len() {
                        remainders[remainder_idx].clone()
                    } else {
                        ItemStack::empty()
                    };

                    {
                        let item = crafting.get_item_mut(grid_slot);
                        if !item.is_empty() {
                            item.shrink(1);
                        }
                    }

                    if !replacement.is_empty() {
                        let current_item = crafting.get_item(grid_slot).clone();

                        if current_item.is_empty() {
                            crafting.set_item(grid_slot, replacement);
                        } else if ItemStack::is_same_item_same_components(
                            &current_item,
                            &replacement,
                        ) {
                            crafting.get_item_mut(grid_slot).grow(replacement.count());
                        } else {
                            remainder_overflow.push(replacement);
                        }
                    }
                }
            }

            crafting.set_changed();
        }

        self.update_result(guard);

        for remainder in remainder_overflow {
            player.add_item_or_drop_with_guard(guard, remainder);
        }

        None
    }

    fn is_result_valid(&self, guard: &ContainerLockGuard, _player: &Player) -> bool {
        let Some(result) = guard.get(self.result_id()) else {
            return false;
        };
        let result_item = result.get_item(0);
        if result_item.is_empty() {
            return false;
        }

        let Some(crafting) = guard.get_typed::<CraftingContainer>(self.crafting_id()) else {
            return false;
        };

        let Some(recipe) = recipe_manager::find_recipe(crafting, self.is_2x2()) else {
            return false;
        };

        ItemStack::matches(result_item, &recipe.assemble())
    }
}
