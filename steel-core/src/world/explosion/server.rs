use std::{
    sync::{Arc, LazyLock},
    vec::IntoIter,
};

use glam::DVec3;
#[cfg(test)]
use rayon::prelude::*;
use rustc_hash::FxHashMap;
#[cfg(test)]
use rustc_hash::{FxBuildHasher, FxHashSet};
use smallvec::SmallVec;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_type_tags::EntityTypeTag;
use steel_registry::vanilla_game_rules::MOB_GRIEFING;
use steel_registry::{
    REGISTRY, TaggedRegistryExt as _, vanilla_attributes, vanilla_damage_types, vanilla_entities,
    vanilla_game_events,
};
use steel_utils::random::Random;
use steel_utils::types::{GameType, UpdateFlags};
use steel_utils::{BlockPos, BlockStateId, PackedBlockPos, WorldAabb};

use crate::behavior::blocks::{FireBlock, PowderSnowBlock};
use crate::behavior::{BLOCK_BEHAVIORS, BlockCollisionContext};
use crate::chunk::gameplay_chunk_lookup_cache::LocalFullChunkHolderCache;
use crate::entity::damage::DamageSource;
use crate::entity::entities::{ItemEntity, PrimedTntEntity};
use crate::entity::{Entity, SharedEntity};
use crate::world::game_event::GameEventContext;
use crate::world::raycast::ExplosionExposureRaycast;
use crate::world::{BlockRegionBounds, BlockRegionRead, World};

use super::{
    BlockInteraction, Explosion, ExplosionBlockReader, ExplosionDamageCalculator,
    ImmutableExplosionBlockCalculator, SelectedDamageCalculator,
};

const RAY_GRID_SIZE: i32 = 16;
const RAY_COUNT: usize = 16 * 16 * 16 - 14 * 14 * 14;
const RAY_STEP: f64 = 0.3_f32 as f64;
const RAY_POWER_DECAY: f32 = 0.225_000_01;
const MIN_DAMAGE_RADIUS: f32 = 1.0e-5;
const NORMALIZE_EPSILON: f64 = 1.0e-5_f32 as f64;
const MAX_DROPS_PER_COMBINED_STACK: i32 = 16;
const BLOCK_CACHE_BITS: u32 = 9;
const BLOCK_CACHE_SIZE: usize = 1 << BLOCK_CACHE_BITS;
const BLOCK_CACHE_MASK: usize = BLOCK_CACHE_SIZE - 1;
const LONG_HASH_PHI: u64 = 0x9e37_79b9_7f4a_7c15;
const JAVA_HASH_MAP_TREEIFY_THRESHOLD: usize = 8;
const JAVA_HASH_MAP_MIN_TREEIFY_CAPACITY: usize = 64;

#[derive(Clone, Copy)]
struct ExplosionRay {
    step: DVec3,
    initial_power: f32,
}

#[derive(Clone, Copy)]
struct ExplosionBlockCacheEntry {
    tag: i64,
    state: BlockStateId,
    resistance: Option<f32>,
    occupied: bool,
    affected: bool,
}

impl ExplosionBlockCacheEntry {
    const EMPTY: Self = Self {
        tag: 0,
        state: BlockStateId(0),
        resistance: None,
        occupied: false,
        affected: false,
    };
}

struct ExplosionBlockCache {
    entries: [ExplosionBlockCacheEntry; BLOCK_CACHE_SIZE],
}

#[derive(Clone, Copy)]
struct ImmutableRayCachePolicy {
    resistance: bool,
    always_allows_block_explosion: bool,
}

impl Default for ExplosionBlockCache {
    fn default() -> Self {
        Self {
            entries: [ExplosionBlockCacheEntry::EMPTY; BLOCK_CACHE_SIZE],
        }
    }
}

struct RegionExplosionBlockReader<'reader, 'world> {
    region: &'reader BlockRegionRead<'world>,
}

impl<'reader, 'world> RegionExplosionBlockReader<'reader, 'world> {
    const fn new(region: &'reader BlockRegionRead<'world>) -> Self {
        Self { region }
    }
}

impl ExplosionBlockReader for RegionExplosionBlockReader<'_, '_> {
    #[inline]
    fn block_state(&self, pos: BlockPos) -> Option<BlockStateId> {
        self.region.get_block_state(pos)
    }
}

static RAY_STEPS: LazyLock<[DVec3; RAY_COUNT]> = LazyLock::new(|| {
    let mut steps = [DVec3::ZERO; RAY_COUNT];
    let mut index = 0;
    for xx in 0..RAY_GRID_SIZE {
        for yy in 0..RAY_GRID_SIZE {
            for zz in 0..RAY_GRID_SIZE {
                if is_boundary_ray(xx, yy, zz) {
                    steps[index] = ray_direction(xx, yy, zz) * RAY_STEP;
                    index += 1;
                }
            }
        }
    }
    debug_assert_eq!(index, RAY_COUNT);
    steps
});

