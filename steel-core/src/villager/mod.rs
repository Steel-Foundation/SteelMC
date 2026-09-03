//! Vanilla villager data and trading foundations.

#[doc(hidden)]
pub mod generated {
    #[rustfmt::skip]
    include!(concat!(env!("OUT_DIR"), "/villager_trades.rs"));
    #[rustfmt::skip]
    include!(concat!(env!("OUT_DIR"), "/biome_spawns.rs"));
}

pub use generated::{
    BIOME_SPAWNS, BiomeSpawnData, ProfessionTrades, SpawnData, TradeData, TradeItem,
    VILLAGER_TRADES,
};

use steel_registry::{REGISTRY, RegistryExt, item_stack::ItemStack};
use steel_utils::Identifier;

/// A server-side villager offer resolved against the current item registry.
#[derive(Debug, Clone)]
pub struct MerchantOffer {
    /// The first (required) cost item stack.
    pub cost_a: ItemStack,
    /// The optional second cost item stack (vanilla `costB`).
    pub cost_b: Option<ItemStack>,
    /// The item stack given in exchange for the costs.
    pub result: ItemStack,
    /// Maximum times this offer can be used before it is exhausted.
    pub max_uses: i32,
    /// Experience granted to the player on a completed trade.
    pub xp: i32,
    /// How strongly villager reputation discounts this offer's costs.
    pub reputation_discount: f32,
    /// Times this offer has been used so far.
    pub uses: i32,
}

impl MerchantOffer {
    /// Resolves an extracted trade into registry-backed item stacks.
    pub fn from_data(data: &TradeData) -> Option<Self> {
        Some(Self {
            cost_a: resolve_item(data.wants)?,
            cost_b: match data.additional_wants {
                Some(item) => Some(resolve_item(item)?),
                None => None,
            },
            result: resolve_item(data.gives)?,
            max_uses: i32::try_from(data.max_uses).ok()?.max(1),
            xp: i32::try_from(data.xp).ok()?.max(0),
            reputation_discount: data.reputation_discount.max(0.0),
            uses: 0,
        })
    }

    /// Returns whether this is out_of_stock.
    pub fn is_out_of_stock(&self) -> bool {
        self.uses >= self.max_uses
    }
    /// can_trade.
    pub fn can_trade(&self, a: &ItemStack, b: &ItemStack) -> bool {
        !self.is_out_of_stock()
            && same_item(a, &self.cost_a)
            && a.count() >= self.cost_a.count()
            && match (&self.cost_b, b.is_empty()) {
                (None, true) => true,
                (Some(cost), false) => same_item(b, cost) && b.count() >= cost.count(),
                _ => false,
            }
    }
    /// take.
    pub fn take(&mut self, a: &mut ItemStack, b: &mut ItemStack) -> bool {
        if !self.can_trade(a, b) {
            return false;
        }
        a.shrink(self.cost_a.count());
        if let Some(cost) = &self.cost_b {
            b.shrink(cost.count());
        }
        self.uses += 1;
        true
    }
}

fn resolve_item(item: TradeItem) -> Option<ItemStack> {
    let Some(key) = item.id.parse::<Identifier>().ok() else {
        log::warn!(
            "dropping villager trade with unparseable item id {}",
            item.id
        );
        return None;
    };
    let Some(item_ref) = REGISTRY.items.by_key(&key) else {
        log::warn!(
            "dropping villager trade referencing unknown item {}",
            item.id
        );
        return None;
    };
    // Zero/negative counts would serialize as an empty stack, which vanilla
    // clients reject while decoding merchant offers.
    if item.count == 0 {
        log::warn!("dropping villager trade with zero count for {}", item.id);
        return None;
    }
    Some(ItemStack::with_count(
        item_ref,
        i32::try_from(item.count).ok()?,
    ))
}

fn same_item(a: &ItemStack, b: &ItemStack) -> bool {
    !a.is_empty() && a.item() == b.item()
}

/// Vanilla `VillagerData.MIN_VILLAGER_LEVEL`.
pub const MIN_VILLAGER_LEVEL: i32 = 1;
/// Vanilla `VillagerData.MAX_VILLAGER_LEVEL`.
pub const MAX_VILLAGER_LEVEL: i32 = 5;
/// Vanilla `VillagerData.NEXT_LEVEL_XP_THRESHOLDS`.
pub const NEXT_LEVEL_XP_THRESHOLDS: [i32; 5] = [0, 10, 70, 150, 250];

/// Vanilla `VillagerData.canLevelUp`.
#[must_use]
pub const fn can_level_up(level: i32) -> bool {
    level >= MIN_VILLAGER_LEVEL && level < MAX_VILLAGER_LEVEL
}

/// Vanilla `VillagerData.getMaxXpPerLevel` — XP needed to leave `level`.
#[must_use]
pub const fn max_xp_per_level(level: i32) -> i32 {
    if can_level_up(level) {
        NEXT_LEVEL_XP_THRESHOLDS[level as usize]
    } else {
        0
    }
}

