//! Vilalger offers and profession trade tables.

use std::str::FromStr;

use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_villager_trades::VILLAGER_TRADES;
use steel_registry::villager_trade::VillagerTrade;
use steel_registry::{REGISTRY, RegistryExt};
use steel_utils::Identifier;

#[derive(Clone)]
pub struct MerchantOffer {
    cost_a: ItemStack,
    cost_b: Option<ItemStack>,
    result: ItemStack,
    uses: i32,
    max_uses: i32,
    xp: i32,
}

impl MerchantOffer {
    #[must_use]
    pub fn new(
        cost_a: ItemStack,
        cost_b: Option<ItemStack>,
        result: ItemStack,
        max_uses: i32,
        xp: i32,
    ) -> Self {
        Self {
            cost_a,
            cost_b,
            result,
            uses: 0,
            max_uses,
            xp,
        }
    }

    #[must_use]
    pub const fn cost_a(&self) -> &ItemStack {
        &self.cost_a
    }

    #[must_use]
    pub const fn cost_b(&self) -> Option<&ItemStack> {
        self.cost_b.as_ref()
    }

    #[must_use]
    pub const fn result(&self) -> &ItemStack {
        &self.result
    }

    #[must_use]
    pub const fn uses(&self) -> i32 {
        self.uses
    }

    #[must_use]
    pub const fn max_uses(&self) -> i32 {
        self.max_uses
    }

    #[must_use]
    pub const fn xp(&self) -> i32 {
        self.xp
    }

    #[must_use]
    pub const fn is_out_of_stock(&self) -> bool {
        self.uses >= self.max_uses
    }

    pub const fn increment_uses(&mut self) {
        self.uses += 1;
    }
}

pub type MerchantOffers = Vec<MerchantOffer>;

#[must_use]
pub fn offers_for(profession_key: &Identifier, level: i32) -> MerchantOffers {
    let Some(table) = VILLAGER_TRADES
        .iter()
        .find(|table| table.profession == profession_key.path.as_ref())
        else {
            return Vec::new();
        };
    let Ok(level_index) = usize::try_from(level - 1) else {
        return Vec::new();
    };
    let Some(trade_level) = table.levels.get(level_index) else {
        return Vec::new();
    };

    trade_level.trades.iter().filter_map(build_offer).collect()
}

fn build_offer(trade: &VillagerTrade) -> Option<MerchantOffer> {
    let cost_a = resolve_item(trade.wants, trade.wants_count)?;
    let cost_b = match trade.additional {
        Some((id, count)) => Some(resolve_item(id, count)?),
        None => None,
    };
    let result = resolve_item(trade.gives, trade.gives_count)?;
    Some(MerchantOffer::new(cost_a, cost_b, result, trade.max_uses, trade.xp))
}

fn resolve_item(key: &str, count: i32) -> Option<ItemStack> {
    let id = Identifier::from_str(key).ok()?;
    let item = REGISTRY.items.by_key(&id)?;
    Some(ItemStack::with_count(item, count))
}
