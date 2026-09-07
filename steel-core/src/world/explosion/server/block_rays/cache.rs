//! Bounded block-ray caches for immutable explosion calculators.

use std::mem;

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_utils::{BlockPos, BlockStateId, PackedBlockPos};

use crate::world::BlockRegionBounds;
use crate::world::explosion::{ExplosionBlockReader, ImmutableExplosionBlockCalculator};

use super::super::ServerExplosion;
use super::{
    ExplosionRay, ExplosionRayContext, ExplosionWorldBounds, JavaBlockPosSet, RAY_POWER_DECAY,
    RAY_STEPS, ray_power_loss_from_resistance,
};

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

pub(super) struct ExplosionBlockCache {
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

pub(super) struct DenseExplosionBlockCache {
    min: BlockPos,
    size_x: usize,
    size_y: usize,
    size_z: usize,
    slots: Vec<u16>,
    entries: Vec<DenseExplosionBlockCacheEntry>,
}

#[derive(Clone, Copy)]
pub(super) enum ExplosionBlockCacheLookup<Miss> {
    Hit(usize),
    Miss(Miss),
}

pub(super) trait ExplosionRayBlockCache {
    type Miss: Copy;

    fn lookup(&self, pos: BlockPos) -> Option<ExplosionBlockCacheLookup<Self::Miss>>;
    fn state(&self, entry_index: usize) -> BlockStateId;
    fn resistance(&self, entry_index: usize) -> Option<f32>;
    fn affected(&self, entry_index: usize) -> bool;
    fn insert(&mut self, miss: Self::Miss, state: BlockStateId, resistance: Option<f32>) -> usize;
    fn mark_affected(&mut self, entry_index: usize);
}

#[derive(Clone, Copy)]
pub(super) struct ImmutableRayCachePolicy {
    pub(super) resistance: bool,
    pub(super) always_allows_block_explosion: bool,
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
    pub(super) fn try_new(bounds: BlockRegionBounds) -> Option<Self> {
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
    pub(super) fn cell_index(&self, pos: BlockPos) -> Option<usize> {
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

impl ServerExplosion<'_> {
    pub(super) fn calculate_immutable_ray_powers_with_reader<R: ExplosionBlockReader>(
        &self,
        powers: &[f32; super::RAY_COUNT],
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
        powers: &[f32; super::RAY_COUNT],
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
        powers: &[f32; super::RAY_COUNT],
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
}

pub(super) fn visit_immutable_ray_positions_cached<
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
fn ray_block_pos<const USE_BOUNDED_FLOOR: bool>(position: glam::DVec3) -> BlockPos {
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
pub(super) fn bounded_floor_to_i32(value: f64) -> i32 {
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