/// Returns the extracted trade group for a profession and exactly one career level.
///
/// Vanilla `Villager.updateTrades` only rolls the current level's trade set and
/// appends those offers; lower-level trades are already on the merchant.
#[must_use]
pub fn trade_groups_for(
    profession: &str,
    tier: u8,
) -> impl Iterator<Item = &'static ProfessionTrades> {
    VILLAGER_TRADES
        .iter()
        .filter(move |group| group.profession == profession && group.tier == tier)
}

/// Builds the selectable offers for one villager using a stable entity seed.
/// Offers whose extracted modifier pipeline is not yet representable by the item-component
/// foundation are excluded instead of producing a different item silently.
///
/// TODO: want-side component predicates are not yet extracted into
/// [`TradeData`], so trades like the wandering trader's water-bottle buy accept
/// any potion of that item instead of only the exact required components.
pub fn offers_for_seed(profession: &str, tier: u8, seed: u64) -> Vec<MerchantOffer> {
    let mut random = steel_utils::random::xoroshiro::Xoroshiro::from_seed(seed);
    offers_from_random(profession, tier, &mut random)
}

/// Builds offers for a profession and level using the caller's RNG source.
///
/// Vanilla selects offers with the villager's unseeded `random`, so every profession
/// (re)assignment rolls fresh trades.
pub fn offers_from_random(
    profession: &str,
    tier: u8,
    random: &mut impl steel_utils::random::Random,
) -> Vec<MerchantOffer> {
    trade_groups_for(profession, tier)
        .flat_map(|group| select_offers(group, random))
        .filter(|data| !data.has_item_modifiers)
        .filter_map(MerchantOffer::from_data)
        .collect()
}

/// Selects the vanilla number of offers from one extracted trade set.
/// The caller owns the persistent RNG state for the entity/world.
pub fn select_offers<'a>(
    group: &'a ProfessionTrades,
    random: &mut impl steel_utils::random::Random,
) -> Vec<&'a TradeData> {
    let mut remaining = group.trades.iter().collect::<Vec<_>>();
    let amount = usize::from(group.amount).min(remaining.len());
    let mut selected = Vec::with_capacity(amount);
    for _ in 0..amount {
        let index = random
            .next_i32_bounded(i32::try_from(remaining.len()).expect("trade set exceeds i32"))
            as usize;
        selected.push(remaining.swap_remove(index));
    }
    selected
}

/// Returns whether a spawn category is water-only according to vanilla's spawn rules.
#[must_use]
pub fn requires_water(category: &str, entity_type: &str) -> bool {
    matches!(category, "water_ambient" | "water_creature" | "axolotls")
        || matches!(
            entity_type,
            "minecraft:cod"
                | "minecraft:salmon"
                | "minecraft:pufferfish"
                | "minecraft:tropical_fish"
                | "minecraft:squid"
                | "minecraft:glow_squid"
                | "minecraft:axolotl"
                | "minecraft:dolphin"
                | "minecraft:guardian"
                | "minecraft:elder_guardian"
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_utils::random::xoroshiro::Xoroshiro;

    #[test]
    fn trade_selection_is_bounded_and_seeded() {
        let group = VILLAGER_TRADES
            .iter()
            .find(|group| group.profession == "farmer" && group.tier == 1)
            .expect("farmer tier one extracted");
        let mut first = Xoroshiro::from_seed(42);
        let mut second = Xoroshiro::from_seed(42);
        let a = select_offers(group, &mut first)
            .iter()
            .map(|trade| trade.gives.id)
            .collect::<Vec<_>>();
        let b = select_offers(group, &mut second)
            .iter()
            .map(|trade| trade.gives.id)
            .collect::<Vec<_>>();
        assert_eq!(a, b);
        assert_eq!(a.len(), 2);
    }

    #[test]
    fn water_categories_require_water() {
        assert!(requires_water("water_ambient", "minecraft:cod"));
        assert!(requires_water("creature", "minecraft:cod"));
        assert!(!requires_water("creature", "minecraft:cow"));
    }

    #[test]
    fn unemployed_and_nitwit_villagers_have_no_trade_groups() {
        // The trading gate relies on `none`/`nitwit` having no extracted trade sets while a real
        // profession does, so an unemployed villager can never open a trading screen.
        assert_eq!(trade_groups_for("none", 1).count(), 0);
        assert_eq!(trade_groups_for("nitwit", 1).count(), 0);
        assert_ne!(trade_groups_for("farmer", 1).count(), 0);
    }

    #[test]
    fn trade_groups_are_the_current_tier_only() {
        // Vanilla `updateTrades` rolls one level at a time. Selecting every
        // `tier <= current` would duplicate novice offers when the villager levels up.
        assert_eq!(trade_groups_for("farmer", 1).count(), 1);
        assert_eq!(trade_groups_for("farmer", 2).count(), 1);
        assert_eq!(trade_groups_for("farmer", 2).next().unwrap().tier, 2);
    }
}
