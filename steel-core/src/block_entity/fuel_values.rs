//! Furnace fuel durations derived from Vanilla's hardcoded transform.

use std::sync::OnceLock;

use steel_registry::blocks::BlockRef;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{
    REGISTRY, RegistryEntry as _, RegistryExt as _, TaggedRegistryExt as _, item_stack::ItemStack,
    vanilla_blocks, vanilla_items,
};
use steel_utils::Identifier;

const STANDARD_BURN_TIME: i32 = 200;

/// Burn durations indexed by item registry ID.
pub struct FuelValues {
    values: Vec<i32>,
}

impl FuelValues {
    /// Returns whether `stack` is accepted as furnace fuel.
    #[must_use]
    pub fn is_fuel(&self, stack: &ItemStack) -> bool {
        self.burn_duration(stack) > 0
    }

    /// Returns the number of ticks one item in `stack` burns for.
    #[must_use]
    pub fn burn_duration(&self, stack: &ItemStack) -> i32 {
        if stack.is_empty() {
            return 0;
        }
        self.values.get(stack.item().id()).copied().unwrap_or(0)
    }

    fn vanilla(base_unit: i32) -> Self {
        let mut builder = FuelValuesBuilder::new();
        builder.add_item(&vanilla_items::LAVA_BUCKET, base_unit * 100);
        builder.add_block(&vanilla_blocks::COAL_BLOCK, base_unit * 8 * 10);
        builder.add_item(&vanilla_items::BLAZE_ROD, base_unit * 12);
        builder.add_item(&vanilla_items::COAL, base_unit * 8);
        builder.add_item(&vanilla_items::CHARCOAL, base_unit * 8);
        builder.add_tag(&ItemTag::LOGS, base_unit * 3 / 2);
        builder.add_tag(&ItemTag::BAMBOO_BLOCKS, base_unit * 3 / 2);
        builder.add_tag(&ItemTag::PLANKS, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::BAMBOO_MOSAIC, base_unit * 3 / 2);
        builder.add_tag(&ItemTag::WOODEN_STAIRS, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::BAMBOO_MOSAIC_STAIRS, base_unit * 3 / 2);
        builder.add_tag(&ItemTag::WOODEN_SLABS, base_unit * 3 / 4);
        builder.add_block(&vanilla_blocks::BAMBOO_MOSAIC_SLAB, base_unit * 3 / 4);
        builder.add_tag(&ItemTag::WOODEN_TRAPDOORS, base_unit * 3 / 2);
        builder.add_tag(&ItemTag::WOODEN_PRESSURE_PLATES, base_unit * 3 / 2);
        builder.add_tag(&ItemTag::WOODEN_SHELVES, base_unit * 3 / 2);
        builder.add_tag(&ItemTag::WOODEN_FENCES, base_unit * 3 / 2);
        builder.add_tag(&ItemTag::FENCE_GATES, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::NOTE_BLOCK, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::BOOKSHELF, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::CHISELED_BOOKSHELF, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::LECTERN, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::JUKEBOX, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::CHEST, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::TRAPPED_CHEST, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::CRAFTING_TABLE, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::DAYLIGHT_DETECTOR, base_unit * 3 / 2);
        builder.add_tag(&ItemTag::BANNERS, base_unit * 3 / 2);
        builder.add_item(&vanilla_items::BOW, base_unit * 3 / 2);
        builder.add_item(&vanilla_items::FISHING_ROD, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::LADDER, base_unit * 3 / 2);
        builder.add_tag(&ItemTag::SIGNS, base_unit);
        builder.add_tag(&ItemTag::HANGING_SIGNS, base_unit * 4);
        builder.add_item(&vanilla_items::WOODEN_SHOVEL, base_unit);
        builder.add_item(&vanilla_items::WOODEN_SWORD, base_unit);
        builder.add_item(&vanilla_items::WOODEN_SPEAR, base_unit);
        builder.add_item(&vanilla_items::WOODEN_HOE, base_unit);
        builder.add_item(&vanilla_items::WOODEN_AXE, base_unit);
        builder.add_item(&vanilla_items::WOODEN_PICKAXE, base_unit);
        builder.add_tag(&ItemTag::WOODEN_DOORS, base_unit);
        builder.add_tag(&ItemTag::BOATS, base_unit * 6);
        builder.add_tag(&ItemTag::WOOL, base_unit / 2);
        builder.add_tag(&ItemTag::WOODEN_BUTTONS, base_unit / 2);
        builder.add_item(&vanilla_items::STICK, base_unit / 2);
        builder.add_tag(&ItemTag::SAPLINGS, base_unit / 2);
        builder.add_item(&vanilla_items::BOWL, base_unit / 2);
        builder.add_tag(&ItemTag::WOOL_CARPETS, 1 + base_unit / 3);
        builder.add_block(&vanilla_blocks::DRIED_KELP_BLOCK, 1 + base_unit * 20);
        builder.add_item(&vanilla_items::CROSSBOW, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::BAMBOO, base_unit / 4);
        builder.add_block(&vanilla_blocks::DEAD_BUSH, base_unit / 2);
        builder.add_block(&vanilla_blocks::SHORT_DRY_GRASS, base_unit / 2);
        builder.add_block(&vanilla_blocks::TALL_DRY_GRASS, base_unit / 2);
        builder.add_block(&vanilla_blocks::SCAFFOLDING, base_unit / 4);
        builder.add_block(&vanilla_blocks::LOOM, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::BARREL, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::CARTOGRAPHY_TABLE, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::FLETCHING_TABLE, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::SMITHING_TABLE, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::COMPOSTER, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::AZALEA, base_unit / 2);
        builder.add_block(&vanilla_blocks::FLOWERING_AZALEA, base_unit / 2);
        builder.add_block(&vanilla_blocks::MANGROVE_ROOTS, base_unit * 3 / 2);
        builder.add_block(&vanilla_blocks::LEAF_LITTER, base_unit / 2);
        builder.remove_tag(&ItemTag::NON_FLAMMABLE_WOOD);
        builder.build()
    }
}

