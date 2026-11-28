//! Code-defined enchantment providers.

use steel_registry::enchantment::EnchantmentRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_enchantments;
use steel_utils::random::RandomSource;
use steel_utils::value_providers::IntProvider;

const MIN_ENCHANTMENT_LEVEL: i32 = 1;

/// Applies a generated level to an item's enchantments.
pub trait EnchantmentProvider {
    fn enchant(&self, item: &mut ItemStack, random: &mut RandomSource);
}

/// Applies one enchantment with a generated level.
pub struct SingleEnchantmentProvider {
    enchantment: EnchantmentRef,
    level: IntProvider,
}

impl SingleEnchantmentProvider {
    #[must_use]
    pub const fn new(enchantment: EnchantmentRef, level: IntProvider) -> Self {
        Self { enchantment, level }
    }
}

impl EnchantmentProvider for SingleEnchantmentProvider {
    fn enchant(&self, item: &mut ItemStack, random: &mut RandomSource) {
        let sampled = self.level.sample(random);
        let level = sampled.clamp(MIN_ENCHANTMENT_LEVEL, self.enchantment.max_level as i32) as u32;
        item.set_enchantments(&[(self.enchantment.key.clone(), level)], false);
    }
}

/// Silk Touch I applied to blocks dropped by an enderman.
pub static ENDERMAN_LOOT_DROP: SingleEnchantmentProvider = SingleEnchantmentProvider::new(
    &vanilla_enchantments::SILK_TOUCH,
    IntProvider::Constant(MIN_ENCHANTMENT_LEVEL),
);

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_enchantments, vanilla_items};
    use steel_utils::random::RandomSource;
    use steel_utils::random::legacy_random::LegacyRandom;

    use super::*;

    #[test]
    fn single_provider_sets_silk_touch_one() {
        init_vanilla_registry();
        let mut item = ItemStack::new(&vanilla_items::DIAMOND_AXE);
        let mut random = RandomSource::Legacy(LegacyRandom::from_seed(1));

        ENDERMAN_LOOT_DROP.enchant(&mut item, &mut random);

        assert_eq!(
            item.get_enchantment_level(&vanilla_enchantments::SILK_TOUCH.key),
            1
        );
    }
}