#[derive(Clone, Copy)]
struct ExplosionRayContext {
    center: DVec3,
    bounds: ExplosionWorldBounds,
}

#[derive(Clone, Copy)]
struct ExplosionWorldBounds {
    min_y: i32,
    max_y: i32,
}

impl ExplosionWorldBounds {
    const fn from_world(world: &World) -> Self {
        Self {
            min_y: world.get_min_y(),
            max_y: world.get_max_y(),
        }
    }

    const fn contains(self, pos: BlockPos) -> bool {
        pos.y() >= self.min_y && pos.y() <= self.max_y && World::is_in_world_bounds_horizontal(pos)
    }
}

pub(super) struct ServerExplosion<'a> {
    world: &'a Arc<World>,
    fire: bool,
    block_interaction: BlockInteraction,
    center: DVec3,
    source: Option<&'a dyn Entity>,
    indirect_source: Option<SharedEntity>,
    radius: f32,
    damage_source: DamageSource,
    damage_calculator: SelectedDamageCalculator<'a>,
    immutable_block_calculator: Option<&'a dyn ImmutableExplosionBlockCalculator>,
    pub(super) hit_players: FxHashMap<i32, DVec3>,
}

impl<'a> ServerExplosion<'a> {
    #[expect(
        clippy::too_many_arguments,
        reason = "mirrors the Vanilla ServerExplosion construction boundary"
    )]
    pub(super) fn new(
        world: &'a Arc<World>,
        source: Option<&'a dyn Entity>,
        damage_source: Option<DamageSource>,
        damage_calculator: Option<&'a dyn ExplosionDamageCalculator>,
        immutable_block_calculator: Option<&'a dyn ImmutableExplosionBlockCalculator>,
        center: DVec3,
        radius: f32,
        fire: bool,
        block_interaction: BlockInteraction,
    ) -> Self {
        let indirect_source = source
            .filter(|source| source.as_living_entity().is_none())
            .and_then(Entity::explosion_indirect_source);
        let indirect_source_entity = source
            .filter(|source| source.as_living_entity().is_some())
            .or(indirect_source.as_deref());
        let damage_source = damage_source
            .unwrap_or_else(|| default_explosion_damage_source(source, indirect_source_entity));
        let damage_calculator = match damage_calculator {
            Some(calculator) => SelectedDamageCalculator::Custom(calculator),
            None => source.map_or(
                SelectedDamageCalculator::Default,
                SelectedDamageCalculator::Entity,
            ),
        };
        Self {
            world,
            fire,
            block_interaction,
            center,
            source,
            indirect_source,
            radius,
            damage_source,
            damage_calculator,
            immutable_block_calculator,
            hit_players: FxHashMap::default(),
        }
    }

    pub(super) fn explode(&mut self) -> usize {
        self.world.game_event_at(
            &vanilla_game_events::EXPLODE,
            self.center,
            &GameEventContext::new(self.source, None),
        );
        let mut affected = self.calculate_exploded_positions_from_level_random();
        self.hurt_entities();
        if self.interacts_with_blocks() {
            self.interact_with_blocks(&mut affected);
        }
        if self.fire {
            self.create_fire(&affected);
        }
        affected.len()
    }

    fn calculate_exploded_positions_from_level_random(&self) -> Vec<BlockPos> {
        let Some(calculator) = self.immutable_calculator_for_rays() else {
            return self.calculate_exploded_positions_sequential(|| {
                self.world.with_random(Random::next_f32)
            });
        };

        let powers = self
            .world
            .with_random(|random| self.draw_immutable_ray_powers(|| random.next_f32()));
        self.calculate_immutable_ray_powers(&powers, calculator)
    }

    #[cfg(test)]
    fn calculate_exploded_positions(&self, mut next_float: impl FnMut() -> f32) -> Vec<BlockPos> {
        let Some(calculator) = self.immutable_calculator_for_rays() else {
            return self.calculate_exploded_positions_sequential(next_float);
        };

        let powers = self.draw_immutable_ray_powers(&mut next_float);
        self.calculate_immutable_ray_powers(&powers, calculator)
    }

    fn immutable_calculator_for_rays(&self) -> Option<&dyn ImmutableExplosionBlockCalculator> {
        if !self.radius.is_finite() || self.radius < 0.0 {
            return None;
        }
        self.immutable_block_calculator
    }

    fn calculate_immutable_ray_powers(
        &self,
        powers: &[f32; RAY_COUNT],
        calculator: &dyn ImmutableExplosionBlockCalculator,
    ) -> Vec<BlockPos> {
        if let Some(read_radius) = calculator.bounded_block_read_radius()
            && let Some(bounds) = self.immutable_ray_region_bounds(read_radius)
            && let Some(Some(affected)) = self.world.try_with_block_region(bounds, |region| {
                if !region.has_complete_data() {
                    return None;
                }
                let reader = RegionExplosionBlockReader::new(region);
                self.calculate_immutable_ray_powers_with_reader(powers, calculator, &reader)
            })
        {
            return affected;
        }

        self.calculate_immutable_ray_powers_uncached_with_reader(
            powers,
            calculator,
            self.world.as_ref(),
        )
    }

    fn calculate_immutable_ray_powers_with_reader<R: ExplosionBlockReader>(
        &self,
        powers: &[f32; RAY_COUNT],
        calculator: &dyn ImmutableExplosionBlockCalculator,
        reader: &R,
    ) -> Option<Vec<BlockPos>> {
        let context = ExplosionRayContext {
            center: self.center,
            bounds: ExplosionWorldBounds::from_world(self.world),
        };
        let mut affected = JavaBlockPosSet::default();
        let mut cache = ExplosionBlockCache::default();
        let cache_policy = ImmutableRayCachePolicy {
            resistance: calculator.can_cache_explosion_resistance(),
            always_allows_block_explosion: calculator.always_allows_block_explosion(),
        };
        for (&step, &initial_power) in RAY_STEPS.iter().zip(powers) {
            if !visit_immutable_ray_positions_cached(
                ExplosionRay {
                    step,
                    initial_power,
                },
                context,
                reader,
                calculator,
                cache_policy,
                &mut cache,
                &mut affected,
            ) {
                return None;
            }
        }
        Some(affected.into_iter().collect())
    }

    fn calculate_immutable_ray_powers_uncached_with_reader<R: ExplosionBlockReader>(
        &self,
        powers: &[f32; RAY_COUNT],
        calculator: &dyn ImmutableExplosionBlockCalculator,
        reader: &R,
    ) -> Vec<BlockPos> {
        let context = ExplosionRayContext {
            center: self.center,
            bounds: ExplosionWorldBounds::from_world(self.world),
        };
        let mut affected = JavaBlockPosSet::default();
        for (&step, &initial_power) in RAY_STEPS.iter().zip(powers) {
            visit_immutable_ray_positions(
                ExplosionRay {
                    step,
                    initial_power,
                },
                context,
                reader,
                calculator,
                |pos| {
                    affected.insert(pos);
                },
            );
        }
        affected.into_iter().collect()
    }

    fn immutable_ray_region_bounds(&self, read_radius: u32) -> Option<BlockRegionBounds> {
        if !self.center.is_finite() {
            return None;
        }
        let maximum_ray_distance =
            f64::from(self.radius) * f64::from(1.3_f32) / f64::from(RAY_POWER_DECAY) * RAY_STEP;
        let extent = maximum_ray_distance + f64::from(read_radius) + 1.0;
        if !extent.is_finite() {
            return None;
        }
        let extent = DVec3::splat(extent);
        Some(BlockRegionBounds::from_corners(
            BlockPos::from(self.center - extent),
            BlockPos::from(self.center + extent),
        ))
    }

    fn calculate_exploded_positions_sequential(
        &self,
        mut next_float: impl FnMut() -> f32,
    ) -> Vec<BlockPos> {
        let mut affected = JavaBlockPosSet::default();
        let bounds = ExplosionWorldBounds::from_world(self.world);

        for &step in RAY_STEPS.iter() {
            let mut remaining_power = self.radius * (0.7 + next_float() * 0.6);
            let mut ray_pos = self.center;
            while remaining_power > 0.0 {
                let pos = BlockPos::from(ray_pos);
                let state = self.world.get_block_state(pos);
                let fluid = state.get_fluid_state();
                if !bounds.contains(pos) {
                    break;
                }

                if let Some(resistance) = self
                    .damage_calculator
                    .block_explosion_resistance(self, self.world, pos, state, fluid)
                {
                    remaining_power -= (resistance + 0.3) * 0.3;
                }

                if remaining_power > 0.0
                    && self.damage_calculator.should_block_explode(
                        self,
                        self.world,
                        pos,
                        state,
                        remaining_power,
                    )
                {
                    affected.insert(pos);
                }

                ray_pos += step;
                remaining_power -= RAY_POWER_DECAY;
            }
        }

        affected.into_iter().collect()
    }

    fn draw_immutable_ray_powers(&self, mut next_float: impl FnMut() -> f32) -> [f32; RAY_COUNT] {
        let mut powers = [0.0; RAY_COUNT];
        for power in &mut powers {
            *power = self.radius * (0.7 + next_float() * 0.6);
        }
        powers
    }

    #[cfg(test)]
    fn draw_immutable_rays(&self, next_float: impl FnMut() -> f32) -> Vec<ExplosionRay> {
        let powers = self.draw_immutable_ray_powers(next_float);
        RAY_STEPS
            .iter()
            .copied()
            .zip(powers)
            .map(|(step, initial_power)| ExplosionRay {
                step,
                initial_power,
            })
            .collect()
    }

    fn hurt_entities(&mut self) {
        if self.radius < MIN_DAMAGE_RADIUS {
            return;
        }

        let double_radius = self.radius * 2.0;
        let radius = f64::from(double_radius);
        let bounds = WorldAabb::from_min_max(
            DVec3::new(
                (self.center.x - radius - 1.0).floor(),
                (self.center.y - radius - 1.0).floor(),
                (self.center.z - radius - 1.0).floor(),
            ),
            DVec3::new(
                (self.center.x + radius + 1.0).floor(),
                (self.center.y + radius + 1.0).floor(),
                (self.center.z + radius + 1.0).floor(),
            ),
        );
        let source_id = self.source.map(Entity::id);
        let entities = self.world.get_entities_in_aabb_matching(&bounds, |entity| {
            source_id != Some(entity.id()) && !entity.is_spectator()
        });
        let redirect_owner = self.damage_source.causing_entity_id.and_then(|owner_id| {
            self.indirect_source
                .as_ref()
                .filter(|owner| owner.id() == owner_id)
                .cloned()
                .or_else(|| self.world.get_entity_by_id(owner_id))
        });
        let builtin_entity_effects = self.damage_calculator.has_builtin_entity_effects();
        let mut exposure_raycast =
            ExplosionExposureRaycast::new(self.world.as_ref(), BlockCollisionContext::empty());
        // The freshly constructed cache is already safe for the first target.
        let mut reusable_from_previous_tnt = true;

        for entity in entities {
            // Exact Steel PrimedTNT rejects damage, keeps the base no-op explosion callback, and
            // only accepts the impulse. With built-in entity effects, no block mutation can occur
            // between these targets, so their static exposure shapes remain current.
            let inert_primed_tnt = builtin_entity_effects
                && steel_utils::Downcast::downcast_ref::<PrimedTntEntity>(entity.as_ref())
                    .is_some();
            if !reusable_from_previous_tnt || !inert_primed_tnt {
                exposure_raycast.clear();
            }
            reusable_from_previous_tnt = inert_primed_tnt;

            if entity.ignore_explosion(self) {
                continue;
            }
            let distance = entity.position().distance(self.center) / radius;
            if distance > 1.0 {
                continue;
            }

            let delta = entity.explosion_damage_origin() - self.center;
            let delta_length = delta.length();
            let direction = if delta_length < NORMALIZE_EPSILON {
                DVec3::ZERO
            } else {
                delta / delta_length
            };
            let should_damage = self
                .damage_calculator
                .should_damage_entity(self, entity.as_ref());
            let knockback_multiplier = self.damage_calculator.knockback_multiplier(entity.as_ref());
            let exposure = if !should_damage && knockback_multiplier == 0.0 {
                0.0
            } else {
                EntityExplosionExposure::capture(entity.as_ref())
                    .calculate_cached_with(&mut exposure_raycast, self.center)
            };

            if should_damage {
                let amount =
                    self.damage_calculator
                        .entity_damage_amount(self, entity.as_ref(), exposure);
                entity.hurt(self.world, &self.damage_source, amount);
            }

            let knockback_resistance = entity.as_living_entity().map_or(0.0, |living| {
                living
                    .attributes()
                    .lock()
                    .required_value(vanilla_attributes::EXPLOSION_KNOCKBACK_RESISTANCE)
            });
            let knockback_power = (1.0 - distance)
                * f64::from(exposure)
                * f64::from(knockback_multiplier)
                * (1.0 - knockback_resistance);
            let knockback = direction * knockback_power;
            entity.push_impulse(knockback);

            if REGISTRY.entity_types.is_in_tag(
                entity.entity_type(),
                &EntityTypeTag::REDIRECTABLE_PROJECTILE,
            ) {
                if let Some(projectile) = entity.as_projectile() {
                    projectile.set_owner_entity(redirect_owner.as_ref());
                }
            } else if let Some(player) = entity.as_player()
                && !player.is_spectator()
                && (player.game_mode() != GameType::Creative || !player.abilities.lock().flying)
            {
                self.hit_players.insert(player.id(), knockback);
            }

            entity.on_explosion_hit(self.source);
        }
    }

    fn interact_with_blocks(&self, affected: &mut [BlockPos]) {
        self.world.with_random(|random| {
            vanilla_shuffle(affected, |bound| random.next_i32_bounded(bound));
        });
        let mut stacks = Vec::new();
        let mut full_chunks = LocalFullChunkHolderCache::new();

        for &pos in affected.iter() {
            let state = self
                .world
                .get_block_state_with_local_holder_cache(pos, &mut full_chunks);
            BLOCK_BEHAVIORS
                .get_behavior(state.get_block())
                .on_explosion_hit(state, self.world, pos, self, &mut |stack, stack_pos| {
                    add_or_append_stack(&mut stacks, stack, stack_pos);
                });
        }

        for stack in stacks {
            self.world.pop_resource(stack.pos, stack.stack);
        }
    }

    fn create_fire(&self, affected: &[BlockPos]) {
        self.create_fire_with(affected, || {
            self.world.with_random(|random| random.next_i32_bounded(3))
        });
    }

    fn create_fire_with(&self, affected: &[BlockPos], mut next_int: impl FnMut() -> i32) {
        for &pos in affected {
            if next_int() == 0
                && self.world.get_block_state(pos).is_air()
                && self.world.get_block_state(pos.below()).is_solid_render()
            {
                self.world.set_block(
                    pos,
                    FireBlock::get_state(self.world.as_ref(), pos),
                    UpdateFlags::UPDATE_ALL,
                );
            }
        }
    }

    fn interacts_with_blocks(&self) -> bool {
        self.block_interaction != BlockInteraction::Keep
    }

    pub(super) fn is_small(&self) -> bool {
        self.radius < 2.0 || !self.interacts_with_blocks()
    }
}

