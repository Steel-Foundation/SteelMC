use std::{
    mem,
    sync::{Arc, LazyLock},
    vec::IntoIter,
};

use glam::DVec3;
use rustc_hash::FxHashMap;
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
use crate::chunk::paletted_container::BlockPalette;
use crate::entity::damage::DamageSource;
use crate::entity::entities::{ItemEntity, PrimedTntEntity};
use crate::entity::{Entity, SharedEntity};
use crate::world::game_event::GameEventContext;
use crate::world::raycast::{ExplosionExposureRaycast, collision_path_axis_block_bounds};
use crate::world::{BlockRegionBounds, BlockRegionRead, MAX_BLOCK_REGION_WORKSET_SLOTS, World};

use super::{
    BlockInteraction, Explosion, ExplosionBlockReader, ExplosionDamageCalculator,
    ImmutableExplosionBlockCalculator, SelectedDamageCalculator,
};

const RAY_GRID_SIZE: i32 = 16;
const RAY_GRID_LAST_INDEX: i32 = RAY_GRID_SIZE - 1;
const RAY_GRID_INTERIOR_SIZE: i32 = RAY_GRID_SIZE - 2;
const RAY_COUNT: usize = (RAY_GRID_SIZE * RAY_GRID_SIZE * RAY_GRID_SIZE
    - RAY_GRID_INTERIOR_SIZE * RAY_GRID_INTERIOR_SIZE * RAY_GRID_INTERIOR_SIZE)
    as usize;
const RAY_STEP: f64 = 0.3_f32 as f64;
const RAY_POWER_DECAY: f32 = 0.225_000_01;
const INITIAL_RAY_POWER_BASE: f32 = 0.7;
const INITIAL_RAY_POWER_RANDOM_SCALE: f32 = 0.6;
const MAX_INITIAL_RAY_POWER_SCALE: f32 = 1.3;
const RESISTANCE_POWER_OFFSET: f32 = 0.3;
const RESISTANCE_POWER_SCALE: f32 = 0.3;
const RAY_REGION_BLOCK_PADDING: f64 = 1.0;
const MIN_DAMAGE_RADIUS: f32 = 1.0e-5;
const DAMAGE_RADIUS_SCALE: f32 = 2.0;
const ENTITY_QUERY_PADDING: f64 = 1.0;
const NORMALIZE_EPSILON: f64 = 1.0e-5_f32 as f64;
const FIRE_CHANCE_DENOMINATOR: i32 = 3;
const SMALL_EXPLOSION_RADIUS: f32 = 2.0;
const EXPOSURE_SAMPLE_DENSITY: f64 = 2.0;
const EXPOSURE_SAMPLE_OFFSET_DIVISOR: f64 = 2.0;
const MAX_EXPOSURE_CERTIFICATE_AXIS_WORK: usize =
    MAX_BLOCK_REGION_WORKSET_SLOTS * BlockPalette::SIZE;
const MAX_DROPS_PER_COMBINED_STACK: i32 = 16;
const BLOCK_CACHE_BITS: u32 = 9;
const BLOCK_CACHE_SIZE: usize = 1 << BLOCK_CACHE_BITS;
const BLOCK_CACHE_MASK: usize = BLOCK_CACHE_SIZE - 1;
/// Bounds the temporary dense-cache allocations while covering standard radius-four explosions.
const MAX_DENSE_BLOCK_CACHE_CELLS: usize = 8_192;
const EMPTY_DENSE_BLOCK_CACHE_SLOT: u16 = u16::MAX;
const DENSE_BLOCK_CACHE_HAS_RESISTANCE: u8 = 1;
const DENSE_BLOCK_CACHE_AFFECTED: u8 = 1 << 1;
const F64_INTEGER_MANTISSA_BIAS: f64 = 6_755_399_441_055_744.0;
const DENSE_BLOCK_CACHE_ENTRY_SIZE_BYTES: usize = 8;
const LONG_HASH_PHI: u64 = 0x9e37_79b9_7f4a_7c15;
const JAVA_HASH_MAP_TREEIFY_THRESHOLD: usize = 8;
const JAVA_HASH_MAP_MIN_TREEIFY_CAPACITY: usize = 64;
const JAVA_HASH_MAP_LOAD_FACTOR_NUMERATOR: usize = 3;
const JAVA_HASH_MAP_LOAD_FACTOR_DENOMINATOR: usize = 4;
const JAVA_BLOCK_POS_HASH_MULTIPLIER: i32 = 31;
const JAVA_HASH_MAP_SPREAD_SHIFT: u32 = 16;

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

