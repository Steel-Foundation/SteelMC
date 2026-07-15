//! Villager capability trait

use steel_registry::entity_data::VillagerData;

use crate::{
    entity::{EntityId, Mob},
    trading::SharedMerchantOffers,
};

/// Villager-specific behaviour, reached from brain behaviors via [`Mob::as_villager`].
pub trait Villager: Mob {
    /// Returns the villager's data (biome type, profession, level).
    fn villager_data(&self) -> VillagerData;
    /// Sets the villager's data (biome type, profession, level).
    fn set_villager_data(&self, data: VillagerData);
    /// Returns the shared list of trade offers.
    fn offers(&self) -> SharedMerchantOffers;
    /// Adds trade offers for the current profession and level.
    fn update_trades(&self);
    /// Returns the villager's accumulated trading experience.
    fn villager_xp(&self) -> i32;
    /// Records a completed trade, awarding XP and scheduling any level-up.
    fn notify_trade(&self, xp: i32);
    /// Returns true while a player has this villager's trade menu open.
    fn is_trading(&self) -> bool;
    /// Sets (or clears) the id of the player currently trading with this villager.
    fn set_trading_player(&self, id: Option<EntityId>);
    /// Restocks trades if the villager is due for a restock.
    fn try_restock(&self);
}