fn ray_direction(xx: i32, yy: i32, zz: i32) -> DVec3 {
    let mut xd = f64::from(xx as f32 / 15.0 * 2.0 - 1.0);
    let mut yd = f64::from(yy as f32 / 15.0 * 2.0 - 1.0);
    let mut zd = f64::from(zz as f32 / 15.0 * 2.0 - 1.0);
    let direction_length = (xd * xd + yd * yd + zd * zd).sqrt();
    xd /= direction_length;
    yd /= direction_length;
    zd /= direction_length;
    DVec3::new(xd, yd, zd)
}

const fn is_boundary_ray(xx: i32, yy: i32, zz: i32) -> bool {
    xx == 0
        || xx == RAY_GRID_SIZE - 1
        || yy == 0
        || yy == RAY_GRID_SIZE - 1
        || zz == 0
        || zz == RAY_GRID_SIZE - 1
}

fn vanilla_shuffle<T>(values: &mut [T], mut next_index: impl FnMut(i32) -> i32) {
    let Ok(length) = i32::try_from(values.len()) else {
        return;
    };
    for remaining in (2..=length).rev() {
        let swap_index = next_index(remaining) as usize;
        values.swap(remaining as usize - 1, swap_index);
    }
}

#[cfg(test)]
fn calculate_immutable_rays_sharded<R: ExplosionBlockReader>(
    rays: &[ExplosionRay],
    context: ExplosionRayContext,
    reader: &R,
    calculator: &dyn ImmutableExplosionBlockCalculator,
    target_shards: usize,
) -> Vec<BlockPos> {
    if rays.is_empty() {
        return Vec::new();
    }
    let target_shards = target_shards.clamp(1, rays.len());
    let rays_per_shard = rays.len().div_ceil(target_shards);
    let batches: Vec<Vec<BlockPos>> = rays
        .par_chunks(rays_per_shard)
        .map(|rays| {
            let initial_capacity = rays.len() * 8;
            let mut seen = FxHashSet::with_capacity_and_hasher(initial_capacity, FxBuildHasher);
            let mut affected = Vec::with_capacity(initial_capacity);
            for ray in rays {
                visit_immutable_ray_positions(*ray, context, reader, calculator, |pos| {
                    if seen.insert(pos) {
                        affected.push(pos);
                    }
                });
            }
            affected
        })
        .collect();
    unique_affected_positions(batches)
}

