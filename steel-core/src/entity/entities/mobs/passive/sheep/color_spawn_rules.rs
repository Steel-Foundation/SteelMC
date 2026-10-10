use steel_registry::biome::BiomeRef;
use steel_registry::{DyeColor, vanilla_biome_tags::BiomeTag};
use steel_utils::random::Random;

use super::SheepEntity;
use SheepColorProvider::{Single, Weighted};

pub(super) const TEMPERATE_SPAWN_CONFIGURATION: SheepColorProvider = Weighted(&[
    (Single(DyeColor::Black), 5),
    (Single(DyeColor::Gray), 5),
    (Single(DyeColor::LightGray), 5),
    (Single(DyeColor::Brown), 3),
    (
        Weighted(&SheepColorProvider::common_colors(DyeColor::White)),
        82,
    ),
]);

const WARM_SPAWN_CONFIGURATION: SheepColorProvider = Weighted(&[
    (Single(DyeColor::Gray), 5),
    (Single(DyeColor::LightGray), 5),
    (Single(DyeColor::White), 5),
    (Single(DyeColor::Black), 3),
    (
        Weighted(&SheepColorProvider::common_colors(DyeColor::Brown)),
        82,
    ),
]);

const COLD_SPAWN_CONFIGURATION: SheepColorProvider = Weighted(&[
    (Single(DyeColor::LightGray), 5),
    (Single(DyeColor::Gray), 5),
    (Single(DyeColor::White), 5),
    (Single(DyeColor::Brown), 3),
    (
        Weighted(&SheepColorProvider::common_colors(DyeColor::Black)),
        82,
    ),
]);

/// minecraft nested providers consume another roll only for the common color group
pub(super) enum SheepColorProvider {
    Single(DyeColor),
    Weighted(&'static [(Self, i32)]),
}

impl SheepColorProvider {
    const fn common_colors(default_color: DyeColor) -> [(Self, i32); 2] {
        [(Single(default_color), 499), (Single(DyeColor::Pink), 1)]
    }

    pub(super) fn get(&self, random: &mut impl Random) -> DyeColor {
        match self {
            Self::Single(color) => *color,
            Self::Weighted(entries) => {
                let total_weight = entries.iter().map(|(_, weight)| weight).sum();
                let mut selection = random.next_i32_bounded(total_weight);

                for (provider, weight) in *entries {
                    selection -= weight;
                    if selection < 0 {
                        return provider.get(random);
                    }
                }

                unreachable!("selection exceeded total sheep color weight");
            }
        }
    }
}

impl SheepEntity {
    /// picks a spawn color using the biome nested weighted configuration
    #[must_use]
    pub fn random_sheep_color(biome: BiomeRef, random: &mut impl Random) -> DyeColor {
        let configuration = if biome.has_tag(&BiomeTag::SPAWNS_WARM_VARIANT_FARM_ANIMALS) {
            &WARM_SPAWN_CONFIGURATION
        } else if biome.has_tag(&BiomeTag::SPAWNS_COLD_VARIANT_FARM_ANIMALS) {
            &COLD_SPAWN_CONFIGURATION
        } else {
            &TEMPERATE_SPAWN_CONFIGURATION
        };

        configuration.get(random)
    }
}
