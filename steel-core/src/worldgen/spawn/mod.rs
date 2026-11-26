//! original mob spawning during the chunk Spawn stage.

pub(crate) mod finalize;
#[cfg(feature = "benchmark-support")]
pub mod fixture;
mod placement;
pub(crate) mod rules;
#[cfg(test)]
mod tests;

use glam::{DVec3, IVec3};
use steel_math::DEGREE_360;
use steel_registry::biome::{BiomeRef, WorldgenCreatureSpawn};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::{REGISTRY, RegistryExt};
use steel_utils::random::{Random, RandomSource, legacy_random::LegacyRandom};
use steel_utils::{BlockPos, SectionPos};

use crate::entity::{ENTITIES, Entity, EntityBase, SharedEntity};
use crate::world::World;
use crate::worldgen::generator::vanilla::fuzzed_biome_at_block;
use crate::worldgen::region::WorldGenRegion;

use self::finalize::{SpawnContext, SpawnData, SpawnState};
use self::placement::{Attempt, Placement};

const DEFAULT_CREATURE_WORLD_GEN_SPAWN_PROBABILITY: f32 = 0.1;
const CREATURE_WORLD_GEN_PROBABILITY_MAX: f32 = 0.999_999_9;
const ATTEMPTS_PER_MOB: usize = 4;
const RANDOM_WALK_BOUND: i32 = 5;

#[derive(Clone, Copy)]
pub(crate) struct SpawnSite {
    pub entity_type: EntityTypeRef,
    pub pos: BlockPos,
}

pub(crate) struct SpawnSiteContext<'a, 'region> {
    region: &'a mut WorldGenRegion<'region>,
    biome_zoom_seed: i64,
    pub site: SpawnSite,
}

impl<'region> SpawnSiteContext<'_, 'region> {
    pub(crate) const fn region(&self) -> &WorldGenRegion<'region> {
        self.region
    }

    pub(crate) fn biome(&self) -> BiomeRef {
        biome_at(self.region(), self.biome_zoom_seed, self.site.pos)
    }

    /// Draws from the region stream shared with spawn finalization
    pub(crate) const fn random_mut(&mut self) -> &mut RandomSource {
        self.region.random_mut()
    }
}

#[derive(Clone, Copy)]
enum SpawnMode {
    Materialize,
    #[cfg(test)]
    Trace {
        spawn_mobs: bool,
    },
}

pub(super) fn spawn_original_mobs(
    region: &mut WorldGenRegion<'_>,
    world: &World,
    world_seed: i64,
    biome_zoom_seed: i64,
) {
    let _ = generate(
        region,
        world,
        world_seed,
        biome_zoom_seed,
        SpawnMode::Materialize,
        |_, _, _| {},
    );
}