#[cfg(test)]
fn calculate_immutable_rays_sequential<R: ExplosionBlockReader>(
    rays: &[ExplosionRay],
    context: ExplosionRayContext,
    reader: &R,
    calculator: &dyn ImmutableExplosionBlockCalculator,
) -> Vec<BlockPos> {
    let mut affected = JavaBlockPosSet::default();
    let mut cache = ExplosionBlockCache::default();
    let cache_policy = ImmutableRayCachePolicy {
        resistance: calculator.can_cache_explosion_resistance(),
        always_allows_block_explosion: calculator.always_allows_block_explosion(),
    };
    for ray in rays {
        assert!(visit_immutable_ray_positions_cached(
            *ray,
            context,
            reader,
            calculator,
            cache_policy,
            &mut cache,
            &mut affected,
        ));
    }
    affected.into_iter().collect()
}

fn visit_immutable_ray_positions_cached<R: ExplosionBlockReader>(
    ray: ExplosionRay,
    context: ExplosionRayContext,
    reader: &R,
    calculator: &dyn ImmutableExplosionBlockCalculator,
    cache_policy: ImmutableRayCachePolicy,
    cache: &mut ExplosionBlockCache,
    affected: &mut JavaBlockPosSet,
) -> bool {
    let mut remaining_power = ray.initial_power;
    let mut ray_pos = context.center;
    let mut previous_cell: Option<(BlockPos, usize)> = None;
    while remaining_power > 0.0 {
        let pos = BlockPos::from(ray_pos);
        if let Some((previous, cache_index)) = previous_cell
            && previous == pos
            && cache_policy.resistance
            && cache_policy.always_allows_block_explosion
            && cache.entries[cache_index].affected
        {
            if let Some(resistance) = cache.entries[cache_index].resistance {
                remaining_power -= (resistance + 0.3) * 0.3;
            }
            ray_pos += ray.step;
            remaining_power -= RAY_POWER_DECAY;
            continue;
        }

        let (tag, cache_index) = match previous_cell {
            Some((previous, cache_index)) if previous == pos => {
                (cache.entries[cache_index].tag, cache_index)
            }
            _ => {
                let tag = PackedBlockPos::from(pos).as_raw();
                (tag, explosion_block_cache_index(tag))
            }
        };
        let cached = cache.entries[cache_index];
        let cache_hit = cached.occupied && cached.tag == tag;
        let state = if cache_hit {
            cached.state
        } else {
            let Some(state) = reader.block_state(pos) else {
                return false;
            };
            state
        };
        if !context.bounds.contains(pos) {
            break;
        }

        let resistance = if cache_hit && cache_policy.resistance {
            cached.resistance
        } else {
            let fluid = state.get_fluid_state();
            calculator.explosion_resistance(reader, pos, state, fluid)
        };
        if !cache_hit {
            cache.entries[cache_index] = ExplosionBlockCacheEntry {
                tag,
                state,
                resistance: if cache_policy.resistance {
                    resistance
                } else {
                    None
                },
                occupied: true,
                affected: false,
            };
        }
        previous_cell = Some((pos, cache_index));

        if let Some(resistance) = resistance {
            remaining_power -= (resistance + 0.3) * 0.3;
        }

        if remaining_power > 0.0 {
            let already_affected = cache.entries[cache_index].affected;
            let should_explode = if cache_policy.always_allows_block_explosion {
                !already_affected
            } else {
                calculator.should_explode(reader, pos, state, remaining_power)
            };
            if should_explode && !already_affected {
                affected.insert(pos);
                cache.entries[cache_index].affected = true;
            }
        }

        ray_pos += ray.step;
        remaining_power -= RAY_POWER_DECAY;
    }
    true
}