#[repr(C)]
#[derive(Clone, Copy)]
struct DenseExplosionBlockCacheEntry {
    resistance: f32,
    state: BlockStateId,
    flags: u8,
    _padding: u8,
}

const _: [(); DENSE_BLOCK_CACHE_ENTRY_SIZE_BYTES] =
    [(); mem::size_of::<DenseExplosionBlockCacheEntry>()];

struct DenseExplosionBlockCache {
    min: BlockPos,
    size_x: usize,
    size_y: usize,
    size_z: usize,
    slots: Vec<u16>,
    entries: Vec<DenseExplosionBlockCacheEntry>,
}

#[derive(Clone, Copy)]
enum ExplosionBlockCacheLookup<Miss> {
    Hit(usize),
    Miss(Miss),
}

trait ExplosionRayBlockCache {
    type Miss: Copy;

    fn lookup(&self, pos: BlockPos) -> Option<ExplosionBlockCacheLookup<Self::Miss>>;
    fn state(&self, entry_index: usize) -> BlockStateId;
    fn resistance(&self, entry_index: usize) -> Option<f32>;
    fn affected(&self, entry_index: usize) -> bool;
    fn insert(&mut self, miss: Self::Miss, state: BlockStateId, resistance: Option<f32>) -> usize;
    fn mark_affected(&mut self, entry_index: usize);
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

impl ExplosionRayBlockCache for ExplosionBlockCache {
    type Miss = (usize, i64);

    #[inline]
    fn lookup(&self, pos: BlockPos) -> Option<ExplosionBlockCacheLookup<Self::Miss>> {
        let tag = PackedBlockPos::from(pos).as_raw();
        let cache_index = explosion_block_cache_index(tag);
        let entry = self.entries[cache_index];
        Some(if entry.occupied && entry.tag == tag {
            ExplosionBlockCacheLookup::Hit(cache_index)
        } else {
            ExplosionBlockCacheLookup::Miss((cache_index, tag))
        })
    }

    #[inline]
    fn state(&self, entry_index: usize) -> BlockStateId {
        self.entries[entry_index].state
    }

    #[inline]
    fn resistance(&self, entry_index: usize) -> Option<f32> {
        self.entries[entry_index].resistance
    }

    #[inline]
    fn affected(&self, entry_index: usize) -> bool {
        self.entries[entry_index].affected
    }

    #[inline]
    fn insert(
        &mut self,
        (cache_index, tag): Self::Miss,
        state: BlockStateId,
        resistance: Option<f32>,
    ) -> usize {
        self.entries[cache_index] = ExplosionBlockCacheEntry {
            tag,
            state,
            resistance,
            occupied: true,
            affected: false,
        };
        cache_index
    }

    #[inline]
    fn mark_affected(&mut self, entry_index: usize) {
        self.entries[entry_index].affected = true;
    }
}

impl DenseExplosionBlockCache {
    fn try_new(bounds: BlockRegionBounds) -> Option<Self> {
        let (min, max) = bounds.corners();
        let size_x = inclusive_block_count(min.x(), max.x())?;
        let size_y = inclusive_block_count(min.y(), max.y())?;
        let size_z = inclusive_block_count(min.z(), max.z())?;
        let volume = size_x.checked_mul(size_y)?.checked_mul(size_z)?;
        if volume > MAX_DENSE_BLOCK_CACHE_CELLS
            || volume >= usize::from(EMPTY_DENSE_BLOCK_CACHE_SLOT)
        {
            return None;
        }

        Some(Self {
            min,
            size_x,
            size_y,
            size_z,
            slots: vec![EMPTY_DENSE_BLOCK_CACHE_SLOT; volume],
            entries: Vec::with_capacity(volume),
        })
    }

