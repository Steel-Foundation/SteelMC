//! temp container for the villager trade menu.

use steel_registry::item_stack::ItemStack;

use crate::inventory::container::Container;
use crate::trading::{MerchantOffer, MerchantOffers};

pub const PAYMENT1_SLOT: usize = 0;
pub const PAYMENT2_SLOT: usize = 1;
pub const RESULT_SLOT: usize = 2;

pub struct MerchantContainer {
    items: [ItemStack; 3],
    offers: MerchantOffers,
    selection_hint: i32,
    active_offer: Option<usize>,
    future_xp: i32,
}

impl MerchantContainer {
    #[must_use]
    pub fn new(offers: MerchantOffers) -> Self {
        Self {
            items: [ItemStack::empty(), ItemStack::empty(), ItemStack::empty()],
            offers,
            selection_hint: -1,
            active_offer: None,
            future_xp: 0,
        }
    }

    #[must_use]
    pub fn active_offer(&self) -> Option<&MerchantOffer> {
        self.active_offer.map(|i| &self.offers[i])
    }

    #[must_use]
    pub const fn offers(&self) -> &MerchantOffers {
        &self.offers
    }

    #[must_use]
    pub const fn future_xp(&self) -> i32 {
        self.future_xp
    }

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

        if self.offers.is_empty() {
            return;
        }

        let mut matched = self.recipe_for(&buy_a, &buy_b);
        if matched.is_none_or(|i| self.offers[i].is_out_of_stock()) {
            self.active_offer = matched;
            matched = self.recipe_for(&buy_b, &buy_a);
        }

        if let Some(i) = matched
            && !self.offers[i].is_out_of_stock()
        {
            self.active_offer = Some(i);
            self.items[RESULT_SLOT] = self.offers[i].assemble();
            self.future_xp = self.offers[i].xp();
        } else {
            self.items[RESULT_SLOT] = ItemStack::empty();
            self.future_xp = 0;
        }
    }

    fn recipe_for(&self, a: &ItemStack, b: &ItemStack) -> Option<usize> {
        if let Ok(hint) = usize::try_from(self.selection_hint)
            && self.offers.get(hint).is_some_and(|o| o.satisfied_by(a, b))
        {
            return Some(hint);
        }
        self.offers.iter().position(|o| o.satisfied_by(a, b))
    }

    pub fn take_trade(&mut self) {
        let Some(index) = self.active_offer else {
            return;
        };

        let mut buy_a = self.items[PAYMENT1_SLOT].clone();
        let mut buy_b = self.items[PAYMENT2_SLOT].clone();

        let took = {
            let offer = &self.offers[index];
            offer.take(&mut buy_a, &mut buy_b) || offer.take(&mut buy_b, &mut buy_a)
        };

        if took {
            self.offers[index].increment_uses();
            self.set_item(PAYMENT1_SLOT, buy_a);
            self.set_item(PAYMENT2_SLOT, buy_b);
        }
    }
}

impl Container for MerchantContainer {
    fn get_container_size(&self) -> usize {
        self.items.len()
    }

    fn get_item(&self, slot: usize) -> &ItemStack {
        &self.items[slot]
    }

    fn get_item_mut(&mut self,slot: usize) ->  &mut ItemStack {
        &mut self.items[slot]
    }

    fn set_item(&mut self,slot: usize,stack: ItemStack) {
        self.items[slot] = stack;
        if slot == PAYMENT1_SLOT || slot == PAYMENT2_SLOT {
            self.update_sell_item();
        }
    }

    fn set_changed(&mut self) {
        self.update_sell_item();
    }
}