#[inline]
const fn explosion_block_cache_index(tag: i64) -> usize {
    let mut mixed = (tag as u64).wrapping_mul(LONG_HASH_PHI);
    mixed ^= mixed >> 32;
    mixed ^= mixed >> 16;
    (mixed as usize) & BLOCK_CACHE_MASK
}

#[derive(Default)]
struct JavaBlockPosSet {
    buckets: Vec<SmallVec<[BlockPos; 2]>>,
    len: usize,
}

impl JavaBlockPosSet {
    fn insert(&mut self, pos: BlockPos) -> bool {
        if self.buckets.is_empty() {
            self.buckets.resize_with(16, SmallVec::new);
        }
        let index = java_block_pos_bucket(pos, self.buckets.len());
        if self.buckets[index].contains(&pos) {
            return false;
        }
        self.buckets[index].push(pos);
        // HashMap attempts to treeify after adding a ninth entry to one bin, but grows the
        // table instead while its capacity is below 64. That split changes iteration order.
        if self.buckets.len() < JAVA_HASH_MAP_MIN_TREEIFY_CAPACITY
            && self.buckets[index].len() > JAVA_HASH_MAP_TREEIFY_THRESHOLD
        {
            self.resize();
        }
        self.len += 1;
        if self.len > self.buckets.len() * 3 / 4 {
            self.resize();
        }
        true
    }