    #[inline]
    fn cell_index(&self, pos: BlockPos) -> Option<usize> {
        let x = usize::try_from(i64::from(pos.x()) - i64::from(self.min.x())).ok()?;
        let y = usize::try_from(i64::from(pos.y()) - i64::from(self.min.y())).ok()?;
        let z = usize::try_from(i64::from(pos.z()) - i64::from(self.min.z())).ok()?;
        if x >= self.size_x || y >= self.size_y || z >= self.size_z {
            return None;
        }
        Some((y * self.size_z + z) * self.size_x + x)
    }
}

impl ExplosionRayBlockCache for DenseExplosionBlockCache {
    type Miss = usize;

    #[inline]
    fn lookup(&self, pos: BlockPos) -> Option<ExplosionBlockCacheLookup<Self::Miss>> {
        let slot_index = self.cell_index(pos)?;
        let entry_index = self.slots[slot_index];
        Some(if entry_index == EMPTY_DENSE_BLOCK_CACHE_SLOT {
            ExplosionBlockCacheLookup::Miss(slot_index)
        } else {
            ExplosionBlockCacheLookup::Hit(usize::from(entry_index))
        })
    }

    #[inline]
    fn state(&self, entry_index: usize) -> BlockStateId {
        self.entries[entry_index].state
    }

    #[inline]
    fn resistance(&self, entry_index: usize) -> Option<f32> {
        let entry = self.entries[entry_index];
        (entry.flags & DENSE_BLOCK_CACHE_HAS_RESISTANCE != 0).then_some(entry.resistance)
    }

    #[inline]
    fn affected(&self, entry_index: usize) -> bool {
        self.entries[entry_index].flags & DENSE_BLOCK_CACHE_AFFECTED != 0
    }

    #[inline]
    fn insert(
        &mut self,
        slot_index: Self::Miss,
        state: BlockStateId,
        resistance: Option<f32>,
    ) -> usize {
        let entry_index = self.entries.len();
        debug_assert!(entry_index < usize::from(EMPTY_DENSE_BLOCK_CACHE_SLOT));
        let (resistance, flags) = resistance.map_or((0.0, 0), |resistance| {
            (resistance, DENSE_BLOCK_CACHE_HAS_RESISTANCE)
        });
        self.entries.push(DenseExplosionBlockCacheEntry {
            resistance,
            state,
            flags,
            _padding: 0,
        });
        self.slots[slot_index] = entry_index as u16;
        entry_index
    }

