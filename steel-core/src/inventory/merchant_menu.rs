//! The villager trade menu.

use std::{mem, sync::Arc};

use steel_registry::item_stack::ItemStack;
use steel_registry::menu_type::MenuTypeRef;
use steel_registry::vanilla_menu_types;
use steel_utils::locks::SyncMutex;
use text_components::TextComponent;

use crate::entity::{Entity, Mob, SharedEntity};
use crate::inventory::{
    SyncPlayerInv,
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
use crate::trading::{MerchantOffer, MerchantOffers, SharedMerchantOffers};

pub mod slots {
    /// First payment slot
    pub const PAYMENT1_SLOT: usize = 0;
    /// Second payment slot
    pub const PAYMENT2_SLOT: usize = 1;
    /// Trade result slot
    pub const RESULT_SLOT: usize = 2;
    /// Start of main inventory
    pub const INV_SLOT_START: usize = 3;
    /// End fo main inventory
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
    #[must_use]
    pub fn new(
        inventory: SyncPlayerInv,
        container_id: u8,
        offers: SharedMerchantOffers,
        merchant: SharedEntity,
    ) -> Self {
        let trade_container: SyncMerchantContainer = Arc::new(SyncMutex::new(
            MerchantContainer::new(offers, Arc::clone(&merchant)),
        ));

        let mut menu_slots = Vec::with_capacity(slots::TOTAL_SLOTS);

        menu_slots.push(SlotType::Normal(NormalSlot::new(
            ContainerRef::MerchantContainer(Arc::clone(&trade_container)),
            slots::PAYMENT1_SLOT,
        )));
        menu_slots.push(SlotType::Normal(NormalSlot::new(
            ContainerRef::MerchantContainer(Arc::clone(&trade_container)),
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
    pub const fn trade_container(&self) -> &SyncMerchantContainer {
        &self.trade_container
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
            villager.set_trading_player(None)
        }
    }
}

impl MenuInstance for MerchantMenu {
    fn menu_type(&self) -> MenuTypeRef {
        &vanilla_menu_types::MERCHANT
    }

    fn container_id(&self) -> u8 {
        self.behavior.container_id
    }
}

pub struct MerchantMenuProvider {
    inventory: SyncPlayerInv,
    offers: SharedMerchantOffers,
    merchant: SharedEntity,
    title: TextComponent,
}

impl MerchantMenuProvider {
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

    fn create(&self, container_id: u8) -> Box<dyn MenuInstance> {
        Box::new(MerchantMenu::new(
            self.inventory.clone(),
            container_id,
            Arc::clone(&self.offers),
            Arc::clone(&self.merchant),
        ))
    }
}