    fn resize(&mut self) {
        let new_capacity = self.buckets.len().saturating_mul(2);
        if new_capacity == self.buckets.len() {
            return;
        }
        let mut resized = Vec::with_capacity(new_capacity);
        resized.resize_with(new_capacity, SmallVec::new);
        for bucket in self.buckets.drain(..) {
            for pos in bucket {
                let index = java_block_pos_bucket(pos, new_capacity);
                resized[index].push(pos);
            }
        }
        self.buckets = resized;
    }
}

impl IntoIterator for JavaBlockPosSet {
    type Item = BlockPos;
    type IntoIter = IntoIter<BlockPos>;

    fn into_iter(self) -> Self::IntoIter {
        self.buckets
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .into_iter()
    }
}

const fn java_block_pos_bucket(pos: BlockPos, capacity: usize) -> usize {
    let hash = pos
        .y()
        .wrapping_add(pos.z().wrapping_mul(31))
        .wrapping_mul(31)
        .wrapping_add(pos.x()) as u32;
    let spread = hash ^ (hash >> 16);
    spread as usize & (capacity - 1)
}

fn visit_immutable_ray_positions<R: ExplosionBlockReader>(
    ray: ExplosionRay,
    context: ExplosionRayContext,
    reader: &R,
    calculator: &dyn ImmutableExplosionBlockCalculator,
    mut visit: impl FnMut(BlockPos),
) {
    let mut remaining_power = ray.initial_power;
    let mut ray_pos = context.center;
    while remaining_power > 0.0 {
        let pos = BlockPos::from(ray_pos);
        let Some(state) = reader.block_state(pos) else {
            return;
        };
        let fluid = state.get_fluid_state();
        if !context.bounds.contains(pos) {
            break;
        }

        if let Some(resistance) = calculator.explosion_resistance(reader, pos, state, fluid) {
            remaining_power -= (resistance + 0.3) * 0.3;
        }

        if remaining_power > 0.0 && calculator.should_explode(reader, pos, state, remaining_power) {
            visit(pos);
        }

        ray_pos += ray.step;
        remaining_power -= RAY_POWER_DECAY;
    }
}

