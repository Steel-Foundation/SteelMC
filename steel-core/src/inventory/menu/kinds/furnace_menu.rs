//! Furnace, blast-furnace, and smoker menus.

use std::array::from_fn;

use steel_registry::recipe::SingleItemRecipeInput;
use steel_registry::{REGISTRY, menu_type::MenuTypeRef, vanilla_menu_types};
use steel_utils::{DowncastType, DowncastTypeKey, locks::Shared};

use crate::block_entity::entities::{FurnaceContainer, FurnaceKind};
use crate::inventory::fuel_values::VANILLA_FUEL_VALUES;
use crate::inventory::menu::builder::SectionKind;
use crate::inventory::prelude::*;
use crate::inventory::slots::{FurnaceFuelSlot, FurnaceResultSlot};
use crate::player::player_inventory::PlayerInventory;

/// Builds the three-slot Vanilla menu for one furnace-family block entity.
#[must_use]
pub fn furnace_menu(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    container: ContainerRef,
    kind: FurnaceKind,
) -> Menu {
    let mut builder = MenuBuilder::new(menu_type(kind), container_id);

    let input = builder.section_at(container.clone(), [0], SectionKind::Normal);
    let fuel = builder.section_at(
        container.clone(),
        [1],
        SectionKind::custom(|container, index| {
            Box::new(FurnaceFuelSlot::new(container.clone(), index))
        }),
    );
    let result = builder.section_at(
        container.clone(),
        [2],
        SectionKind::custom(|container, index| {
            Box::new(FurnaceResultSlot::new(container.clone(), index))
        }),
    );
    let player = builder.player_inventory(&inventory);
    let data = from_fn(|_| builder.data_slot(0));

    builder.build(FurnaceMenuKind {
        container,
        kind,
        input,
        fuel,
        result,
        player,
        data,
    })
}

#[must_use]
const fn menu_type(kind: FurnaceKind) -> MenuTypeRef {
    match kind {
        FurnaceKind::Furnace => &vanilla_menu_types::FURNACE,
        FurnaceKind::BlastFurnace => &vanilla_menu_types::BLAST_FURNACE,
        FurnaceKind::Smoker => &vanilla_menu_types::SMOKER,
    }
}

/// State and shift-click behavior shared by all furnace-family menus.
pub struct FurnaceMenuKind {
    container: ContainerRef,
    kind: FurnaceKind,
    input: Section,
    fuel: Section,
    result: Section,
    player: PlayerInventorySections,
    data: [DataSlot; 4],
}

// SAFETY: This Steel-owned key uniquely identifies furnace-family menu behavior.
unsafe impl DowncastType for FurnaceMenuKind {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:menu/abstract_furnace");
}

impl FurnaceMenuKind {
    fn update_data(&self, behavior: &mut MenuBehavior, guard: &ContainerLockGuard) {
        let Some(container) = guard.get_typed::<FurnaceContainer>(self.container.container_id())
        else {
            return;
        };
        for (slot, value) in self.data.into_iter().zip(container.data()) {
            slot.set(behavior, value);
        }
    }

    fn can_smelt(&self, stack: &ItemStack) -> bool {
        REGISTRY
            .recipes
            .find_match(
                self.kind.recipe_type(),
                &SingleItemRecipeInput::new(stack.clone()),
            )
            .is_some()
    }
}

impl MenuKind for FurnaceMenuKind {
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

        let moved = if self.result.contains(slot_index) {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                self.player.all().start(),
                self.player.all().end(),
                FillDirection::Backward,
            )
        } else if self.input.contains(slot_index) || self.fuel.contains(slot_index) {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                self.player.all().start(),
                self.player.all().end(),
                FillDirection::Forward,
            )
        } else if self.can_smelt(&clicked) {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                self.input.start(),
                self.input.end(),
                FillDirection::Forward,
            )
        } else if VANILLA_FUEL_VALUES.is_fuel(clicked.item()) {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                self.fuel.start(),
                self.fuel.end(),
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
            false
        };

        if !moved {
            return Some(ItemStack::empty());
        }
        behavior.update_quick_move_source(guard, slot_index, &remaining, &clicked);
        if remaining.count() == clicked.count() {
            return Some(ItemStack::empty());
        }
        if let Some(remainder) = behavior.slots()[slot_index].on_take(guard, &remaining, player) {
            player.add_item_or_drop_with_guard(guard, remainder);
        }
        Some(clicked)
    }

    fn can_take_item_for_pick_all(&self, _carried: &ItemStack, slot_index: usize) -> bool {
        !self.result.contains(slot_index)
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_items};
    use steel_utils::locks::IntoShared as _;

    use super::*;
    use crate::inventory::container::SimpleContainer;

    #[test]
    fn manual_input_accepts_items_without_a_matching_recipe() {
        init_vanilla_registry();

        for (kind, item) in [
            (FurnaceKind::Furnace, &vanilla_items::FURNACE),
            (FurnaceKind::Smoker, &vanilla_items::RAW_IRON),
        ] {
            let stack = ItemStack::new(item);
            assert!(
                REGISTRY
                    .recipes
                    .find_match(
                        kind.recipe_type(),
                        &SingleItemRecipeInput::new(stack.clone()),
                    )
                    .is_none()
            );

            let menu = furnace_menu(
                PlayerInventory::new().into_shared(),
                1,
                ContainerRef::from(SimpleContainer::new(3).into_shared()),
                kind,
            );

            assert!(menu.behavior().slots()[0].may_place(&stack));
        }
    }
}