    #[inline]
    fn mark_affected(&mut self, entry_index: usize) {
        self.entries[entry_index].flags |= DENSE_BLOCK_CACHE_AFFECTED;
    }
}

fn inclusive_block_count(min: i32, max: i32) -> Option<usize> {
    usize::try_from(i64::from(max) - i64::from(min) + 1).ok()
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
    // Keep Vanilla's X/Y/Z traversal order: random ray powers are consumed in this order.
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

impl ExplosionRayContext {
    /// Proves the cached traversal's current and first out-of-region samples fit in `i32`.
    /// Every component is bounded by [`RAY_STEP`]. One-cell headroom on each face therefore covers
    /// the sample that makes the bounded reader request the generic fallback.
    fn can_use_bounded_floor(self, region_bounds: BlockRegionBounds) -> bool {
        let (min, max) = region_bounds.corners();
        self.center.is_finite()
            && ray_axis_has_bounded_floor(self.center.x, min.x(), max.x())
            && ray_axis_has_bounded_floor(self.center.y, min.y(), max.y())
            && ray_axis_has_bounded_floor(self.center.z, min.z(), max.z())
    }
}

fn ray_axis_has_bounded_floor(center: f64, min: i32, max: i32) -> bool {
    min > i32::MIN && max < i32::MAX && center >= f64::from(min) && center < f64::from(max) + 1.0
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
        let damage_source = damage_source.unwrap_or_else(|| {
            let mut damage_source = default_explosion_damage_source(source, indirect_source_entity);
            if let Some(indirect_source) = &indirect_source {
                damage_source = damage_source.with_causing_entity_reference(indirect_source);
            }
            damage_source
        });
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
                self.calculate_immutable_ray_powers_with_reader(powers, calculator, &reader, bounds)
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
        bounds: BlockRegionBounds,
    ) -> Option<Vec<BlockPos>> {
        let context = ExplosionRayContext {
            center: self.center,
            bounds: ExplosionWorldBounds::from_world(self.world),
        };
        let cache_policy = ImmutableRayCachePolicy {
            resistance: calculator.can_cache_explosion_resistance(),
            always_allows_block_explosion: calculator.always_allows_block_explosion(),
        };
        let use_bounded_floor = context.can_use_bounded_floor(bounds);

        if cache_policy.resistance
            && cache_policy.always_allows_block_explosion
            && let Some(cache) = DenseExplosionBlockCache::try_new(bounds)
        {
            return Self::calculate_immutable_ray_powers_with_cache(
                powers,
                calculator,
                reader,
                context,
                cache_policy,
                cache,
                use_bounded_floor,
            );
        }

        Self::calculate_immutable_ray_powers_with_cache(
            powers,
            calculator,
            reader,
            context,
            cache_policy,
            ExplosionBlockCache::default(),
            use_bounded_floor,
        )
    }

    fn calculate_immutable_ray_powers_with_cache<
        R: ExplosionBlockReader,
        C: ExplosionRayBlockCache,
    >(
        powers: &[f32; RAY_COUNT],
        calculator: &dyn ImmutableExplosionBlockCalculator,
        reader: &R,
        context: ExplosionRayContext,
        cache_policy: ImmutableRayCachePolicy,
        cache: C,
        use_bounded_floor: bool,
    ) -> Option<Vec<BlockPos>> {
        if use_bounded_floor {
            return Self::calculate_immutable_ray_powers_with_cache_mode::<R, C, true>(
                powers,
                calculator,
                reader,
                context,
                cache_policy,
                cache,
            );
        }
        Self::calculate_immutable_ray_powers_with_cache_mode::<R, C, false>(
            powers,
            calculator,
            reader,
            context,
            cache_policy,
            cache,
        )
    }

    fn calculate_immutable_ray_powers_with_cache_mode<
        R: ExplosionBlockReader,
        C: ExplosionRayBlockCache,
        const USE_BOUNDED_FLOOR: bool,
    >(
        powers: &[f32; RAY_COUNT],
        calculator: &dyn ImmutableExplosionBlockCalculator,
        reader: &R,
        context: ExplosionRayContext,
        cache_policy: ImmutableRayCachePolicy,
        mut cache: C,
    ) -> Option<Vec<BlockPos>> {
        let mut affected = JavaBlockPosSet::default();
        for (&step, &initial_power) in RAY_STEPS.iter().zip(powers) {
            if !visit_immutable_ray_positions_cached::<R, C, USE_BOUNDED_FLOOR>(
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
        let maximum_ray_distance = f64::from(self.radius) * f64::from(MAX_INITIAL_RAY_POWER_SCALE)
            / f64::from(RAY_POWER_DECAY)
            * RAY_STEP;
        let extent = maximum_ray_distance + f64::from(read_radius) + RAY_REGION_BLOCK_PADDING;
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
            let mut remaining_power = initial_ray_power(self.radius, next_float());
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
                    remaining_power -= ray_power_loss_from_resistance(resistance);
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
            *power = initial_ray_power(self.radius, next_float());
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

        let double_radius = self.radius * DAMAGE_RADIUS_SCALE;
        let radius = f64::from(double_radius);
        let bounds = WorldAabb::from_min_max(
            DVec3::new(
                (self.center.x - radius - ENTITY_QUERY_PADDING).floor(),
                (self.center.y - radius - ENTITY_QUERY_PADDING).floor(),
                (self.center.z - radius - ENTITY_QUERY_PADDING).floor(),
            ),
            DVec3::new(
                (self.center.x + radius + ENTITY_QUERY_PADDING).floor(),
                (self.center.y + radius + ENTITY_QUERY_PADDING).floor(),
                (self.center.z + radius + ENTITY_QUERY_PADDING).floor(),
            ),
        );
        let source_id = self.source.map(Entity::id);
        let entities = self.world.get_entities_in_aabb_matching(&bounds, |entity| {
            source_id != Some(entity.id()) && !entity.is_spectator()
        });
        let redirect_owner = self.damage_source.causing_entity(self.world);
        let builtin_entity_effects = self.damage_calculator.has_builtin_entity_effects();
        let mut exposure_raycast =
            ExplosionExposureRaycast::new(self.world.as_ref(), BlockCollisionContext::empty());
        exposure_raycast.configure_clear_grid(
            BlockPos::from(bounds.min_corner()),
            BlockPos::from(bounds.max_corner()),
        );
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
            self.world
                .with_random(|random| random.next_i32_bounded(FIRE_CHANCE_DENOMINATOR))
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
        self.radius < SMALL_EXPLOSION_RADIUS || !self.interacts_with_blocks()
    }
}

fn ray_direction(xx: i32, yy: i32, zz: i32) -> DVec3 {
    let mut xd = ray_direction_component(xx);
    let mut yd = ray_direction_component(yy);
    let mut zd = ray_direction_component(zz);
    let direction_length = (xd * xd + yd * yd + zd * zd).sqrt();
    xd /= direction_length;
    yd /= direction_length;
    zd /= direction_length;
    DVec3::new(xd, yd, zd)
}

fn ray_direction_component(index: i32) -> f64 {
    f64::from(index as f32 / RAY_GRID_LAST_INDEX as f32 * 2.0 - 1.0)
}

fn initial_ray_power(radius: f32, random: f32) -> f32 {
    radius * (INITIAL_RAY_POWER_BASE + random * INITIAL_RAY_POWER_RANDOM_SCALE)
}

fn ray_power_loss_from_resistance(resistance: f32) -> f32 {
    (resistance + RESISTANCE_POWER_OFFSET) * RESISTANCE_POWER_SCALE
}

const fn is_boundary_ray(xx: i32, yy: i32, zz: i32) -> bool {
    xx == 0
        || xx == RAY_GRID_LAST_INDEX
        || yy == 0
        || yy == RAY_GRID_LAST_INDEX
        || zz == 0
        || zz == RAY_GRID_LAST_INDEX
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

fn visit_immutable_ray_positions_cached<
    R: ExplosionBlockReader,
    C: ExplosionRayBlockCache,
    const USE_BOUNDED_FLOOR: bool,
>(
    ray: ExplosionRay,
    context: ExplosionRayContext,
    reader: &R,
    calculator: &dyn ImmutableExplosionBlockCalculator,
    cache_policy: ImmutableRayCachePolicy,
    cache: &mut C,
    affected: &mut JavaBlockPosSet,
) -> bool {
    let mut remaining_power = ray.initial_power;
    let mut ray_pos = context.center;
    let mut previous_cell: Option<(BlockPos, usize)> = None;
    while remaining_power > 0.0 {
        let pos = ray_block_pos::<USE_BOUNDED_FLOOR>(ray_pos);
        if let Some((previous, cache_index)) = previous_cell
            && previous == pos
            && cache_policy.resistance
            && cache_policy.always_allows_block_explosion
            && cache.affected(cache_index)
        {
            if let Some(resistance) = cache.resistance(cache_index) {
                remaining_power -= ray_power_loss_from_resistance(resistance);
            }
            ray_pos += ray.step;
            remaining_power -= RAY_POWER_DECAY;
            continue;
        }

        let lookup = match previous_cell {
            Some((previous, cache_index)) if previous == pos => {
                ExplosionBlockCacheLookup::Hit(cache_index)
            }
            _ => {
                let Some(lookup) = cache.lookup(pos) else {
                    return false;
                };
                lookup
            }
        };
        let state = match lookup {
            ExplosionBlockCacheLookup::Hit(cache_index) => cache.state(cache_index),
            ExplosionBlockCacheLookup::Miss(_) => {
                let Some(state) = reader.block_state(pos) else {
                    return false;
                };
                state
            }
        };
        if !context.bounds.contains(pos) {
            break;
        }

        let resistance = match lookup {
            ExplosionBlockCacheLookup::Hit(cache_index) if cache_policy.resistance => {
                cache.resistance(cache_index)
            }
            _ => {
                let fluid = state.get_fluid_state();
                calculator.explosion_resistance(reader, pos, state, fluid)
            }
        };
        let cache_index = match lookup {
            ExplosionBlockCacheLookup::Hit(cache_index) => cache_index,
            ExplosionBlockCacheLookup::Miss(miss) => cache.insert(
                miss,
                state,
                if cache_policy.resistance {
                    resistance
                } else {
                    None
                },
            ),
        };
        previous_cell = Some((pos, cache_index));

        if let Some(resistance) = resistance {
            remaining_power -= ray_power_loss_from_resistance(resistance);
        }

        if remaining_power > 0.0 {
            let already_affected = cache.affected(cache_index);
            let should_explode = if cache_policy.always_allows_block_explosion {
                !already_affected
            } else {
                calculator.should_explode(reader, pos, state, remaining_power)
            };
            if should_explode && !already_affected {
                affected.insert(pos);
                cache.mark_affected(cache_index);
            }
        }

        ray_pos += ray.step;
        remaining_power -= RAY_POWER_DECAY;
    }
    true
}

#[inline]
fn ray_block_pos<const USE_BOUNDED_FLOOR: bool>(position: DVec3) -> BlockPos {
    if USE_BOUNDED_FLOOR {
        BlockPos::new(
            bounded_floor_to_i32(position.x),
            bounded_floor_to_i32(position.y),
            bounded_floor_to_i32(position.z),
        )
    } else {
        BlockPos::from(position)
    }
}

/// Floors a finite in-range coordinate without Rust's saturating float-to-int conversion.
///
/// Adding `1.5 * 2^52` maps every integral binary64 value in the i32 range exactly into the
/// mantissa; its low 32 bits are the integer's two's-complement representation.
#[inline]
fn bounded_floor_to_i32(value: f64) -> i32 {
    debug_assert!(
        value.is_finite() && value >= f64::from(i32::MIN) && value < f64::from(i32::MAX) + 1.0
    );
    ((value.floor() + F64_INTEGER_MANTISSA_BIAS).to_bits() as u32) as i32
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
    buckets: Vec<JavaBlockPosBucket>,
    entries: Vec<JavaBlockPosEntry>,
}

#[derive(Clone, Copy)]
struct JavaBlockPosBucket {
    head: u32,
    tail: u32,
}

impl JavaBlockPosBucket {
    const EMPTY: Self = Self {
        head: JAVA_BLOCK_POS_SET_EMPTY_INDEX,
        tail: JAVA_BLOCK_POS_SET_EMPTY_INDEX,
    };
}

struct JavaBlockPosEntry {
    pos: BlockPos,
    next: u32,
}

impl JavaBlockPosSet {
    fn insert(&mut self, pos: BlockPos) -> bool {
        if self.buckets.is_empty() {
            self.buckets.resize(16, JavaBlockPosBucket::EMPTY);
            self.entries.reserve(16);
        }
        let index = java_block_pos_bucket(pos, self.buckets.len());
        let bucket = self.buckets[index];
        let mut current = bucket.head;
        let mut bin_len = 0;
        while current != JAVA_BLOCK_POS_SET_EMPTY_INDEX {
            let entry = &self.entries[current as usize];
            if entry.pos == pos {
                return false;
            }
            current = entry.next;
            bin_len += 1;
        }

        let Ok(entry_index) = u32::try_from(self.entries.len()) else {
            panic!("JavaBlockPosSet entry arena exceeded its u32 index space");
        };
        assert_ne!(
            entry_index, JAVA_BLOCK_POS_SET_EMPTY_INDEX,
            "JavaBlockPosSet entry arena exhausted its u32 index space"
        );
        self.entries.push(JavaBlockPosEntry {
            pos,
            next: JAVA_BLOCK_POS_SET_EMPTY_INDEX,
        });
        if bucket.tail == JAVA_BLOCK_POS_SET_EMPTY_INDEX {
            self.buckets[index] = JavaBlockPosBucket {
                head: entry_index,
                tail: entry_index,
            };
        } else {
            self.entries[bucket.tail as usize].next = entry_index;
            self.buckets[index].tail = entry_index;
        }

        // HashMap attempts to treeify after adding a ninth entry to one bin, but grows the
        // table instead while its capacity is below 64. That split changes iteration order.
        if self.buckets.len() < JAVA_HASH_MAP_MIN_TREEIFY_CAPACITY
            && bin_len >= JAVA_HASH_MAP_TREEIFY_THRESHOLD
        {
            self.resize();
        }
        // Steel intentionally keeps list bins at larger capacities. HashMap tree-bin order can
        // depend on JVM identity hashes and is not a reproducible Vanilla ordering contract.
        if self.entries.len()
            > self.buckets.len() * JAVA_HASH_MAP_LOAD_FACTOR_NUMERATOR
                / JAVA_HASH_MAP_LOAD_FACTOR_DENOMINATOR
        {
            self.resize();
        }
        true
    }

    fn resize(&mut self) {
        let new_capacity = self.buckets.len().saturating_mul(2);
        if new_capacity == self.buckets.len() {
            return;
        }
        let resized = vec![JavaBlockPosBucket::EMPTY; new_capacity];
        let old_buckets = mem::replace(&mut self.buckets, resized);
        for bucket in old_buckets {
            let mut current = bucket.head;
            while current != JAVA_BLOCK_POS_SET_EMPTY_INDEX {
                let entry_index = current as usize;
                let next = self.entries[entry_index].next;
                let index = java_block_pos_bucket(self.entries[entry_index].pos, new_capacity);
                let new_bucket = self.buckets[index];
                self.entries[entry_index].next = JAVA_BLOCK_POS_SET_EMPTY_INDEX;
                if new_bucket.tail == JAVA_BLOCK_POS_SET_EMPTY_INDEX {
                    self.buckets[index] = JavaBlockPosBucket {
                        head: current,
                        tail: current,
                    };
                } else {
                    self.entries[new_bucket.tail as usize].next = current;
                    self.buckets[index].tail = current;
                }
                current = next;
            }
        }
    }
}

impl IntoIterator for JavaBlockPosSet {
    type Item = BlockPos;
    type IntoIter = IntoIter<BlockPos>;

    fn into_iter(self) -> Self::IntoIter {
        let mut ordered = Vec::with_capacity(self.entries.len());
        for bucket in self.buckets {
            let mut current = bucket.head;
            while current != JAVA_BLOCK_POS_SET_EMPTY_INDEX {
                let entry = &self.entries[current as usize];
                ordered.push(entry.pos);
                current = entry.next;
            }
        }
        ordered.into_iter()
    }
}

const JAVA_BLOCK_POS_SET_EMPTY_INDEX: u32 = u32::MAX;

const fn java_block_pos_bucket(pos: BlockPos, capacity: usize) -> usize {
    let hash = pos
        .y()
        .wrapping_add(pos.z().wrapping_mul(JAVA_BLOCK_POS_HASH_MULTIPLIER))
        .wrapping_mul(JAVA_BLOCK_POS_HASH_MULTIPLIER)
        .wrapping_add(pos.x()) as u32;
    let spread = hash ^ (hash >> JAVA_HASH_MAP_SPREAD_SHIFT);
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
            remaining_power -= ray_power_loss_from_resistance(resistance);
        }

        if remaining_power > 0.0 && calculator.should_explode(reader, pos, state, remaining_power) {
            visit(pos);
        }

        ray_pos += ray.step;
        remaining_power -= RAY_POWER_DECAY;
    }
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
            .with_direct_entity_position(entity.position());
    }
    if let Some(entity) = indirect {
        source = source.with_causing_entity(entity.id());
    }
    source
}

pub(crate) fn default_explosion_damage_source_with_references(
    direct: &SharedEntity,
    indirect: Option<&SharedEntity>,
) -> DamageSource {
    let indirect_entity = indirect.map(|entity| entity.as_ref() as &dyn Entity);
    let mut source = default_explosion_damage_source(Some(direct.as_ref()), indirect_entity)
        .with_direct_entity_reference(direct);
    if let Some(indirect) = indirect {
        source = source.with_causing_entity_reference(indirect);
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
        let x_step = exposure_axis_step(bounding_box.width());
        let y_step = exposure_axis_step(bounding_box.height());
        let z_step = exposure_axis_step(bounding_box.depth());
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
            x_offset: (1.0 - (1.0 / x_step).floor() * x_step) / EXPOSURE_SAMPLE_OFFSET_DIVISOR,
            z_offset: (1.0 - (1.0 / z_step).floor() * z_step) / EXPOSURE_SAMPLE_OFFSET_DIVISOR,
        }
    }

