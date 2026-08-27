//! Natural spawning driven by generated biome spawn tables.
use crate::{
    entity::{ENTITIES, Entity, EntitySpawnReason, Mob, next_entity_id},
    physics::CollisionWorld,
    villager::BIOME_SPAWNS,
    world::{World, level_reader::LevelReader},
    worldgen::natural_spawn::{choose_spawn, group_size},
};
use glam::DVec3;
use std::sync::Arc;
use steel_registry::{
    RegistryExt,
    blocks::block_state_ext::BlockStateExt,
    entity_type::MobCategory,
    fluid::FluidStateExt,
    vanilla_game_rules::{SPAWN_MOBS, SPAWN_MONSTERS},
};
use steel_utils::{
    BlockPos, WorldAabb,
    random::{Random as _, legacy_random::LegacyRandom},
};

const INTERVAL: u64 = 20;
const RADIUS: i32 = 128;
const MIN_DISTANCE_SQ: f64 = 24.0 * 24.0;

fn category_name(category: MobCategory) -> &'static str {
    match category {
        MobCategory::Monster => "monster",
        MobCategory::Creature => "creature",
        MobCategory::Ambient => "ambient",
        MobCategory::Axolotls => "axolotls",
        MobCategory::UndergroundWaterCreature => "underground_water_creature",
        MobCategory::WaterCreature => "water_creature",
        MobCategory::WaterAmbient => "water_ambient",
        MobCategory::Misc => "misc",
    }
}

impl World {
    pub(super) fn tick_natural_spawning(self: &Arc<Self>, tick: u64) {
        if tick % INTERVAL != 0 || !self.get_game_rule(&SPAWN_MOBS) {
            return;
        }
        let mut players = Vec::new();
        self.players.iter_players(|_, player| {
            if !player.is_spectator() {
                players.push(player.clone());
            }
            true
        });
        for player in players {
            self.spawn_for_player(&player);
        }
    }

    fn spawn_for_player(self: &Arc<Self>, player: &crate::player::Player) {
        let center = player.block_position();
        let mut random = LegacyRandom::from_seed(rand::random());
        for category in MobCategory::SPAWNING_CATEGORIES {
            if category == MobCategory::Monster && !self.get_game_rule(&SPAWN_MONSTERS) {
                continue;
            }
            let cap = category.max_instances_per_chunk().max(0) as usize;
            let area = WorldAabb::new(
                f64::from(center.x() - RADIUS),
                f64::from(self.get_min_y()),
                f64::from(center.z() - RADIUS),
                f64::from(center.x() + RADIUS),
                f64::from(self.get_max_y()),
                f64::from(center.z() + RADIUS),
            );
            let count = self
                .get_entities_in_aabb_matching(&area, |e| {
                    e.entity_type().mob_category == category && !e.is_removed()
                })
                .len();
            if count >= cap {
                continue;
            }
            let aquatic = matches!(
                category,
                MobCategory::Axolotls
                    | MobCategory::UndergroundWaterCreature
                    | MobCategory::WaterCreature
                    | MobCategory::WaterAmbient
            );
            let attempts = if aquatic { 24 } else { 3 };
            let mut groups_spawned = 0;
            let mut spawned = 0usize;
            for _ in 0..attempts {
                let x = center.x() + random.next_i32_between(-RADIUS, RADIUS);
                let z = center.z() + random.next_i32_between(-RADIUS, RADIUS);
                let Some(y) = self.height_at(
                    crate::chunk::heightmap::HeightmapType::MotionBlockingNoLeaves,
                    x,
                    z,
                ) else {
                    continue;
                };
                let y = if matches!(
                    category,
                    MobCategory::Axolotls
                        | MobCategory::UndergroundWaterCreature
                        | MobCategory::WaterCreature
                        | MobCategory::WaterAmbient
                ) {
                    y - 1 - random.next_i32_bounded(16)
                } else {
                    y
                };
                if y < self.get_min_y() {
                    continue;
                }
                let pos = BlockPos::new(x, y, z);
                let Some(distance) = self.nearest_player_distance_sqr(DVec3::new(
                    f64::from(x) + 0.5,
                    f64::from(y),
                    f64::from(z) + 0.5,
                )) else {
                    continue;
                };
                if distance <= MIN_DISTANCE_SQ {
                    continue;
                }
                let Some(biome) = self.biome_at(pos) else {
                    continue;
                };
                let Some(data) = BIOME_SPAWNS
                    .iter()
                    .find(|d| d.biome == biome.key.path.as_ref())
                else {
                    continue;
                };
                let Some(spawn) = choose_spawn(data, category_name(category), &mut random) else {
                    continue;
                };
                let Ok(key) = spawn.entity_type.parse() else {
                    continue;
                };
                let Some(entity_type) = steel_registry::REGISTRY.entity_types.by_key(&key) else {
                    continue;
                };
                if spawn.entity_type == "minecraft:slime" {
                    let cx = x.div_euclid(16) as i64;
                    let cz = z.div_euclid(16) as i64;
                    let seed = self.seed();
                    let mixed = cx.wrapping_mul(cx).wrapping_mul(4_987_142)
                        .wrapping_add(cx.wrapping_mul(5_947_611))
                        .wrapping_add(cz.wrapping_mul(cz).wrapping_mul(4_392_871))
                        .wrapping_add(cz.wrapping_mul(3_897_111))
                        ^ seed;
                    let mut slime_rng = LegacyRandom::from_seed(mixed as u64);
                    if slime_rng.next_i32_bounded(10) != 0 {
                        continue;
                    }
                }
                if entity_type.mob_category != category
                    || !self.valid_spawn(pos, category, spawn.entity_type, entity_type.dimensions)
                {
                    continue;
                }
                let amount = (group_size(spawn, &mut random).max(1) as usize)
                    .min(cap.saturating_sub(count + spawned));
                spawned += self.spawn_group(x, y, z, entity_type, amount, category, &mut random);
                groups_spawned += 1;
                if spawned >= cap.saturating_sub(count) {
                    break;
                }
                if !aquatic || groups_spawned >= 3 {
                    break;
                }
            }
        }
    }

