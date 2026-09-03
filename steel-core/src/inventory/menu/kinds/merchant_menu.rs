use std::sync::Arc;

use steel_protocol::packets::game::{CMerchantOffers, ItemCost, MerchantOfferPacket};
use steel_registry::{item_stack::ItemStack, vanilla_menu_types};
use steel_utils::locks::{IntoShared, Shared, SyncMutex};

use crate::{
    inventory::{
        container::{ResultContainer, SimpleContainer},
        prelude::*,
        slots::ResultHandler,
    },
    player::player_inventory::PlayerInventory,
    villager::MerchantOffer,
};

/// Server-side merchant the trading menu reads and notifies.
///
/// Villagers implement this so the menu can send live level/XP and call
/// vanilla `Merchant.notifyTrade` without depending on the concrete entity type.
pub trait MerchantAccess: Send + Sync {
    /// Shared offer list mutated when a trade is taken.
    fn offers(&self) -> Arc<SyncMutex<Vec<MerchantOffer>>>;
    /// Vanilla `Merchant.getVillagerXp` career progress shown in the trade GUI.
    fn villager_xp(&self) -> i32;
    /// Vanilla villager career level sent with merchant offers (1–5).
    fn villager_level(&self) -> i32;
    /// Vanilla `Merchant.notifyTrade` after a completed exchange.
    fn notify_trade(&self, player: &Player, offer_xp: i32);
    /// Vanilla `AbstractVillager.stopTrading` when the menu closes.
    fn stop_trading(&self);
    /// Vanilla `MerchantMenu.stillValid`.
    fn still_valid(&self, player: &Player) -> bool;
    /// Vanilla `Merchant.showProgressBar`.
    fn show_progress(&self) -> bool {
        true
    }
    /// Vanilla `Merchant.canRestock`.
    fn can_restock(&self) -> bool {
        true
    }
}

/// Builds the vanilla merchant menu: two payment slots, one result, and the player inventory.
#[must_use]
pub fn merchant(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    merchant: Arc<dyn MerchantAccess>,
) -> Menu {
    let payment = SimpleContainer::new(2).into_shared();
    let result = ResultContainer::new().into_shared();
    let selected = Arc::new(SyncMutex::new(0));
    let handler = MerchantResultHandler {
        payment: payment.clone(),
        result: result.clone(),
        merchant: Arc::clone(&merchant),
        selected: selected.clone(),
    };
    let mut builder = MenuBuilder::new(&vanilla_menu_types::MERCHANT, container_id);
    let payment_section = builder.section_all(payment.clone());
    let result_section = builder.result_slot(handler);
    let player = builder.player_inventory(&inventory);
    builder.route(result_section, player.all(), FillDirection::Backward);
    builder.route(payment_section, player.all(), FillDirection::Forward);
    builder.route(player.all(), payment_section, FillDirection::Forward);
    builder.build(MerchantKind {
        merchant,
        result,
        payment,
        selected,
    })
}

/// The merchant kind type.
pub struct MerchantKind {
    merchant: Arc<dyn MerchantAccess>,
    result: Shared<ResultContainer>,
    payment: Shared<SimpleContainer>,
    selected: Arc<SyncMutex<usize>>,
}

unsafe impl steel_utils::DowncastType for MerchantKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/merchant");
}

impl MenuKind for MerchantKind {
    fn can_take_item_for_pick_all(&self, _carried: &ItemStack, _slot_index: usize) -> bool {
        false
    }

    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        self.merchant.still_valid(player)
    }

    fn on_open(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        player: &Player,
    ) {
        self.send_offers(behavior.container_id(), player);

        let handler = MerchantResultHandler {
            payment: self.payment.clone(),
            result: self.result.clone(),
            merchant: Arc::clone(&self.merchant),
            selected: self.selected.clone(),
        };
        handler.update_result(guard);
    }

    fn on_select_trade(&mut self, behavior: &mut MenuBehavior, offer: usize, player: &Player) {
        if offer >= self.merchant.offers().lock().len() {
            return;
        }
        *self.selected.lock() = offer;
        let handler = MerchantResultHandler {
            payment: self.payment.clone(),
            result: self.result.clone(),
            merchant: Arc::clone(&self.merchant),
            selected: self.selected.clone(),
        };
        let mut guard = behavior.lock_all_containers();
        handler.update_result(&mut guard);
        behavior.send_all_data_to_remote(&player.connection);
    }

    fn slots_changed(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        player: &Player,
    ) {
        let handler = MerchantResultHandler {
            payment: self.payment.clone(),
            result: self.result.clone(),
            merchant: Arc::clone(&self.merchant),
            selected: self.selected.clone(),
        };
        handler.update_result(guard);
        // After a completed trade the offer uses and villager XP have changed;
        // resend so the client XP bar and out-of-stock marks stay in sync.
        self.send_offers(behavior.container_id(), player);
    }

    fn removed(&mut self, _behavior: &mut MenuBehavior, _player: &Player) {
        self.result.lock().set_item(0, ItemStack::empty());
        self.merchant.stop_trading();
    }
}

