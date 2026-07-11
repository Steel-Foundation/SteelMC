//! Transient 3-slot container backing the villager trade menu.

use std::sync::Arc;
use steel_registry::item_stack::ItemStack;

use crate::entity::{Mob, SharedEntity};
use crate::inventory::container::Container;
use crate::trading::{MerchantOffer, SharedMerchantOffers};

/// Slot index of the first payment slot.
pub const PAYMENT1_SLOT: usize = 0;
/// Slot index of the second payment slot.
pub const PAYMENT2_SLOT: usize = 1;
/// Slot index of the trade result slot.
pub const RESULT_SLOT: usize = 2;

/// A transient 3-slot container that computes a trade result from two payment
/// slots against a villager's shared offer list. Based on Java's `MerchantContainer`.
pub struct MerchantContainer {
    items: [ItemStack; 3],
    offers: SharedMerchantOffers,
    merchant: SharedEntity,
    selection_hint: i32,
    active_offer: Option<usize>,
    future_xp: i32,
}

impl MerchantContainer {
    /// Creates a new merchant container for the given offers and villager.
    #[must_use]
    pub fn new(offers: SharedMerchantOffers, merchant: SharedEntity) -> Self {
        Self {
            items: [ItemStack::empty(), ItemStack::empty(), ItemStack::empty()],
            offers,
            merchant,
            selection_hint: -1,
            active_offer: None,
            future_xp: 0,
        }
    }

    /// Returns true when the payment slots match an in-stock trade.
    #[must_use]
    pub const fn has_active_offer(&self) -> bool {
        self.active_offer.is_some()
    }

    /// Returns a handle to the shared offer list.
    #[must_use]
    pub fn offers(&self) -> SharedMerchantOffers {
        Arc::clone(&self.offers)
    }

    /// Returns the XP a completed trade would grant.
    #[must_use]
    pub const fn future_xp(&self) -> i32 {
        self.future_xp
    }

    /// Sets the selected trade index and recomputes the result slot.
    pub fn set_selection_hint(&mut self, hint: i32) {
        self.selection_hint = hint;
        self.update_sell_item();
    }

    fn update_sell_item(&mut self) {
        self.active_offer = None;

        let (buy_a, buy_b) = if self.items[PAYMENT1_SLOT].is_empty() {
            (self.items[PAYMENT2_SLOT].clone(), ItemStack::empty())
        } else {
            (
                self.items[PAYMENT1_SLOT].clone(),
                self.items[PAYMENT2_SLOT].clone(),
            )
        };

        let offers = self.offers.lock();
        if offers.is_empty() {
            return;
        }

        let mut matched = recipe_for(offers.as_slice(), self.selection_hint, &buy_a, &buy_b);
        if matched.is_none_or(|i| offers[i].is_out_of_stock()) {
            self.active_offer = matched;
            matched = recipe_for(offers.as_slice(), self.selection_hint, &buy_b, &buy_a);
        }

        if let Some(i) = matched
            && !offers[i].is_out_of_stock()
        {
            let result = offers[i].assemble();
            let xp = offers[i].xp();
            drop(offers);
            self.active_offer = Some(i);
            self.items[RESULT_SLOT] = result;
            self.future_xp = xp;
        } else {
            drop(offers);
            self.items[RESULT_SLOT] = ItemStack::empty();
            self.future_xp = 0;
        }
    }

    /// Executes the active trade: consumes the payment, increments the offer's
    /// use count, refills the result slot, and notifies the villager.
    pub fn take_trade(&mut self) {
        let Some(index) = self.active_offer else {
            return;
        };

        let mut buy_a = self.items[PAYMENT1_SLOT].clone();
        let mut buy_b = self.items[PAYMENT2_SLOT].clone();

        let awarded_xp = {
            let mut offers = self.offers.lock();
            let Some(offer) = offers.get_mut(index) else {
                return;
            };
            let took = offer.take(&mut buy_a, &mut buy_b) || offer.take(&mut buy_b, &mut buy_a);

            if !took {
                return;
            }

            offer.increment_uses();
            offer.xp()
        };

        self.set_item(PAYMENT1_SLOT, buy_a);
        self.set_item(PAYMENT2_SLOT, buy_b);

        if let Some(villager) = self.merchant.as_mob().and_then(Mob::as_villager) {
            villager.notify_trade(awarded_xp);
        }
    }
}

fn recipe_for(
    offers: &[MerchantOffer],
    selection_hint: i32,
    a: &ItemStack,
    b: &ItemStack,
) -> Option<usize> {
    if let Ok(hint) = usize::try_from(selection_hint)
        && offers.get(hint).is_some_and(|o| o.satisfied_by(a, b))
    {
        return Some(hint);
    }
    offers.iter().position(|o| o.satisfied_by(a, b))
}

impl Container for MerchantContainer {
    fn get_container_size(&self) -> usize {
        self.items.len()
    }

    fn get_item(&self, slot: usize) -> &ItemStack {
        &self.items[slot]
    }

    fn get_item_mut(&mut self, slot: usize) -> &mut ItemStack {
        &mut self.items[slot]
    }

    fn set_item(&mut self, slot: usize, stack: ItemStack) {
        self.items[slot] = stack;
        if slot == PAYMENT1_SLOT || slot == PAYMENT2_SLOT {
            self.update_sell_item();
        }
    }

    fn set_changed(&mut self) {
        self.update_sell_item();
    }
}