    fn valid_spawn(
        self: &Arc<Self>,
        pos: BlockPos,
        category: MobCategory,
        entity_type: &str,
        dimensions: steel_registry::entity_type::EntityDimensions,
    ) -> bool {
        let state = self.get_block_state(pos);
        let below = self.get_block_state(pos.below());
        let water_category =
            matches!(
                category,
                MobCategory::Axolotls
                    | MobCategory::UndergroundWaterCreature
                    | MobCategory::WaterCreature
                    | MobCategory::WaterAmbient
            ) || crate::villager::requires_water(category_name(category), entity_type);
        if water_category {
            if !state.get_fluid_state().is_water() || !below.get_fluid_state().is_water() {
                return false;
            }
        } else if !state.is_air() || !below.is_solid() || below.get_fluid_state().is_water() {
            return false;
        }
        let block_light = self.light_value_at(crate::chunk::light::LightLayer::Block, pos);
        let light = self.raw_brightness(pos, 0);
        let half = f64::from(dimensions.width) / 2.0;
        let aabb = WorldAabb::new(
            f64::from(pos.x()) + 0.5 - half,
            f64::from(pos.y()),
            f64::from(pos.z()) + 0.5 - half,
            f64::from(pos.x()) + 0.5 + half,
            f64::from(pos.y()) + f64::from(dimensions.height),
            f64::from(pos.z()) + 0.5 + half,
        );
        if entity_type == "minecraft:bat" {
            // Bats use a dedicated underground rule, rather than the daytime
            // brightness rule used by ordinary ambient creatures.
            return !self.can_see_sky(pos)
                && light <= 3
                && !crate::physics::WorldCollisionProvider::new(self).has_block_collision(&aabb)
                && !self.has_entity_in_aabb_matching(&aabb, Entity::blocks_building);
        }
        if entity_type == "minecraft:glow_squid"
            && (self.can_see_sky(pos) || light > 7 || pos.y() >= self.sea_level - 20)
        {
            return false;
        }
        if category == MobCategory::Monster {
            if block_light != 0 {
                return false;
            }
        } else if category.is_friendly()
            && !water_category
            && light <= 8
        {
            return false;
        }
        !crate::physics::WorldCollisionProvider::new(self).has_block_collision(&aabb)
            && !self.has_entity_in_aabb_matching(&aabb, Entity::blocks_building)
    }

    fn spawn_group(
        self: &Arc<Self>,
        x: i32,
        y: i32,
        z: i32,
        entity_type: &'static steel_registry::entity_type::EntityType,
        amount: usize,
        category: MobCategory,
        random: &mut LegacyRandom,
    ) -> usize {
        let mut spawned = 0;
        for _ in 0..amount {
            let pos = BlockPos::new(
                x + random.next_i32_between(-5, 5),
                y,
                z + random.next_i32_between(-5, 5),
            );
            if !self.valid_spawn(
                pos,
                category,
                entity_type.key.path.as_ref(),
                entity_type.dimensions,
            ) {
                continue;
            }
            let Some(entity) = ENTITIES.create(
                entity_type,
                next_entity_id(),
                DVec3::new(
                    f64::from(pos.x()) + 0.5,
                    f64::from(pos.y()),
                    f64::from(pos.z()) + 0.5,
                ),
                Arc::downgrade(self),
            ) else {
                continue;
            };
            if let Some(mob) = entity.as_mob() {
                let _ = Mob::finalize_spawn(mob, self, EntitySpawnReason::Natural, None);
            }
            if self.try_add_entity(entity).is_ok() {
                spawned += 1;
            }
        }
        spawned
    }
}
