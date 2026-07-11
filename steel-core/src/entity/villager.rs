//! Villager capability trait

use steel_registry::entity_data::VillagerData;

use crate::{entity::Mob, trading::SharedMerchantOffers};

pub trait Villager: Mob {
    fn villager_data(&self) -> VillagerData;
    fn set_villager_data(&self, data: VillagerData);
    fn offers(&self) -> SharedMerchantOffers;
    fn updateTrades(&self);
    fn villager_xp(&self) -> i32;
    fn notify_trade(&self, xp: i32);
    fn is_trading(&self) -> bool;
    fn set_trading_player(&self, id: Option<i32>);
    fn try_restock(&self);
}