    const fn has_negative_step(self) -> bool {
        self.x_step < 0.0 || self.y_step < 0.0 || self.z_step < 0.0
    }

    fn sample_position(&self, x_fraction: f64, y_fraction: f64, z_fraction: f64) -> DVec3 {
        DVec3::new(
            self.bounding_box.min_x()
                + (self.bounding_box.max_x() - self.bounding_box.min_x()) * x_fraction
                + self.x_offset,
            self.bounding_box.min_y()
                + (self.bounding_box.max_y() - self.bounding_box.min_y()) * y_fraction,
            self.bounding_box.min_z()
                + (self.bounding_box.max_z() - self.bounding_box.min_z()) * z_fraction
                + self.z_offset,
        )
    }

    fn axis_sample_block_bounds(
        axis_min: f64,
        axis_max: f64,
        step: f64,
        offset: f64,
        center: f64,
    ) -> Option<(i32, i32)> {
        let can_sample_axis = axis_min.is_finite()
            && axis_max.is_finite()
            && axis_max >= axis_min
            && step.is_finite()
            && step > 0.0
            && offset.is_finite()
            && center.is_finite();
        if !can_sample_axis {
            return None;
        }

        let axis_length = axis_max - axis_min;
        let mut min_block = i32::MAX;
        let mut max_block = i32::MIN;
        let mut fraction = 0.0;
        for _ in 0..MAX_EXPOSURE_CERTIFICATE_AXIS_WORK {
            if fraction > 1.0 {
                return Some((min_block, max_block));
            }
            let sample = axis_min + axis_length * fraction + offset;
            let (sample_min, sample_max) = collision_path_axis_block_bounds(
                sample,
                center,
                MAX_EXPOSURE_CERTIFICATE_AXIS_WORK,
            )?;
            min_block = min_block.min(sample_min);
            max_block = max_block.max(sample_max);
            fraction += step;
            if !fraction.is_finite() {
                return None;
            }
        }
        None
    }

