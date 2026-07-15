//! The villager trade menu.

use std::{mem, sync::Arc};

use steel_registry::item_stack::ItemStack;
use steel_registry::menu_type::MenuTypeRef;
use steel_registry::vanilla_menu_types;
use steel_utils::locks::SyncMutex;
use text_components::TextComponent;

use crate::entity::{Mob, SharedEntity};
use crate::inventory::{
    MenuId, SyncPlayerInv,
    container::Container,
    lock::{ContainerLockGuard, ContainerRef},
    menu::{Menu, MenuBehavior},
    menu_provider::{MenuInstance, MenuProvider},
    merchant::MerchantContainer,
    slot::{
        MerchantResultSlot, NormalSlot, Slot, SlotType, SyncMerchantContainer,
        add_standard_inventory_slots,
    },
};
use crate::player::Player;
use crate::trading::SharedMerchantOffers;

/// Slot indices within the merchant menu.
pub mod slots {
    /// First payment slot
    pub const PAYMENT1_SLOT: usize = 0;
    /// Second payment slot
    pub const PAYMENT2_SLOT: usize = 1;
    /// Trade result slot
    pub const RESULT_SLOT: usize = 2;
    /// Start of main inventory
    pub const INV_SLOT_START: usize = 3;
    /// End of main inventory
    pub const INV_SLOT_END: usize = 30;
    /// Start of hotbar
    pub const USE_ROW_SLOT_START: usize = 30;
    /// End of hotbar
    pub const USE_ROW_SLOT_END: usize = 39;
    /// Total number of slots in trade menu
    pub const TOTAL_SLOTS: usize = 39;
}

/// The vilager trade menu.
pub struct MerchantMenu {
    behavior: MenuBehavior,
    trade_container: SyncMerchantContainer,
    merchant: SharedEntity,
}

impl MerchantMenu {
    /// Creates a merchant menu for the given offers and villager.
    #[must_use]
    pub fn new(
        inventory: SyncPlayerInv,
        container_id: MenuId,
        offers: SharedMerchantOffers,
        merchant: SharedEntity,
    ) -> Self {
        let trade_container: SyncMerchantContainer = Arc::new(SyncMutex::new(
            MerchantContainer::new(offers, Arc::clone(&merchant)),
        ));

        let mut menu_slots = Vec::with_capacity(slots::TOTAL_SLOTS);

        menu_slots.push(SlotType::Normal(NormalSlot::new(
            ContainerRef::from(Arc::clone(&trade_container)),
            slots::PAYMENT1_SLOT,
        )));
        menu_slots.push(SlotType::Normal(NormalSlot::new(
            ContainerRef::from(Arc::clone(&trade_container)),
            slots::PAYMENT2_SLOT,
        )));

        menu_slots.push(SlotType::MerchantResult(MerchantResultSlot::new(
            Arc::clone(&trade_container),
        )));

        add_standard_inventory_slots(&mut menu_slots, &inventory);

        Self {
            behavior: MenuBehavior::new(
                menu_slots,
                container_id,
                Some(&vanilla_menu_types::MERCHANT),
            ),
            trade_container,
            merchant,
        }
    }

    #[must_use]
    /// Returns the transient trade container backing this menu.
    pub const fn trade_container(&self) -> &SyncMerchantContainer {
        &self.trade_container
    }

    fn try_move_items(&mut self, index: i32) {
        let Ok(idx) = usize::try_from(index) else {
            return;
        };

        let (cost_a, cost_b) = {
            let container = self.trade_container.lock();
            let offers_arc = container.offers();
            let offers = offers_arc.lock();
            let Some(offer) = offers.get(idx) else {
                return;
            };
            (offer.cost_a().clone(), offer.cost_b().cloned())
        };

        let mut guard = self.behavior.lock_all_containers();

        for slot in [slots::PAYMENT1_SLOT, slots::PAYMENT2_SLOT] {
            let old = self.behavior.slots[slot].get_item(&guard).clone();
            if old.is_empty() {
                continue;
            }
            let mut moving = old;
            if !self.behavior.move_item_stack_to(
                &mut guard,
                &mut moving,
                slots::INV_SLOT_START,
                slots::USE_ROW_SLOT_END,
                true,
            ) {
                return;
            }
            self.behavior.slots[slot].set_item(&mut guard, moving);
        }

        if self.behavior.slots[slots::PAYMENT1_SLOT]
            .get_item(&guard)
            .is_empty()
            && self.behavior.slots[slots::PAYMENT2_SLOT]
                .get_item(&guard)
                .is_empty()
        {
            self.move_from_inventory_to_payment_slot(&mut guard, slots::PAYMENT1_SLOT, &cost_a);
            if let Some(cost_b) = &cost_b {
                self.move_from_inventory_to_payment_slot(&mut guard, slots::PAYMENT2_SLOT, cost_b);
            }
        }
    }

    fn move_from_inventory_to_payment_slot(
        &self,
        guard: &mut ContainerLockGuard,
        payment_slot: usize,
        cost: &ItemStack,
    ) {
        for i in slots::INV_SLOT_START..slots::USE_ROW_SLOT_END {
            let inv_item = self.behavior.slots[i].get_item(guard).clone();
            if inv_item.is_empty() || !ItemStack::is_same_item(&inv_item, cost) {
                continue;
            }
            let current = self.behavior.slots[payment_slot].get_item(guard).clone();
            if !current.is_empty() && !ItemStack::is_same_item_same_components(&inv_item, &current)
            {
                continue;
            }
            let max_stack = inv_item.max_stack_size();
            let move_count = (max_stack - current.count()).min(inv_item.count());
            if move_count <= 0 {
                continue;
            }
            let new_payment = inv_item.copy_with_count(current.count() + move_count);
            self.behavior.slots[i]
                .get_item_mut(guard)
                .shrink(move_count);
            self.behavior.slots[i].set_changed(guard);
            self.behavior.slots[payment_slot].set_item(guard, new_payment);
            if current.count() + move_count >= max_stack {
                break;
            }
        }
    }
}