fn generate(
    region: &mut WorldGenRegion<'_>,
    world: &World,
    world_seed: i64,
    biome_zoom_seed: i64,
    mode: SpawnMode,
    mut on_spawn: impl FnMut(DVec3, f32, &SpawnState<'_>),
) -> i64 {
    let center = region.center();
    let size = SectionPos::section_to_block_coord(1, 0);
    let origin_x = center.0.x * size;
    let origin_z = center.0.y * size;
    let mut random = LegacyRandom::for_chunk_generation(world_seed, origin_x, origin_z);

    #[cfg(test)]
    if matches!(mode, SpawnMode::Trace { spawn_mobs: false }) {
        return random.get_seed();
    }

    let source = BlockPos::new(origin_x, region.max_y_exclusive() - 1, origin_z);
    let list = biome_at(region, biome_zoom_seed, source).worldgen_creature_spawns;
    if list.last().is_none_or(|entry| entry.cumulative_weight == 0) {
        return random.get_seed();
    }

    let probability = creature_spawn_probability(noise_biome_at(region, source));
    while random.next_f32() < probability {
        let Some(entry) = WorldgenCreatureSpawn::select(list, &mut random) else {
            continue;
        };

        generate_group(
            region,
            world,
            biome_zoom_seed,
            mode,
            entry,
            &mut random,
            &mut on_spawn,
        );
    }

    random.get_seed()
}

// Keep group setup off chunks that never select one.
#[inline(never)]
fn generate_group(
    region: &mut WorldGenRegion<'_>,
    world: &World,
    biome_zoom_seed: i64,
    mode: SpawnMode,
    entry: &WorldgenCreatureSpawn,
    random: &mut LegacyRandom,
    on_spawn: &mut impl FnMut(DVec3, f32, &SpawnState<'_>),
) {
    let center = region.center();
    let chunk_width = SectionPos::section_to_block_coord(1, 0);
    let origin_x = center.0.x * chunk_width;
    let origin_z = center.0.y * chunk_width;
    let count = entry.count.sample(random);
    let mut x = origin_x + random.next_i32_bounded(chunk_width);
    let mut z = origin_z + random.next_i32_bounded(chunk_width);
    let (start_x, start_z) = (x, z);
    let entity_type = entry.entity_type;
    let Some(placement) = Placement::for_entity(entity_type) else {
        panic!("Missing worldgen spawn placement for {}", entity_type.key);
    };

    let Some(with_group) = ENTITIES.worldgen_spawn_group(entity_type) else {
        panic!(
            "Missing worldgen spawn finalization for {}",
            entity_type.key
        );
    };

    let factory = match mode {
        SpawnMode::Materialize => ENTITIES.factory_for(entity_type),
        #[cfg(test)]
        SpawnMode::Trace { .. } => None,
    };

    with_group(&mut |group| {
        for _ in 0..count {
            for _ in 0..ATTEMPTS_PER_MOB {
                let success = match placement.try_position(region, world, biome_zoom_seed, x, z) {
                    Attempt::WalkWithoutCandidate => false,
                    Attempt::RetryAtSamePosition => continue,
                    Attempt::Candidate {
                        position,
                        can_finalize,
                    } => {
                        let yaw = spawn_yaw(random);
                        if can_finalize {
                            let site = SpawnSite {
                                entity_type,
                                pos: BlockPos::from(position),
                            };

                            let mut context = SpawnSiteContext {
                                region,
                                biome_zoom_seed,
                                site,
                            };

                            let state = group.finalize(SpawnContext {
                                site,
                                biome: context.biome(),
                                world,
                                random: context.random_mut(),
                            });

                            on_spawn(position, yaw, &state);

                            if let Some(factory) = &factory {
                                let entity = factory(position, region.weak_world());
                                add_spawn(region, &state, entity, position, yaw);
                            }
                        }

                        can_finalize
                    }
                };

                x += random_walk(random);
                z += random_walk(random);

                while x - origin_x < 0
                    || x - origin_x >= chunk_width
                    || z - origin_z < 0
                    || z - origin_z >= chunk_width
                {
                    x = start_x + random_walk(random);
                    z = start_z + random_walk(random);
                }

                if success {
                    break;
                }
            }
        }
    });
}

fn add_spawn(
    region: &WorldGenRegion<'_>,
    state: &SpawnState<'_>,
    entity: SharedEntity,
    position: DVec3,
    yaw: f32,
) {
    if !initialize_spawn(entity.as_ref(), &state.data, position, yaw) {
        return;
    }

    let passenger = state.passenger.as_ref().and_then(|data| {
        let rider = ENTITIES.create(data.entity_type, position, region.weak_world())?;
        if !initialize_spawn(rider.as_ref(), data, position, yaw) {
            return None;
        }

        EntityBase::restore_passenger_relationship(&entity, &rider);

        Some(rider)
    });

    let _ = region.add_fresh_entity(entity);
    if let Some(passenger) = passenger {
        let _ = region.add_fresh_entity(passenger);
    }
}

fn initialize_spawn(entity: &dyn Entity, data: &SpawnData<'_>, position: DVec3, yaw: f32) -> bool {
    entity.snap_to(position, yaw, 0.0);
    entity.base().set_old_rotation_to_current();
    if let Err(error) = data.apply(entity) {
        log::warn!(
            "Skipping worldgen spawn of {} at {position:?}: {error}",
            data.entity_type.key
        );
        return false;
    }

    true
}

fn random_walk(random: &mut LegacyRandom) -> i32 {
    random.next_i32_bounded(RANDOM_WALK_BOUND) - random.next_i32_bounded(RANDOM_WALK_BOUND)
}

fn spawn_yaw(random: &mut LegacyRandom) -> f32 {
    random.next_f32() * DEGREE_360
}

fn creature_spawn_probability(biome: BiomeRef) -> f32 {
    biome
        .creature_world_gen_spawn_probability
        .unwrap_or(DEFAULT_CREATURE_WORLD_GEN_SPAWN_PROBABILITY)
        .clamp(0.0, CREATURE_WORLD_GEN_PROBABILITY_MAX)
}

fn biome_at(region: &WorldGenRegion<'_>, biome_zoom_seed: i64, pos: BlockPos) -> BiomeRef {
    let id = fuzzed_biome_at_block(biome_zoom_seed, pos, |quart| {
        region.noise_biome_id(quart.x, quart.y, quart.z)
    });

    registered_biome(id)
}

fn noise_biome_at(region: &WorldGenRegion<'_>, pos: BlockPos) -> BiomeRef {
    let quart: IVec3 = pos.0 >> 2;
    registered_biome(region.noise_biome_id(quart.x, quart.y, quart.z))
}

fn registered_biome(id: u16) -> BiomeRef {
    let Some(biome) = REGISTRY.biomes.by_id(usize::from(id)) else {
        panic!("Biome {id} missing from the registry during chunk generation");
    };

    biome
}