#[cfg(test)]
fn unique_affected_positions(batches: impl IntoIterator<Item = Vec<BlockPos>>) -> Vec<BlockPos> {
    let mut affected = FxHashSet::default();
    for batch in batches {
        for pos in batch {
            affected.insert(pos);
        }
    }
    affected.into_iter().collect()
}

impl Explosion for ServerExplosion<'_> {
    fn world(&self) -> &Arc<World> {
        self.world
    }

    fn damage_source(&self) -> &DamageSource {
        &self.damage_source
    }

    fn block_interaction(&self) -> BlockInteraction {
        self.block_interaction
    }

    fn indirect_source_entity(&self) -> Option<&dyn Entity> {
        self.source
            .filter(|source| source.as_living_entity().is_some())
            .or(self.indirect_source.as_deref())
    }

    fn direct_source_entity(&self) -> Option<&dyn Entity> {
        self.source
    }

    fn radius(&self) -> f32 {
        self.radius
    }

    fn center(&self) -> DVec3 {
        self.center
    }

    fn can_trigger_blocks(&self) -> bool {
        if self.block_interaction != BlockInteraction::TriggerBlock {
            return false;
        }
        self.source.is_none_or(|source| {
            source.entity_type() != &vanilla_entities::BREEZE_WIND_CHARGE
                || self.world.get_game_rule(&MOB_GRIEFING)
        })
    }

    fn should_affect_blocklike_entities(&self) -> bool {
        let is_wind_charge = self.source.is_some_and(|source| {
            source.entity_type() == &vanilla_entities::BREEZE_WIND_CHARGE
                || source.entity_type() == &vanilla_entities::WIND_CHARGE
        });
        !is_wind_charge
            && (self.world.get_game_rule(&MOB_GRIEFING)
                || self.block_interaction.should_affect_blocklike_entities())
    }
}

fn default_explosion_damage_source(
    direct: Option<&dyn Entity>,
    indirect: Option<&dyn Entity>,
) -> DamageSource {
    let damage_type = if direct.is_some() && indirect.is_some() {
        &vanilla_damage_types::PLAYER_EXPLOSION
    } else {
        &vanilla_damage_types::EXPLOSION
    };
    let mut source = DamageSource::environment(damage_type);
    if let Some(entity) = direct {
        source = source
            .with_direct_entity(entity.id())
            .with_source_position(entity.position());
    }
    if let Some(entity) = indirect {
        source = source.with_causing_entity(entity.id());
    }
    source
}

#[derive(Clone, Copy)]
struct EntityExplosionExposure {
    bounding_box: WorldAabb,
    collision_context: BlockCollisionContext,
    x_step: f64,
    y_step: f64,
    z_step: f64,
    x_offset: f64,
    z_offset: f64,
}

