//! Deterministic natural-spawn selection built from extracted biome data.

use steel_utils::random::Random;

use crate::villager::{BiomeSpawnData, SpawnData};

/// Selects one weighted spawn entry from a biome/category.
#[must_use]
pub fn choose_spawn<'a>(
    biome: &'a BiomeSpawnData,
    category: &str,
    random: &mut impl Random,
) -> Option<&'a SpawnData> {
    let candidates = biome
        .spawns
        .iter()
        .filter(|spawn| spawn.category == category)
        .collect::<Vec<_>>();
    let total_weight = candidates.iter().map(|spawn| spawn.weight).sum::<i32>();
    if total_weight <= 0 {
        return None;
    }
    let mut choice = random.next_i32_bounded(total_weight);
    for spawn in candidates {
        if choice < spawn.weight {
            return Some(spawn);
        }
        choice -= spawn.weight;
    }
    None
}

/// Chooses the vanilla inclusive group size for a spawn entry.
#[must_use]
pub fn group_size(spawn: &SpawnData, random: &mut impl Random) -> i32 {
    random.next_i32_between(spawn.min_count, spawn.max_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::villager::BIOME_SPAWNS;
    use steel_utils::random::xoroshiro::Xoroshiro;

    #[test]
    fn ocean_contains_weighted_cod_spawn() {
        let ocean = BIOME_SPAWNS
            .iter()
            .find(|biome| biome.biome == "ocean")
            .expect("ocean extracted");
        let mut random = Xoroshiro::from_seed(7);
        let spawn = choose_spawn(ocean, "water_ambient", &mut random).expect("ocean water spawn");
        assert_eq!(spawn.entity_type, "minecraft:cod");
        assert!((3..=6).contains(&group_size(spawn, &mut random)));
    }
}
