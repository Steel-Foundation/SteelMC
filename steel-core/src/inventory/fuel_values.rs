//! Furnace fuel durations.

use std::sync::LazyLock;

use steel_registry::items::{Item, ItemRef};
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::vanilla_items;
use steel_registry::{REGISTRY, RegistryEntry as _, RegistryExt as _, TaggedRegistryExt as _};
use steel_utils::Identifier;

/// Fuel durations indexed by item registry ID.
pub struct FuelValues {
    burn_durations: Vec<i32>,
}

impl FuelValues {
    /// Creates an empty table sized for the currently frozen item registry.
    #[must_use]
    pub fn builder() -> FuelValuesBuilder {
        FuelValuesBuilder {
            burn_durations: vec![0; REGISTRY.items.len()],
        }
    }

    /// Returns whether `item` is furnace fuel.
    #[must_use]
    pub fn is_fuel(&self, item: ItemRef) -> bool {
        self.burn_duration(item) > 0
    }

    /// Returns the item's burn duration in game ticks, or zero when it is not fuel.
    #[must_use]
    pub fn burn_duration(&self, item: ItemRef) -> i32 {
        self.burn_durations.get(item.id()).copied().unwrap_or(0)
    }
}

/// Mutable construction phase for [`FuelValues`].
pub struct FuelValuesBuilder {
    burn_durations: Vec<i32>,
}

impl FuelValuesBuilder {
    /// Assigns one item a burn duration.
    #[must_use]
    pub fn add(mut self, item: ItemRef, burn_duration: i32) -> Self {
        self.put(item, burn_duration);
        self
    }

    /// Assigns every item in a tag a burn duration.
    #[must_use]
    pub fn add_tag(mut self, tag: &Identifier, burn_duration: i32) -> Self {
        for item in REGISTRY.items.iter_tag(tag) {
            self.put(item, burn_duration);
        }
        self
    }

    /// Removes every item in a tag from the table.
    #[must_use]
    pub fn remove_tag(mut self, tag: &Identifier) -> Self {
        for item in REGISTRY.items.iter_tag(tag) {
            self.put(item, 0);
        }
        self
    }

    /// Finishes the immutable table.
    #[must_use]
    pub fn build(self) -> FuelValues {
        FuelValues {
            burn_durations: self.burn_durations,
        }
    }

    fn put(&mut self, item: &'static Item, burn_duration: i32) {
        assert!(burn_duration >= 0, "fuel duration cannot be negative");
        self.burn_durations[item.id()] = burn_duration;
    }
}