impl EntityExplosionExposure {
    fn capture(entity: &dyn Entity) -> Self {
        let bounding_box = entity.bounding_box();
        let x_step = 1.0 / (bounding_box.width() * 2.0 + 1.0);
        let y_step = 1.0 / (bounding_box.height() * 2.0 + 1.0);
        let z_step = 1.0 / (bounding_box.depth() * 2.0 + 1.0);
        let collision_context =
            BlockCollisionContext::entity(entity.position().y, entity.is_descending())
                .with_fall_distance(entity.fall_distance())
                .with_can_walk_on_powder_snow(PowderSnowBlock::can_entity_walk_on_powder_snow(
                    entity,
                ))
                .with_falling_block(entity.entity_type() == &vanilla_entities::FALLING_BLOCK);

        Self {
            bounding_box,
            collision_context,
            x_step,
            y_step,
            z_step,
            x_offset: (1.0 - (1.0 / x_step).floor() * x_step) / 2.0,
            z_offset: (1.0 - (1.0 / z_step).floor() * z_step) / 2.0,
        }
    }

    const fn has_negative_step(self) -> bool {
        self.x_step < 0.0 || self.y_step < 0.0 || self.z_step < 0.0
    }

    fn for_each_sample(self, mut visit: impl FnMut(DVec3)) -> usize {
        let mut sample_count = 0;
        let mut x = 0.0;
        while x <= 1.0 {
            let mut y = 0.0;
            while y <= 1.0 {
                let mut z = 0.0;
                while z <= 1.0 {
                    let from = DVec3::new(
                        self.bounding_box.min_x()
                            + (self.bounding_box.max_x() - self.bounding_box.min_x()) * x
                            + self.x_offset,
                        self.bounding_box.min_y()
                            + (self.bounding_box.max_y() - self.bounding_box.min_y()) * y,
                        self.bounding_box.min_z()
                            + (self.bounding_box.max_z() - self.bounding_box.min_z()) * z
                            + self.z_offset,
                    );
                    visit(from);
                    sample_count += 1;
                    z += self.z_step;
                }
                y += self.y_step;
            }
            x += self.x_step;
        }
        sample_count
    }

    #[cfg(test)]
    fn sample_positions(self) -> SmallVec<[DVec3; 32]> {
        let mut samples = SmallVec::new();
        self.for_each_sample(|sample| samples.push(sample));
        samples
    }

    #[cfg(test)]
    #[inline]
    fn sample_is_visible(self, world: &World, center: DVec3, from: DVec3) -> bool {
        world.is_block_collision_path_clear(from, center, self.collision_context)
    }

    fn exposure(visible_samples: u32, sample_count: usize) -> f32 {
        visible_samples as f32 / sample_count as f32
    }

    #[cfg(test)]
    fn calculate_uncached(self, world: &World, center: DVec3) -> f32 {
        if self.has_negative_step() {
            return 0.0;
        }

        self.calculate_with_visibility(|from| self.sample_is_visible(world, center, from))
    }

    fn calculate_with_visibility(self, mut is_visible: impl FnMut(DVec3) -> bool) -> f32 {
        let mut visible_samples = 0;
        let sample_count = self.for_each_sample(|from| {
            if is_visible(from) {
                visible_samples += 1;
            }
        });
        Self::exposure(visible_samples, sample_count)
    }

    #[cfg(test)]
    fn calculate_cached(self, world: &World, center: DVec3) -> f32 {
        if self.has_negative_step() {
            return 0.0;
        }

        let mut raycast = ExplosionExposureRaycast::new(world, self.collision_context);
        self.calculate_cached_with(&mut raycast, center)
    }

    fn calculate_cached_with(
        self,
        raycast: &mut ExplosionExposureRaycast<'_>,
        center: DVec3,
    ) -> f32 {
        if self.has_negative_step() {
            return 0.0;
        }
        raycast.set_collision_context(self.collision_context);
        self.calculate_with_visibility(|from| raycast.is_path_clear(from, center))
    }
}

#[cfg(test)]
fn seen_percent(world: &World, center: DVec3, entity: &dyn Entity) -> f32 {
    let exposure = EntityExplosionExposure::capture(entity);
    if exposure.has_negative_step() {
        return 0.0;
    }
    exposure.calculate_cached(world, center)
}

struct StackCollector {
    pos: BlockPos,
    stack: ItemStack,
}

fn add_or_append_stack(stacks: &mut Vec<StackCollector>, mut stack: ItemStack, pos: BlockPos) {
    for collector in stacks.iter_mut() {
        if ItemEntity::are_mergeable(&collector.stack, &stack) {
            let available = collector
                .stack
                .max_stack_size()
                .min(MAX_DROPS_PER_COMBINED_STACK)
                - collector.stack.count();
            let transferred = available.min(stack.count());
            collector.stack = collector
                .stack
                .copy_with_count(collector.stack.count() + transferred);
            stack.shrink(transferred);
            if stack.is_empty() {
                return;
            }
        }
    }
    stacks.push(StackCollector { pos, stack });
}

#[cfg(test)]
mod tests;
