//! Vanilla `SheepColorSpawnRules`: the wool color a sheep spawns with, by biome.

use steel_registry::biome::BiomeRef;
use steel_registry::{DyeColor, vanilla_biome_tags::BiomeTag};
use steel_utils::random::Random;

use SheepColorProvider::{Single, Weighted};

const TEMPERATE_SPAWN_CONFIGURATION: SheepColorProvider = Weighted(&[
    (Single(DyeColor::Black), 5),
    (Single(DyeColor::Gray), 5),
    (Single(DyeColor::LightGray), 5),
    (Single(DyeColor::Brown), 3),
    (Weighted(&common_colors(DyeColor::White)), 82),
]);

const WARM_SPAWN_CONFIGURATION: SheepColorProvider = Weighted(&[
    (Single(DyeColor::Gray), 5),
    (Single(DyeColor::LightGray), 5),
    (Single(DyeColor::White), 5),
    (Single(DyeColor::Black), 3),
    (Weighted(&common_colors(DyeColor::Brown)), 82),
]);

const COLD_SPAWN_CONFIGURATION: SheepColorProvider = Weighted(&[
    (Single(DyeColor::LightGray), 5),
    (Single(DyeColor::Gray), 5),
    (Single(DyeColor::White), 5),
    (Single(DyeColor::Brown), 3),
    (Weighted(&common_colors(DyeColor::Black)), 82),
]);

/// `commonColors`: the biome's default color, rarely pink.
const fn common_colors(default_color: DyeColor) -> [(SheepColorProvider, i32); 2] {
    [(Single(default_color), 499), (Single(DyeColor::Pink), 1)]
}

/// A color, or a weighted choice of nested providers (each nesting rolls again).
enum SheepColorProvider {
    Single(DyeColor),
    Weighted(&'static [(Self, i32)]),
}

impl SheepColorProvider {
    fn get(&self, random: &mut impl Random) -> DyeColor {
        match self {
            Single(color) => *color,
            Weighted(entries) => {
                let total_weight = entries.iter().map(|(_, weight)| weight).sum();
                let mut selection = random.next_i32_bounded(total_weight);
                for (provider, weight) in *entries {
                    selection -= weight;
                    if selection < 0 {
                        return provider.get(random);
                    }
                }
                unreachable!("selection is below the positive total weight");
            }
        }
    }
}

/// `SheepColorSpawnRules.getSheepColor`.
pub(super) fn sheep_color(biome: Option<BiomeRef>, random: &mut impl Random) -> DyeColor {
    let configuration = match biome {
        Some(biome) if biome.has_tag(&BiomeTag::SPAWNS_WARM_VARIANT_FARM_ANIMALS) => {
            &WARM_SPAWN_CONFIGURATION
        }
        Some(biome) if biome.has_tag(&BiomeTag::SPAWNS_COLD_VARIANT_FARM_ANIMALS) => {
            &COLD_SPAWN_CONFIGURATION
        }
        _ => &TEMPERATE_SPAWN_CONFIGURATION,
    };
    configuration.get(random)
}