/// Vanilla 26.2 fuel values, built from the generated item registry and tags.
pub static VANILLA_FUEL_VALUES: LazyLock<FuelValues> = LazyLock::new(|| {
    const BASE: i32 = 200;

    FuelValues::builder()
        .add(&vanilla_items::LAVA_BUCKET, BASE * 100)
        .add(&vanilla_items::COAL_BLOCK, BASE * 8 * 10)
        .add(&vanilla_items::BLAZE_ROD, BASE * 12)
        .add(&vanilla_items::COAL, BASE * 8)
        .add(&vanilla_items::CHARCOAL, BASE * 8)
        .add_tag(&ItemTag::LOGS, BASE * 3 / 2)
        .add_tag(&ItemTag::BAMBOO_BLOCKS, BASE * 3 / 2)
        .add_tag(&ItemTag::PLANKS, BASE * 3 / 2)
        .add(&vanilla_items::BAMBOO_MOSAIC, BASE * 3 / 2)
        .add_tag(&ItemTag::WOODEN_STAIRS, BASE * 3 / 2)
        .add(&vanilla_items::BAMBOO_MOSAIC_STAIRS, BASE * 3 / 2)
        .add_tag(&ItemTag::WOODEN_SLABS, BASE * 3 / 4)
        .add(&vanilla_items::BAMBOO_MOSAIC_SLAB, BASE * 3 / 4)
        .add_tag(&ItemTag::WOODEN_TRAPDOORS, BASE * 3 / 2)
        .add_tag(&ItemTag::WOODEN_PRESSURE_PLATES, BASE * 3 / 2)
        .add_tag(&ItemTag::WOODEN_SHELVES, BASE * 3 / 2)
        .add_tag(&ItemTag::WOODEN_FENCES, BASE * 3 / 2)
        .add_tag(&ItemTag::FENCE_GATES, BASE * 3 / 2)
        .add(&vanilla_items::NOTE_BLOCK, BASE * 3 / 2)
        .add(&vanilla_items::BOOKSHELF, BASE * 3 / 2)
        .add(&vanilla_items::CHISELED_BOOKSHELF, BASE * 3 / 2)
        .add(&vanilla_items::LECTERN, BASE * 3 / 2)
        .add(&vanilla_items::JUKEBOX, BASE * 3 / 2)
        .add(&vanilla_items::CHEST, BASE * 3 / 2)
        .add(&vanilla_items::TRAPPED_CHEST, BASE * 3 / 2)
        .add(&vanilla_items::CRAFTING_TABLE, BASE * 3 / 2)
        .add(&vanilla_items::DAYLIGHT_DETECTOR, BASE * 3 / 2)
        .add_tag(&ItemTag::BANNERS, BASE * 3 / 2)
        .add(&vanilla_items::BOW, BASE * 3 / 2)
        .add(&vanilla_items::FISHING_ROD, BASE * 3 / 2)
        .add(&vanilla_items::LADDER, BASE * 3 / 2)
        .add_tag(&ItemTag::SIGNS, BASE)
        .add_tag(&ItemTag::HANGING_SIGNS, BASE * 4)
        .add(&vanilla_items::WOODEN_SHOVEL, BASE)
        .add(&vanilla_items::WOODEN_SWORD, BASE)
        .add(&vanilla_items::WOODEN_SPEAR, BASE)
        .add(&vanilla_items::WOODEN_HOE, BASE)
        .add(&vanilla_items::WOODEN_AXE, BASE)
        .add(&vanilla_items::WOODEN_PICKAXE, BASE)
        .add_tag(&ItemTag::WOODEN_DOORS, BASE)
        .add_tag(&ItemTag::BOATS, BASE * 6)
        .add_tag(&ItemTag::WOOL, BASE / 2)
        .add_tag(&ItemTag::WOODEN_BUTTONS, BASE / 2)
        .add(&vanilla_items::STICK, BASE / 2)
        .add_tag(&ItemTag::SAPLINGS, BASE / 2)
        .add(&vanilla_items::BOWL, BASE / 2)
        .add_tag(&ItemTag::WOOL_CARPETS, 1 + BASE / 3)
        .add(&vanilla_items::DRIED_KELP_BLOCK, 1 + BASE * 20)
        .add(&vanilla_items::CROSSBOW, BASE * 3 / 2)
        .add(&vanilla_items::BAMBOO, BASE / 4)
        .add(&vanilla_items::DEAD_BUSH, BASE / 2)
        .add(&vanilla_items::SHORT_DRY_GRASS, BASE / 2)
        .add(&vanilla_items::TALL_DRY_GRASS, BASE / 2)
        .add(&vanilla_items::SCAFFOLDING, BASE / 4)
        .add(&vanilla_items::LOOM, BASE * 3 / 2)
        .add(&vanilla_items::BARREL, BASE * 3 / 2)
        .add(&vanilla_items::CARTOGRAPHY_TABLE, BASE * 3 / 2)
        .add(&vanilla_items::FLETCHING_TABLE, BASE * 3 / 2)
        .add(&vanilla_items::SMITHING_TABLE, BASE * 3 / 2)
        .add(&vanilla_items::COMPOSTER, BASE * 3 / 2)
        .add(&vanilla_items::AZALEA, BASE / 2)
        .add(&vanilla_items::FLOWERING_AZALEA, BASE / 2)
        .add(&vanilla_items::MANGROVE_ROOTS, BASE * 3 / 2)
        .add(&vanilla_items::LEAF_LITTER, BASE / 2)
        .remove_tag(&ItemTag::NON_FLAMMABLE_WOOD)
        .build()
});

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_items};

    use super::VANILLA_FUEL_VALUES;

    #[test]
    fn vanilla_fuels_respect_specific_values_and_non_flammable_wood_removal() {
        init_vanilla_registry();

        assert_eq!(
            VANILLA_FUEL_VALUES.burn_duration(&vanilla_items::COAL),
            1600
        );
        assert_eq!(
            VANILLA_FUEL_VALUES.burn_duration(&vanilla_items::STICK),
            100
        );
        assert_eq!(
            VANILLA_FUEL_VALUES.burn_duration(&vanilla_items::CRIMSON_PLANKS),
            0
        );
    }
}