impl Menu for MerchantMenu {
    fn behavior(&self) -> &MenuBehavior {
        &self.behavior
    }

    fn behavior_mut(&mut self) -> &mut MenuBehavior {
        &mut self.behavior
    }

    fn quick_move_stack(
        &mut self,
        guard: &mut ContainerLockGuard,
        slot_index: usize,
        player: &Player,
    ) -> ItemStack {
        if slot_index >= self.behavior.slots.len() {
            return ItemStack::empty();
        }

        let stack = self.behavior.slots[slot_index].get_item(guard).clone();
        if stack.is_empty() {
            return ItemStack::empty();
        }
        if slot_index == slots::RESULT_SLOT
            && !self.behavior.slots[slot_index].may_pickup(guard, player)
        {
            return ItemStack::empty();
        }

        let clicked = stack.clone();
        let mut stack_mut = stack;

        let moved = if slot_index == slots::RESULT_SLOT {
            if !self.behavior.move_item_stack_to(
                guard,
                &mut stack_mut,
                slots::INV_SLOT_START,
                slots::USE_ROW_SLOT_END,
                true,
            ) {
                return ItemStack::empty();
            }
            true
        } else if slot_index == slots::PAYMENT1_SLOT || slot_index == slots::PAYMENT2_SLOT {
            self.behavior.move_item_stack_to(
                guard,
                &mut stack_mut,
                slots::INV_SLOT_START,
                slots::USE_ROW_SLOT_END,
                false,
            )
        } else if (slots::INV_SLOT_START..slots::INV_SLOT_END).contains(&slot_index) {
            self.behavior.move_item_stack_to(
                guard,
                &mut stack_mut,
                slots::USE_ROW_SLOT_START,
                slots::USE_ROW_SLOT_END,
                false,
            )
        } else if (slots::USE_ROW_SLOT_START..slots::USE_ROW_SLOT_END).contains(&slot_index) {
            self.behavior.move_item_stack_to(
                guard,
                &mut stack_mut,
                slots::INV_SLOT_START,
                slots::INV_SLOT_END,
                false,
            )
        } else {
            false
        };

        if !moved {
            return ItemStack::empty();
        }

        self.behavior.slots[slot_index].set_item(guard, stack_mut.clone());

        if stack_mut.count == clicked.count {
            return ItemStack::empty();
        }

        self.behavior.slots[slot_index].set_changed(guard);

        #[expect(
            clippy::collapsible_if,
            reason = "the result branch will also play the trade sound"
        )]
        if slot_index == slots::RESULT_SLOT {
            if let Some(remainder) =
                self.behavior.slots[slot_index].on_take(guard, &clicked, player)
            {
                player.add_item_or_drop_with_guard(guard, remainder);
            }
            // TODO play villager trade sound.
        }

        clicked
    }

    fn can_take_item_for_pick_all(&self, _carried: &ItemStack, _slot_index: usize) -> bool {
        false
    }

    fn still_valid(&self, _player: &Player) -> bool {
        self.merchant.is_alive()
    }

    fn removed(&mut self, player: &Player) {
        let carried = mem::take(&mut self.behavior.carried);
        if !carried.is_empty() {
            player.add_item_or_drop(carried);
        }

        let payments: Vec<ItemStack> = {
            let mut container = self.trade_container.lock();
            [slots::PAYMENT1_SLOT, slots::PAYMENT2_SLOT]
                .into_iter()
                .map(|i| container.remove_item_no_update(i))
                .filter(|item| !item.is_empty())
                .collect()
        };
        for item in payments {
            player.add_item_or_drop(item);
        }

        self.trade_container
            .lock()
            .set_item(slots::RESULT_SLOT, ItemStack::empty());

        if let Some(villager) = self.merchant.as_mob().and_then(Mob::as_villager) {
            villager.set_trading_player(None);
        }
    }

    fn select_trade(&mut self, index: i32) {
        self.trade_container().lock().set_selection_hint(index);
        self.try_move_items(index);
    }
}

impl MenuInstance for MerchantMenu {
    fn menu_type(&self) -> MenuTypeRef {
        &vanilla_menu_types::MERCHANT
    }

    fn container_id(&self) -> MenuId {
        self.behavior.container_id
    }
}

/// Opens a [`MerchantMenu`] for a villager's offers.
pub struct MerchantMenuProvider {
    inventory: SyncPlayerInv,
    offers: SharedMerchantOffers,
    merchant: SharedEntity,
    title: TextComponent,
}

impl MerchantMenuProvider {
    /// Creates a provider that will open a merchant menu with these offers.
    #[must_use]
    pub const fn new(
        inventory: SyncPlayerInv,
        offers: SharedMerchantOffers,
        merchant: SharedEntity,
        title: TextComponent,
    ) -> Self {
        Self {
            inventory,
            offers,
            merchant,
            title,
        }
    }
}

impl MenuProvider for MerchantMenuProvider {
    fn title(&self) -> TextComponent {
        self.title.clone()
    }

    fn create(&self, container_id: MenuId) -> Box<dyn MenuInstance> {
        Box::new(MerchantMenu::new(
            self.inventory.clone(),
            container_id,
            Arc::clone(&self.offers),
            Arc::clone(&self.merchant),
        ))
    }
}