    /// Builds a Cartesian envelope containing every block visited by all exposure rays.
    ///
    /// Each axis uses Vanilla's repeated-addition sample sequence and exact scalar DDA recurrence.
    /// This is linear in the three axis sample counts instead of their Cartesian product.
    fn stable_air_certificate_bounds(self, center: DVec3) -> Option<BlockRegionBounds> {
        let (min_x, max_x) = Self::axis_sample_block_bounds(
            self.bounding_box.min_x(),
            self.bounding_box.max_x(),
            self.x_step,
            self.x_offset,
            center.x,
        )?;
        let (min_y, max_y) = Self::axis_sample_block_bounds(
            self.bounding_box.min_y(),
            self.bounding_box.max_y(),
            self.y_step,
            0.0,
            center.y,
        )?;
        let (min_z, max_z) = Self::axis_sample_block_bounds(
            self.bounding_box.min_z(),
            self.bounding_box.max_z(),
            self.z_step,
            self.z_offset,
            center.z,
        )?;
        Some(BlockRegionBounds::from_corners(
            BlockPos::new(min_x, min_y, min_z),
            BlockPos::new(max_x, max_y, max_z),
        ))
    }

    fn for_each_sample(self, mut visit: impl FnMut(DVec3)) -> usize {
        let mut sample_count = 0;
        // Repeated addition and inclusive bounds intentionally mirror Vanilla's floating-point
        // sample sequence; deriving each fraction from an integer index can change boundary rays.
        let mut x_fraction = 0.0;
        while x_fraction <= 1.0 {
            let mut y_fraction = 0.0;
            while y_fraction <= 1.0 {
                let mut z_fraction = 0.0;
                while z_fraction <= 1.0 {
                    visit(self.sample_position(x_fraction, y_fraction, z_fraction));
                    sample_count += 1;
                    z_fraction += self.z_step;
                }
                y_fraction += self.y_step;
            }
            x_fraction += self.x_step;
        }
        sample_count
    }

    #[cfg(test)]
    fn sample_positions(self) -> Vec<DVec3> {
        let mut samples = Vec::new();
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
        if let Some(bounds) = self.stable_air_certificate_bounds(center)
            && raycast.stable_air_box_is_clear(bounds)
        {
            return 1.0;
        }
        raycast.set_collision_context(self.collision_context);
        self.calculate_with_visibility(|from| raycast.is_path_clear(from, center))
    }
}

fn exposure_axis_step(axis_length: f64) -> f64 {
    1.0 / (axis_length * EXPOSURE_SAMPLE_DENSITY + 1.0)
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
#[path = "server/ray_tests.rs"]
mod ray_tests;

#[cfg(test)]
mod tests;