struct FuelValuesBuilder {
    values: Vec<i32>,
}

impl FuelValuesBuilder {
    fn new() -> Self {
        Self {
            values: vec![0; REGISTRY.items.len()],
        }
    }

    fn add_item(&mut self, item: ItemRef, time: i32) {
        self.values[item.id()] = time;
    }

    fn add_block(&mut self, block: BlockRef, time: i32) {
        self.add_item(REGISTRY.items.by_block(block), time);
    }

    fn add_tag(&mut self, tag: &Identifier, time: i32) {
        for item in REGISTRY.items.iter_tag(tag) {
            self.values[item.id()] = time;
        }
    }

    fn remove_tag(&mut self, tag: &Identifier) {
        for item in REGISTRY.items.iter_tag(tag) {
            self.values[item.id()] = 0;
        }
    }

    fn build(self) -> FuelValues {
        FuelValues {
            values: self.values,
        }
    }
}

/// Returns the target-version Vanilla furnace fuel values.
#[must_use]
pub fn vanilla_fuel_values() -> &'static FuelValues {
    static VALUES: OnceLock<FuelValues> = OnceLock::new();
    VALUES.get_or_init(|| FuelValues::vanilla(STANDARD_BURN_TIME))
}

#[cfg(test)]
mod tests {
    use steel_registry::{item_stack::ItemStack, test_support::init_test_registry, vanilla_items};

    use super::vanilla_fuel_values;

    #[test]
    fn vanilla_transform_applies_direct_tagged_and_removed_fuels() {
        init_test_registry();
        let fuels = vanilla_fuel_values();

        assert_eq!(
            fuels.burn_duration(&ItemStack::new(&vanilla_items::LAVA_BUCKET)),
            20_000
        );
        assert_eq!(
            fuels.burn_duration(&ItemStack::new(&vanilla_items::OAK_LOG)),
            300
        );
        assert_eq!(
            fuels.burn_duration(&ItemStack::new(&vanilla_items::OAK_HANGING_SIGN)),
            800
        );
        assert_eq!(
            fuels.burn_duration(&ItemStack::new(&vanilla_items::WHITE_CARPET)),
            67
        );
        assert_eq!(
            fuels.burn_duration(&ItemStack::new(&vanilla_items::DRIED_KELP_BLOCK)),
            4_001
        );
        assert_eq!(
            fuels.burn_duration(&ItemStack::new(&vanilla_items::CRIMSON_PLANKS)),
            0
        );
        assert_eq!(
            fuels.burn_duration(&ItemStack::new(&vanilla_items::WARPED_STEM)),
            0
        );
    }
}
