use std::sync::Arc;

use steel_protocol::packets::game::{CMerchantOffers, ItemCost, MerchantOfferPacket};
use steel_registry::{item_stack::ItemStack, vanilla_menu_types};
use steel_utils::locks::{IntoShared, Shared, SyncMutex};

use crate::{
    entity::Entity,
    inventory::{
        container::{ResultContainer, SimpleContainer},
        prelude::*,
        slots::ResultHandler,
    },
    player::player_inventory::PlayerInventory,
    villager::MerchantOffer,
};

/// Builds the vanilla merchant menu: two payment slots, one result, and the player inventory.
#[must_use]
pub fn merchant(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    offers: Arc<SyncMutex<Vec<MerchantOffer>>>,
) -> Menu {
    let payment = SimpleContainer::new(2).into_shared();
    let result = ResultContainer::new().into_shared();
    let selected = Arc::new(SyncMutex::new(0));
    let handler = MerchantResultHandler {
        payment: payment.clone(),
        result: result.clone(),
        offers: offers.clone(),
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
        offers,
        result,
        payment,
        selected,
    })
}

pub struct MerchantKind {
    offers: Arc<SyncMutex<Vec<MerchantOffer>>>,
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

    fn on_open(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        player: &Player,
    ) {
        // === RUNTIME DEBUG INSTRUMENTATION ===
        let container_id = behavior.container_id();
        eprintln!("[MERCHANT] on_open called");
        eprintln!("[MERCHANT] container_id={}", container_id);
        eprintln!("[MERCHANT] player_id={}", player.id());

        self.send_offers(behavior.container_id(), player);
        eprintln!("[MERCHANT] send_offers completed");

        let handler = MerchantResultHandler {
            payment: self.payment.clone(),
            result: self.result.clone(),
            offers: self.offers.clone(),
            selected: self.selected.clone(),
        };
        handler.update_result(guard);
        eprintln!("[MERCHANT] update_result completed");
        eprintln!("[MERCHANT] on_open finished\n");
    }

    fn on_select_trade(&mut self, behavior: &mut MenuBehavior, offer: usize, player: &Player) {
        if offer >= self.offers.lock().len() {
            return;
        }
        *self.selected.lock() = offer;
        let handler = MerchantResultHandler {
            payment: self.payment.clone(),
            result: self.result.clone(),
            offers: self.offers.clone(),
            selected: self.selected.clone(),
        };
        let mut guard = behavior.lock_all_containers();
        handler.update_result(&mut guard);
        behavior.send_all_data_to_remote(&player.connection);
    }

    fn slots_changed(
        &mut self,
        _behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
        let handler = MerchantResultHandler {
            payment: self.payment.clone(),
            result: self.result.clone(),
            offers: self.offers.clone(),
            selected: self.selected.clone(),
        };
        handler.update_result(guard);
    }

    fn removed(&mut self, _behavior: &mut MenuBehavior, _player: &Player) {
        self.result.lock().set_item(0, ItemStack::empty());
    }
}

impl MerchantKind {
    fn send_offers(&self, container_id: u8, player: &Player) {
        let locked = self.offers.lock();
        let offers_vec: Vec<_> = locked.iter().map(to_packet).collect();

        // === RUNTIME DEBUG INSTRUMENTATION ===
        eprintln!("[MERCHANT] send_offers: preparing CMerchantOffers packet");
        eprintln!("[MERCHANT]   container_id: {}", container_id);
        eprintln!("[MERCHANT]   offers count: {}", offers_vec.len());
        for (index, offer) in locked.iter().enumerate() {
            let cost_b = match &offer.cost_b {
                Some(cost) => format!("{} x{}", cost.item().key, cost.count()),
                None => "absent".to_owned(),
            };
            eprintln!(
                "[MERCHANT]   offer {index}: cost_a={} x{} (empty={}), result={} x{} (empty={}), \
cost_b={cost_b}, uses={}, max_uses={}, xp={}, price_multiplier={}, out_of_stock={}",
                offer.cost_a.item().key,
                offer.cost_a.count(),
                offer.cost_a.is_empty(),
                offer.result.item().key,
                offer.result.count(),
                offer.result.is_empty(),
                offer.uses,
                offer.max_uses,
                offer.xp,
                offer.reputation_discount,
                offer.is_out_of_stock(),
            );
        }
        drop(locked);

        player.send_packet(CMerchantOffers {
            container_id: i32::from(container_id),
            offers: offers_vec,
            villager_level: 1,
            villager_xp: 0,
            show_progress: true,
            can_restock: true,
        });

        eprintln!(
            "[MERCHANT] CMerchantOffers packet sent to player {}",
            player.id()
        );
    }
}

struct MerchantResultHandler {
    payment: Shared<SimpleContainer>,
    result: Shared<ResultContainer>,
    offers: Arc<SyncMutex<Vec<MerchantOffer>>>,
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
        let offers = self.offers.lock();
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
        _player: &Player,
    ) -> Option<ItemStack> {
        let payment_id = ContainerId::from_arc(&self.payment);
        let result_id = ContainerId::from_arc(&self.result);
        let Some([payment, result]) = guard.get_disjoint_mut([payment_id, result_id]) else {
            return None;
        };
        let [a, b] = payment.items_mut() else {
            return None;
        };
        let mut offers = self.offers.lock();
        let selected = *self.selected.lock();
        let Some(offer) = offers
            .get_mut(selected)
            .filter(|offer| offer.can_trade(a, b))
        else {
            return None;
        };
        let _ = offer.take(a, b);
        result.set_item(0, ItemStack::empty());
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
        self.offers
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