impl MerchantKind {
    fn send_offers(&self, container_id: u8, player: &Player) {
        let offers_vec: Vec<_> = self
            .merchant
            .offers()
            .lock()
            .iter()
            .map(to_packet)
            .collect();

        player.send_packet(CMerchantOffers {
            container_id: i32::from(container_id),
            offers: offers_vec,
            villager_level: self.merchant.villager_level(),
            villager_xp: self.merchant.villager_xp(),
            show_progress: self.merchant.show_progress(),
            can_restock: self.merchant.can_restock(),
        });
    }
}

struct MerchantResultHandler {
    payment: Shared<SimpleContainer>,
    result: Shared<ResultContainer>,
    merchant: Arc<dyn MerchantAccess>,
    selected: Arc<SyncMutex<usize>>,
}

impl ResultHandler for MerchantResultHandler {
    fn result_container(&self) -> ContainerRef {
        self.result.clone().into()
    }
    fn dependencies(&self) -> Vec<ContainerRef> {
        vec![self.payment.clone().into()]
    }
    fn update_result(&self, guard: &mut ContainerLockGuard) {
        let payment_id = ContainerId::from_arc(&self.payment);
        let result_id = ContainerId::from_arc(&self.result);
        let Some([payment, result]) = guard.get_disjoint_mut([payment_id, result_id]) else {
            return;
        };
        let [a, b] = payment.items() else {
            return;
        };
        let offers = self.merchant.offers();
        let offers = offers.lock();
        let selected = *self.selected.lock();
        let Some(offer) = offers.get(selected).filter(|offer| offer.can_trade(a, b)) else {
            result.set_item(0, ItemStack::empty());
            return;
        };
        result.set_item(0, offer.result.clone());
    }
    fn on_result_taken(
        &self,
        guard: &mut ContainerLockGuard,
        player: &Player,
    ) -> Option<ItemStack> {
        let payment_id = ContainerId::from_arc(&self.payment);
        let result_id = ContainerId::from_arc(&self.result);
        let Some([payment, result]) = guard.get_disjoint_mut([payment_id, result_id]) else {
            return None;
        };
        let [a, b] = payment.items_mut() else {
            return None;
        };
        let offer_xp = {
            let offers = self.merchant.offers();
            let mut offers = offers.lock();
            let selected = *self.selected.lock();
            let Some(offer) = offers
                .get_mut(selected)
                .filter(|offer| offer.can_trade(a, b))
            else {
                return None;
            };
            let xp = offer.xp;
            if !offer.take(a, b) {
                return None;
            }
            xp
        };
        result.set_item(0, ItemStack::empty());
        self.merchant.notify_trade(player, offer_xp);
        None
    }
    fn is_result_valid(&self, guard: &ContainerLockGuard, _player: &Player) -> bool {
        let payment_id = ContainerId::from_arc(&self.payment);
        let Some(payment) = guard.get(payment_id) else {
            return false;
        };
        let [a, b] = payment.items() else {
            return false;
        };
        let selected = *self.selected.lock();
        self.merchant
            .offers()
            .lock()
            .get(selected)
            .is_some_and(|offer| offer.can_trade(a, b))
    }
}

fn to_packet(offer: &MerchantOffer) -> MerchantOfferPacket {
    MerchantOfferPacket {
        cost_a: ItemCost::from_stack(&offer.cost_a),
        result: offer.result.clone(),
        cost_b: offer.cost_b.as_ref().map(ItemCost::from_stack),
        out_of_stock: offer.is_out_of_stock(),
        uses: offer.uses,
        max_uses: offer.max_uses,
        xp: offer.xp,
        special_price_diff: 0,
        price_multiplier: offer.reputation_discount,
        demand: 0,
    }
}
