//! Brewing-stand menu.

use std::array::from_fn;

use steel_registry::recipe::PotionIngredient;
use steel_registry::vanilla_menu_types;
use steel_utils::{DowncastType, DowncastTypeKey, locks::Shared};

use crate::block_entity::entities::{BREWING_DATA_SLOTS, BrewingStandContainer};
use crate::inventory::menu::builder::SectionKind;
use crate::inventory::prelude::*;
use crate::inventory::slots::{FuelSlot, IngredientsSlot, PotionSlot};
use crate::player::player_inventory::{PlayerInventory, Prediction};

/// Builds the brewing stand menu.
#[must_use]
pub fn brewing_stand_menu(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    container: ContainerRef,
) -> Menu {
    let mut builder = MenuBuilder::new(&vanilla_menu_types::BREWING_STAND, container_id);

    let potions = builder.section_at(
        container.clone(),
        [0, 1, 2],
        SectionKind::custom(|container, index| Box::new(PotionSlot::new(container.clone(), index))),
    );
    let ingredient = builder.section_at(
        container.clone(),
        [3],
        SectionKind::custom(|container, index| {
            Box::new(IngredientsSlot::new(container.clone(), index))
        }),
    );
    let fuel = builder.section_at(
        container.clone(),
        [4],
        SectionKind::custom(|container, index| Box::new(FuelSlot::new(container.clone(), index))),
    );
    let player = builder.player_inventory(&inventory);
    let data = from_fn(|_| builder.data_slot(0));

    builder.build(BrewingStandMenuKind {
        container,
        potions,
        ingredient,
        fuel,
        player,
        data,
    })
}

/// State and shift-click behavior for the brewing-stand menu.
pub struct BrewingStandMenuKind {
    container: ContainerRef,
    potions: Section,
    ingredient: Section,
    fuel: Section,
    player: PlayerInventorySections,
    data: [DataSlot; BREWING_DATA_SLOTS],
}

// SAFETY: This Steel-owned key uniquely identifies brewing-stand menu behavior.
unsafe impl DowncastType for BrewingStandMenuKind {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:menu/brewing_stand");
}

impl BrewingStandMenuKind {
    fn update_data(&self, behavior: &mut MenuBehavior, guard: &ContainerLockGuard) {
        let Some(container) =
            guard.get_typed::<BrewingStandContainer>(self.container.container_id())
        else {
            return;
        };
        for (slot, value) in self.data.into_iter().zip(container.data()) {
            slot.set(behavior, value);
        }
    }

    const fn is_brewing_slot(&self, slot_index: usize) -> bool {
        self.potions.contains(slot_index)
            || self.ingredient.contains(slot_index)
            || self.fuel.contains(slot_index)
    }

    fn ingredient_may_place(&self, behavior: &MenuBehavior, stack: &ItemStack) -> bool {
        behavior.slots()[self.ingredient.start()].may_place(stack)
    }
}

impl MenuKind for BrewingStandMenuKind {
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        self.container.still_valid(player)
    }

    fn on_open(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
        self.update_data(behavior, guard);
    }

    fn on_tick(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
        self.update_data(behavior, guard);
    }

    fn quick_move(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        slot_index: usize,
        player: &Player,
    ) -> Option<ItemStack> {
        if slot_index >= behavior.slots().len() {
            return Some(ItemStack::empty());
        }
        let clicked = behavior.slots()[slot_index].get_item(guard).clone();
        if clicked.is_empty() {
            return Some(ItemStack::empty());
        }
        let mut remaining = clicked.clone();

        let moved = if self.is_brewing_slot(slot_index) {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                self.player.all().start(),
                self.player.all().end(),
                FillDirection::Backward,
            )
        } else if FuelSlot::may_place_item(&clicked) {
            let fuel_moved = behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                self.fuel.start(),
                self.fuel.end(),
                FillDirection::Forward,
            );
            if fuel_moved {
                // A successful fuel move ends the quick-move loop, so the
                // remainder stays in the source slot instead of falling
                // through to the ingredient slot.
                behavior.update_quick_move_source(guard, slot_index, &remaining, &clicked);
                return Some(ItemStack::empty());
            }
            if self.ingredient_may_place(behavior, &clicked) {
                behavior.move_item_stack_to(
                    guard,
                    slot_index,
                    &mut remaining,
                    self.ingredient.start(),
                    self.ingredient.end(),
                    FillDirection::Forward,
                );
            }
            remaining.count() != clicked.count()
        } else if self.ingredient_may_place(behavior, &clicked) {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                self.ingredient.start(),
                self.ingredient.end(),
                FillDirection::Forward,
            )
        } else if PotionIngredient::is_potion_input(&clicked) {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                self.potions.start(),
                self.potions.end(),
                FillDirection::Forward,
            )
        } else if self.player.main().contains(slot_index) {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                self.player.hotbar().start(),
                self.player.hotbar().end(),
                FillDirection::Forward,
            )
        } else if self.player.hotbar().contains(slot_index) {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                self.player.main().start(),
                self.player.main().end(),
                FillDirection::Forward,
            )
        } else {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                self.player.all().start(),
                self.player.all().end(),
                FillDirection::Forward,
            )
        };

        if !moved {
            return Some(ItemStack::empty());
        }
        behavior.update_quick_move_source(guard, slot_index, &remaining, &clicked);
        if remaining.count() == clicked.count() {
            return Some(ItemStack::empty());
        }
        let taken = clicked.copy_with_count(clicked.count - remaining.count);
        let slot = &behavior.slots()[slot_index];
        slot.on_quick_craft(&taken, player);
        if let Some(remainder) = slot.on_take(guard, &remaining, player) {
            player.add_item_or_drop_with_guard(guard, remainder, Prediction::Predicted);
        }
        Some(clicked)
    }
}
