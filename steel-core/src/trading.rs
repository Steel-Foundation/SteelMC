//! Villager offers and profession trade tables.

use std::str::FromStr;
use std::sync::Arc;

use rand::seq::IteratorRandom;
use simdnbt::borrow::NbtCompound as NbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_villager_trades::VILLAGER_TRADES;
use steel_registry::villager_trade::VillagerTrade;
use steel_registry::{REGISTRY, RegistryExt};
use steel_utils::Identifier;
use steel_utils::locks::SyncMutex;

/// A single villager trade offer: up to two cost items for one result item,
/// with a use counter that limits how many times it can be traded before restock.
#[derive(Clone)]
pub struct MerchantOffer {
    cost_a: ItemStack,
    cost_b: Option<ItemStack>,
    result: ItemStack,
    uses: i32,
    max_uses: i32,
    xp: i32,
}

/// A villager's offer list, shared between the entity and any open trade menu.
pub type SharedMerchantOffers = Arc<SyncMutex<MerchantOffers>>;

impl MerchantOffer {
    /// Creates a new offer with a fresh (zero) use count.
    #[must_use]
    pub const fn new(
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

    /// The primary cost item.
    #[must_use]
    pub const fn cost_a(&self) -> &ItemStack {
        &self.cost_a
    }

    /// The optional secondary cost item.
    #[must_use]
    pub const fn cost_b(&self) -> Option<&ItemStack> {
        self.cost_b.as_ref()
    }

    /// The item produced by this trade.
    #[must_use]
    pub const fn result(&self) -> &ItemStack {
        &self.result
    }

    /// How many times this offer has been traded since the last restock.
    #[must_use]
    pub const fn uses(&self) -> i32 {
        self.uses
    }

    /// The maximum number of trades before this offer is out of stock.
    #[must_use]
    pub const fn max_uses(&self) -> i32 {
        self.max_uses
    }

    /// The experience granted to the villager per trade.
    #[must_use]
    pub const fn xp(&self) -> i32 {
        self.xp
    }

    /// Returns true if this offer has been used up (needs a restock).
    #[must_use]
    pub const fn is_out_of_stock(&self) -> bool {
        self.uses >= self.max_uses
    }

    /// Increments the use counter after a completed trade.
    pub const fn increment_uses(&mut self) {
        self.uses += 1;
    }

    /// Returns true if the payment stacks `a`/`b` meet this offer's costs.
    ///
    /// Item type is compared ignoring components (matching vanilla `ItemCost`
    /// with an empty predicate); each count must meet or exceed the cost.
    #[must_use]
    pub fn satisfied_by(&self, a: &ItemStack, b: &ItemStack) -> bool {
        if !ItemStack::is_same_item(a, &self.cost_a) || a.count() < self.cost_a.count() {
            return false;
        }
        match &self.cost_b {
            Some(cost_b) => ItemStack::is_same_item(b, cost_b) && b.count() >= cost_b.count(),
            None => b.is_empty(),
        }
    }

    /// Returns a fresh copy of this offer's result item.
    #[must_use]
    pub fn assemble(&self) -> ItemStack {
        self.result.clone()
    }

    /// Consumes this offer's costs from the payment stacks if they satisfy it.
    ///
    /// Returns true and shrinks `a`/`b` on success; leaves them untouched otherwise.
    pub fn take(&self, a: &mut ItemStack, b: &mut ItemStack) -> bool {
        if !self.satisfied_by(a, b) {
            return false;
        }
        a.shrink(self.cost_a.count());
        if let Some(cost_b) = &self.cost_b {
            b.shrink(cost_b.count());
        }
        true
    }

    /// Serializes this offer to a vanilla-style recipe NBT compound.
    #[must_use]
    pub fn to_nbt(&self) -> NbtCompound {
        let mut c = NbtCompound::new();
        c.insert("buy", self.cost_a.to_nbt_tag_ref());
        if let Some(cost_b) = &self.cost_b {
            c.insert("buyB", cost_b.to_nbt_tag_ref());
        }
        c.insert("sell", self.result.to_nbt_tag_ref());
        c.insert("uses", self.uses);
        c.insert("maxUses", self.max_uses());
        c.insert("xp", self.xp);
        c
    }

    /// Deserializes an offer from a recipe NBT compound, if valid.
    #[must_use]
    pub fn from_nbt(compound: &NbtCompoundView<'_, '_>) -> Option<Self> {
        let cost_a = compound
            .compound("buy")
            .and_then(|c| ItemStack::from_borrowed_compound(&c))?;
        let cost_b = compound
            .compound("buyB")
            .and_then(|c| ItemStack::from_borrowed_compound(&c));
        let result = compound
            .compound("sell")
            .and_then(|c| ItemStack::from_borrowed_compound(&c))?;
        Some(Self {
            cost_a,
            cost_b,
            result,
            uses: compound.int("uses").unwrap_or(0),
            max_uses: compound.int("maxUses").unwrap_or(0),
            xp: compound.int("xp").unwrap_or(0),
        })
    }

    /// Returns true if this offer has been used and could be restocked.
    #[must_use]
    pub const fn needs_restock(&self) -> bool {
        self.uses > 0
    }

    /// Resets the use counter to zero (a restock).
    pub const fn reset_uses(&mut self) {
        self.uses = 0;
    }
}

/// A villager's full list of trade offers.
pub type MerchantOffers = Vec<MerchantOffer>;

/// Rolls the trade offers for a profession at a given level.
///
/// Mirrors vanilla: a random selection of `amount` trades is drawn from the
/// level's pool (without replacement) rather than returning the whole pool.
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

    // Vanilla rolls `amount` trades at random from the level's pool (without
    // replacement) rather than offering the whole pool.
    let amount = usize::try_from(trade_level.amount).unwrap_or(0);
    trade_level
        .trades
        .iter()
        .filter_map(build_offer)
        .sample(&mut rand::rng(), amount)
}

fn build_offer(trade: &VillagerTrade) -> Option<MerchantOffer> {
    let cost_a = resolve_item(trade.wants, trade.wants_count)?;
    let cost_b = match trade.additional {
        Some((id, count)) => Some(resolve_item(id, count)?),
        None => None,
    };
    let result = resolve_item(trade.gives, trade.gives_count)?;
    Some(MerchantOffer::new(
        cost_a,
        cost_b,
        result,
        trade.max_uses,
        trade.xp,
    ))
}

fn resolve_item(key: &str, count: i32) -> Option<ItemStack> {
    let id = Identifier::from_str(key).ok()?;
    let item = REGISTRY.items.by_key(&id)?;
    Some(ItemStack::with_count(item, count))
}
