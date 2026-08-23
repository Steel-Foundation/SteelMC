//! Runtime natural spawning for water mobs.

use std::sync::Arc;

use glam::DVec3;
use steel_registry::{RegistryExt, blocks::block_state_ext::BlockStateExt, fluid::FluidStateExt};
use steel_utils::{
    BlockPos, WorldAabb,
    random::{Random as _, legacy_random::LegacyRandom},
};

use crate::{
    entity::{ENTITIES, Entity, next_entity_id},
    villager::{BIOME_SPAWNS, requires_water},
    world::World,
    worldgen::natural_spawn::group_size,
};

const FISH_CATEGORY_CAP: usize = 16;
const SPAWN_INTERVAL: u64 = 20;
const SEARCH_RADIUS: i32 = 32;
const SURFACE_WATER_DEPTH: i32 = 13;

impl World {
    pub(super) fn tick_natural_spawning(self: &Arc<Self>, tick: u64) {
        if tick % SPAWN_INTERVAL != 0 {
            return;
        }
        let Some(player) = self.nearest_player(DVec3::ZERO, -1.0, |player| !player.is_spectator())
        else {
            return;
        };
        let center = player.block_position();
        let area = WorldAabb::new(
            f64::from(center.x() - SEARCH_RADIUS),
            f64::from(self.get_min_y()),
            f64::from(center.z() - SEARCH_RADIUS),
            f64::from(center.x() + SEARCH_RADIUS),
            f64::from(self.get_max_y()),
            f64::from(center.z() + SEARCH_RADIUS),
        );
        let fish_count = self
            .get_entities_in_aabb_matching(&area, |entity| {
                matches!(
                    entity.entity_type().key.path.as_ref(),
                    "cod" | "salmon" | "pufferfish" | "tropical_fish"
                )
            })
            .len();
        if fish_count >= FISH_CATEGORY_CAP {
            return;
        }

        let mut random = LegacyRandom::from_seed(rand::random());
        let x = center.x() + random.next_i32_between(-SEARCH_RADIUS, SEARCH_RADIUS);
        let z = center.z() + random.next_i32_between(-SEARCH_RADIUS, SEARCH_RADIUS);
        let y = self.sea_level - random.next_i32_bounded(SURFACE_WATER_DEPTH + 1);
        let pos = BlockPos::new(x, y, z);
        let Some(biome) = self.biome_at(pos) else {
            return;
        };
        let Some(data) = BIOME_SPAWNS
            .iter()
            .find(|data| data.biome == biome.key.path.as_ref())
        else {
            return;
        };
        let Some(spawn) =
            crate::worldgen::natural_spawn::choose_spawn(data, "water_ambient", &mut random)
        else {
            return;
        };
        if !requires_water(spawn.category, spawn.entity_type) {
            return;
        }
        let count = usize::try_from(group_size(spawn, &mut random)).unwrap_or_default();
        let count = count.min(FISH_CATEGORY_CAP - fish_count);
        if count == 0 {
            return;
        }
        let key = spawn.entity_type.parse().ok();
        let Some(key) = key else {
            return;
        };
        let Some(entity_type) = steel_registry::REGISTRY.entity_types.by_key(&key) else {
            return;
        };
        for _ in 0..count {
            let candidate = BlockPos::new(
                x + random.next_i32_bounded(6) - random.next_i32_bounded(6),
                y,
                z + random.next_i32_bounded(6) - random.next_i32_bounded(6),
            );
            let state = self.get_block_state(candidate);
            let above = self.get_block_state(candidate.above());
            let below = self.get_block_state(candidate.below());
            if !state.get_fluid_state().is_water()
                || !above.get_fluid_state().is_water()
                || !below.get_fluid_state().is_water()
            {
                continue;
            }
            let Some(entity) = ENTITIES.create(
                entity_type,
                next_entity_id(),
                DVec3::new(
                    f64::from(candidate.x()) + 0.5,
                    f64::from(candidate.y()),
                    f64::from(candidate.z()) + 0.5,
                ),
                Arc::downgrade(self),
            ) else {
                continue;
            };
            let _ = self.try_add_entity(entity);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fish_spawn_interval_is_twenty_ticks() {
        assert_eq!(super::SPAWN_INTERVAL, 20);
    }
}
