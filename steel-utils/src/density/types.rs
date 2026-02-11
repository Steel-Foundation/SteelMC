//! Density function types matching vanilla Minecraft's DensityFunctions.java
//!
//! Each density function type is its own struct, mirroring vanilla's separate
//! record/class pattern. The [`DensityFunction`] enum wraps them for dispatch.
//!
//! # Evaluation
//!
//! Density functions implement the [`DensityFunctionOps`] trait, which provides
//! [`compute`](DensityFunctionOps::compute) and
//! [`compute_cached`](DensityFunctionOps::compute_cached) methods.
//! Noise generators and registry references are resolved ("baked") into the tree
//! at construction time, so evaluation requires only a [`DensityContext`].

use std::sync::Arc;

use enum_dispatch::enum_dispatch;
use rustc_hash::FxHashMap;

use crate::math::{clamp, map_clamped};
use crate::noise::NormalNoise;

// ── DensityFunctionOps trait ────────────────────────────────────────────────

/// Trait for evaluating density functions.
///
/// This is the core evaluation interface, matching vanilla's `DensityFunction.compute()`.
/// Noise generators and registry references must be baked into the tree before calling
/// these methods (see [`DensityFunction::resolve`]).
#[enum_dispatch]
pub trait DensityFunctionOps {
    /// Evaluate this density function at the given position.
    fn compute(&self, ctx: &DensityContext) -> f64;

    /// Evaluate this density function with caching support.
    ///
    /// Like [`compute`](Self::compute) but uses an [`EvalCache`] to avoid redundant
    /// evaluations. `CacheOnce` caches by last (x, y, z), `FlatCache`/`Cache2D` cache
    /// by (x, z).
    fn compute_cached(&self, ctx: &DensityContext, cache: &mut EvalCache) -> f64;
}

// ── Individual density function structs ──────────────────────────────────────

/// A constant density value.
///
/// Matches vanilla's `DensityFunctions.Constant`.
#[derive(Debug, Clone)]
pub struct Constant {
    /// The constant value.
    pub value: f64,
}

impl DensityFunctionOps for Constant {
    fn compute(&self, _ctx: &DensityContext) -> f64 {
        self.value
    }

    fn compute_cached(&self, _ctx: &DensityContext, _cache: &mut EvalCache) -> f64 {
        self.value
    }
}

/// A reference to another density function by ID.
///
/// After resolution via [`DensityFunction::resolve`], `resolved` holds the
/// target function and [`compute`](DensityFunctionOps::compute) delegates to it.
/// Matches vanilla's `DensityFunctions.HolderHolder`.
#[derive(Debug, Clone)]
pub struct Reference {
    /// The density function ID (for debugging / serialization).
    pub id: String,
    /// Resolved target (set by [`DensityFunction::resolve`]).
    pub resolved: Option<Arc<DensityFunction>>,
}

impl DensityFunctionOps for Reference {
    fn compute(&self, ctx: &DensityContext) -> f64 {
        if let Some(resolved) = &self.resolved {
            resolved.compute(ctx)
        } else {
            0.0
        }
    }

    fn compute_cached(&self, ctx: &DensityContext, cache: &mut EvalCache) -> f64 {
        if let Some(resolved) = &self.resolved {
            resolved.compute_cached(ctx, cache)
        } else {
            0.0
        }
    }
}

/// A Y-axis clamped gradient.
///
/// Returns `from_value` at Y = `from_y`, `to_value` at Y = `to_y`,
/// linearly interpolated between, clamped outside the range.
/// Matches vanilla's `DensityFunctions.YClampedGradient`.
#[derive(Debug, Clone)]
pub struct YClampedGradient {
    /// Starting Y coordinate
    pub from_y: i32,
    /// Ending Y coordinate
    pub to_y: i32,
    /// Value at `from_y`
    pub from_value: f64,
    /// Value at `to_y`
    pub to_value: f64,
}

impl DensityFunctionOps for YClampedGradient {
    fn compute(&self, ctx: &DensityContext) -> f64 {
        map_clamped(
            f64::from(ctx.y),
            f64::from(self.from_y),
            f64::from(self.to_y),
            self.from_value,
            self.to_value,
        )
    }

    fn compute_cached(&self, ctx: &DensityContext, _cache: &mut EvalCache) -> f64 {
        self.compute(ctx)
    }
}

/// Sample from a noise generator.
///
/// Matches vanilla's `DensityFunctions.Noise`.
#[derive(Debug, Clone)]
pub struct Noise {
    /// Noise identifier (for debugging / serialization)
    pub noise_id: String,
    /// XZ scale factor
    pub xz_scale: f64,
    /// Y scale factor
    pub y_scale: f64,
    /// Baked noise generator (set at construction time).
    pub noise: Option<NormalNoise>,
}

impl DensityFunctionOps for Noise {
    fn compute(&self, ctx: &DensityContext) -> f64 {
        if let Some(noise) = &self.noise {
            noise.get_value(
                f64::from(ctx.x) * self.xz_scale,
                f64::from(ctx.y) * self.y_scale,
                f64::from(ctx.z) * self.xz_scale,
            )
        } else {
            0.0
        }
    }

    fn compute_cached(&self, ctx: &DensityContext, _cache: &mut EvalCache) -> f64 {
        self.compute(ctx)
    }
}

/// Sample from a shifted noise generator.
///
/// Matches vanilla's `DensityFunctions.ShiftedNoise`.
#[derive(Debug, Clone)]
pub struct ShiftedNoise {
    /// X coordinate shift
    pub shift_x: Arc<DensityFunction>,
    /// Y coordinate shift
    pub shift_y: Arc<DensityFunction>,
    /// Z coordinate shift
    pub shift_z: Arc<DensityFunction>,
    /// XZ scale factor
    pub xz_scale: f64,
    /// Y scale factor
    pub y_scale: f64,
    /// Noise identifier (for debugging / serialization)
    pub noise_id: String,
    /// Baked noise generator.
    pub noise: Option<NormalNoise>,
}

impl DensityFunctionOps for ShiftedNoise {
    fn compute(&self, ctx: &DensityContext) -> f64 {
        let dx = self.shift_x.compute(ctx);
        let dy = self.shift_y.compute(ctx);
        let dz = self.shift_z.compute(ctx);
        if let Some(noise) = &self.noise {
            noise.get_value(
                f64::from(ctx.x) * self.xz_scale + dx,
                f64::from(ctx.y) * self.y_scale + dy,
                f64::from(ctx.z) * self.xz_scale + dz,
            )
        } else {
            0.0
        }
    }

    fn compute_cached(&self, ctx: &DensityContext, cache: &mut EvalCache) -> f64 {
        let dx = self.shift_x.compute_cached(ctx, cache);
        let dy = self.shift_y.compute_cached(ctx, cache);
        let dz = self.shift_z.compute_cached(ctx, cache);
        if let Some(noise) = &self.noise {
            noise.get_value(
                f64::from(ctx.x) * self.xz_scale + dx,
                f64::from(ctx.y) * self.y_scale + dy,
                f64::from(ctx.z) * self.xz_scale + dz,
            )
        } else {
            0.0
        }
    }
}

/// Shift noise generator A for coordinate offsetting.
///
/// Matches vanilla's `DensityFunctions.ShiftA`.
#[derive(Debug, Clone)]
pub struct ShiftA {
    /// Noise identifier (for debugging / serialization)
    pub noise_id: String,
    /// Baked noise generator.
    pub noise: Option<NormalNoise>,
}

impl DensityFunctionOps for ShiftA {
    fn compute(&self, ctx: &DensityContext) -> f64 {
        if let Some(noise) = &self.noise {
            noise.get_value(f64::from(ctx.x) * 0.25, 0.0, f64::from(ctx.z) * 0.25) * 4.0
        } else {
            0.0
        }
    }

    fn compute_cached(&self, ctx: &DensityContext, _cache: &mut EvalCache) -> f64 {
        self.compute(ctx)
    }
}

/// Shift noise generator B for coordinate offsetting.
///
/// Matches vanilla's `DensityFunctions.ShiftB`.
#[derive(Debug, Clone)]
pub struct ShiftB {
    /// Noise identifier (for debugging / serialization)
    pub noise_id: String,
    /// Baked noise generator.
    pub noise: Option<NormalNoise>,
}

impl DensityFunctionOps for ShiftB {
    fn compute(&self, ctx: &DensityContext) -> f64 {
        if let Some(noise) = &self.noise {
            noise.get_value(f64::from(ctx.z) * 0.25, f64::from(ctx.x) * 0.25, 0.0) * 4.0
        } else {
            0.0
        }
    }

    fn compute_cached(&self, ctx: &DensityContext, _cache: &mut EvalCache) -> f64 {
        self.compute(ctx)
    }
}

/// Generic shift noise generator for coordinate offsetting.
///
/// Matches vanilla's `DensityFunctions.Shift`.
#[derive(Debug, Clone)]
pub struct Shift {
    /// Noise identifier (for debugging / serialization)
    pub noise_id: String,
    /// Baked noise generator.
    pub noise: Option<NormalNoise>,
}

impl DensityFunctionOps for Shift {
    fn compute(&self, ctx: &DensityContext) -> f64 {
        if let Some(noise) = &self.noise {
            noise.get_value(
                f64::from(ctx.x) * 0.25,
                f64::from(ctx.y) * 0.25,
                f64::from(ctx.z) * 0.25,
            ) * 4.0
        } else {
            0.0
        }
    }

    fn compute_cached(&self, ctx: &DensityContext, _cache: &mut EvalCache) -> f64 {
        self.compute(ctx)
    }
}

/// The type of two-argument operation.
///
/// Matches vanilla's `DensityFunctions.TwoArgumentSimpleFunction.Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwoArgType {
    /// Add two density functions.
    Add,
    /// Multiply two density functions.
    Mul,
    /// Take the minimum of two density functions.
    Min,
    /// Take the maximum of two density functions.
    Max,
}

/// A two-argument density function (add, mul, min, max).
///
/// Matches vanilla's `DensityFunctions.Ap2` / `TwoArgumentSimpleFunction`.
#[derive(Debug, Clone)]
pub struct TwoArgumentSimple {
    /// The operation type
    pub op: TwoArgType,
    /// First argument
    pub argument1: Arc<DensityFunction>,
    /// Second argument
    pub argument2: Arc<DensityFunction>,
}

impl DensityFunctionOps for TwoArgumentSimple {
    fn compute(&self, ctx: &DensityContext) -> f64 {
        let a = self.argument1.compute(ctx);
        let b = self.argument2.compute(ctx);
        match self.op {
            TwoArgType::Add => a + b,
            TwoArgType::Mul => a * b,
            TwoArgType::Min => a.min(b),
            TwoArgType::Max => a.max(b),
        }
    }

    fn compute_cached(&self, ctx: &DensityContext, cache: &mut EvalCache) -> f64 {
        let a = self.argument1.compute_cached(ctx, cache);
        let b = self.argument2.compute_cached(ctx, cache);
        match self.op {
            TwoArgType::Add => a + b,
            TwoArgType::Mul => a * b,
            TwoArgType::Min => a.min(b),
            TwoArgType::Max => a.max(b),
        }
    }
}

/// The type of mapped (pure transformer) operation.
///
/// Matches vanilla's `DensityFunctions.Mapped.Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappedType {
    /// Absolute value.
    Abs,
    /// Square the value.
    Square,
    /// Cube the value.
    Cube,
    /// Half negative: if v > 0 then v else v * 0.5
    HalfNegative,
    /// Quarter negative: if v > 0 then v else v * 0.25
    QuarterNegative,
    /// Squeeze: clamp(-1, 1) then apply c/2 - c^3/24
    Squeeze,
}

/// A mapped (pure transformer) density function.
///
/// Applies a unary operation to its input.
/// Matches vanilla's `DensityFunctions.Mapped`.
#[derive(Debug, Clone)]
pub struct Mapped {
    /// The mapping type
    pub op: MappedType,
    /// Input density function
    pub input: Arc<DensityFunction>,
}

impl DensityFunctionOps for Mapped {
    fn compute(&self, ctx: &DensityContext) -> f64 {
        let v = self.input.compute(ctx);
        apply_mapped(self.op, v)
    }

    fn compute_cached(&self, ctx: &DensityContext, cache: &mut EvalCache) -> f64 {
        let v = self.input.compute_cached(ctx, cache);
        apply_mapped(self.op, v)
    }
}

/// Clamp a density function value to a range.
///
/// Matches vanilla's `DensityFunctions.Clamp`.
#[derive(Debug, Clone)]
pub struct Clamp {
    /// Input density function
    pub input: Arc<DensityFunction>,
    /// Minimum value
    pub min: f64,
    /// Maximum value
    pub max: f64,
}

impl DensityFunctionOps for Clamp {
    fn compute(&self, ctx: &DensityContext) -> f64 {
        clamp(self.input.compute(ctx), self.min, self.max)
    }

    fn compute_cached(&self, ctx: &DensityContext, cache: &mut EvalCache) -> f64 {
        clamp(self.input.compute_cached(ctx, cache), self.min, self.max)
    }
}

/// Choose between two functions based on input range.
///
/// Matches vanilla's `DensityFunctions.RangeChoice`.
#[derive(Debug, Clone)]
pub struct RangeChoice {
    /// Input density function
    pub input: Arc<DensityFunction>,
    /// Minimum inclusive bound
    pub min_inclusive: f64,
    /// Maximum exclusive bound
    pub max_exclusive: f64,
    /// Function to use when in range
    pub when_in_range: Arc<DensityFunction>,
    /// Function to use when out of range
    pub when_out_of_range: Arc<DensityFunction>,
}

impl DensityFunctionOps for RangeChoice {
    fn compute(&self, ctx: &DensityContext) -> f64 {
        let v = self.input.compute(ctx);
        if v >= self.min_inclusive && v < self.max_exclusive {
            self.when_in_range.compute(ctx)
        } else {
            self.when_out_of_range.compute(ctx)
        }
    }

    fn compute_cached(&self, ctx: &DensityContext, cache: &mut EvalCache) -> f64 {
        let v = self.input.compute_cached(ctx, cache);
        if v >= self.min_inclusive && v < self.max_exclusive {
            self.when_in_range.compute_cached(ctx, cache)
        } else {
            self.when_out_of_range.compute_cached(ctx, cache)
        }
    }
}

/// Blended (interpolated) 3D noise.
///
/// Matches vanilla's `BlendedNoise`.
#[derive(Debug, Clone)]
pub struct BlendedNoise {
    /// XZ scale factor
    pub xz_scale: f64,
    /// Y scale factor
    pub y_scale: f64,
    /// XZ interpolation factor
    pub xz_factor: f64,
    /// Y interpolation factor
    pub y_factor: f64,
    /// Smear scale multiplier
    pub smear_scale_multiplier: f64,
    /// Baked noise generator (uses the "offset" noise as approximation).
    pub noise: Option<NormalNoise>,
}

impl DensityFunctionOps for BlendedNoise {
    fn compute(&self, ctx: &DensityContext) -> f64 {
        if let Some(noise) = &self.noise {
            let scaled_xz = self.xz_scale / self.xz_factor;
            let scaled_y = self.y_scale / self.y_factor;
            noise.get_value(
                f64::from(ctx.x) * scaled_xz,
                f64::from(ctx.y) * scaled_y,
                f64::from(ctx.z) * scaled_xz,
            ) * self.smear_scale_multiplier
        } else {
            0.0
        }
    }

    fn compute_cached(&self, ctx: &DensityContext, _cache: &mut EvalCache) -> f64 {
        self.compute(ctx)
    }
}

/// Weird scaled sampler (for cave generation).
///
/// Matches vanilla's `DensityFunctions.WeirdScaledSampler`.
#[derive(Debug, Clone)]
pub struct WeirdScaledSampler {
    /// Input density function
    pub input: Arc<DensityFunction>,
    /// Noise identifier (for debugging / serialization)
    pub noise_id: String,
    /// Rarity value mapper
    pub rarity_value_mapper: RarityValueMapper,
    /// Baked noise generator.
    pub noise: Option<NormalNoise>,
}

impl DensityFunctionOps for WeirdScaledSampler {
    fn compute(&self, ctx: &DensityContext) -> f64 {
        let rarity = self.input.compute(ctx);
        let scale = self.rarity_value_mapper.get_values(rarity);
        if let Some(noise) = &self.noise {
            scale
                * noise
                    .get_value(
                        f64::from(ctx.x) / scale,
                        f64::from(ctx.y) / scale,
                        f64::from(ctx.z) / scale,
                    )
                    .abs()
        } else {
            0.0
        }
    }

    fn compute_cached(&self, ctx: &DensityContext, cache: &mut EvalCache) -> f64 {
        let rarity = self.input.compute_cached(ctx, cache);
        let scale = self.rarity_value_mapper.get_values(rarity);
        if let Some(noise) = &self.noise {
            scale
                * noise
                    .get_value(
                        f64::from(ctx.x) / scale,
                        f64::from(ctx.y) / scale,
                        f64::from(ctx.z) / scale,
                    )
                    .abs()
        } else {
            0.0
        }
    }
}

/// Blend density (for chunk blending).
///
/// Matches vanilla's `DensityFunctions.BlendDensity`.
#[derive(Debug, Clone)]
pub struct BlendDensity {
    /// Input density function
    pub input: Arc<DensityFunction>,
}

impl DensityFunctionOps for BlendDensity {
    // TODO: Implement Blender integration for chunk-boundary blending.
    // Pass-through is correct when no Blender is active.
    fn compute(&self, ctx: &DensityContext) -> f64 {
        self.input.compute(ctx)
    }

    fn compute_cached(&self, ctx: &DensityContext, cache: &mut EvalCache) -> f64 {
        self.input.compute_cached(ctx, cache)
    }
}

/// The type of cache/marker wrapper.
///
/// Matches vanilla's `DensityFunctions.Marker.Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerType {
    /// Interpolated (cell-based interpolation).
    Interpolated,
    /// Cache for 2D (XZ) positions.
    FlatCache,
    /// Cache for 2D (XZ) positions.
    Cache2D,
    /// Cache the result for one evaluation.
    CacheOnce,
    /// Cache all values in a cell.
    CacheAllInCell,
}

/// A cache/marker wrapper density function.
///
/// These are optimization hints that wrap another density function.
/// Matches vanilla's `DensityFunctions.Marker`.
#[derive(Debug, Clone)]
pub struct Marker {
    /// The marker type
    pub kind: MarkerType,
    /// The wrapped density function
    pub wrapped: Arc<DensityFunction>,
}

impl DensityFunctionOps for Marker {
    // Cache markers are transparent in single-point evaluation.
    fn compute(&self, ctx: &DensityContext) -> f64 {
        self.wrapped.compute(ctx)
    }

    fn compute_cached(&self, ctx: &DensityContext, cache: &mut EvalCache) -> f64 {
        match self.kind {
            // CacheOnce: cache the last (x, y, z) → value per node.
            MarkerType::CacheOnce => {
                let key = Arc::as_ptr(&self.wrapped) as usize;
                if let Some(&(cx, cy, cz, val)) = cache.once.get(&key)
                    && cx == ctx.x
                    && cy == ctx.y
                    && cz == ctx.z
                {
                    return val;
                }
                let val = self.wrapped.compute_cached(ctx, cache);
                cache.once.insert(key, (ctx.x, ctx.y, ctx.z, val));
                val
            }

            // FlatCache/Cache2D: cache by (x, z) per node, ignoring y.
            MarkerType::FlatCache | MarkerType::Cache2D => {
                let key = Arc::as_ptr(&self.wrapped) as usize;
                let flat_key = (key, ctx.x, ctx.z);
                if let Some(&val) = cache.flat.get(&flat_key) {
                    return val;
                }
                let val = self.wrapped.compute_cached(ctx, cache);
                cache.flat.insert(flat_key, val);
                val
            }

            // CacheAllInCell/Interpolated: pass-through for now (used by terrain, not biomes).
            MarkerType::CacheAllInCell | MarkerType::Interpolated => {
                self.wrapped.compute_cached(ctx, cache)
            }
        }
    }
}

/// Cubic spline density function wrapper.
///
/// Wraps an `Arc<CubicSpline>` for spline-based density evaluation.
/// Matches vanilla's `DensityFunctions.Spline`.
#[derive(Debug, Clone)]
pub struct Spline {
    /// The cubic spline.
    pub spline: Arc<CubicSpline>,
}

impl DensityFunctionOps for Spline {
    fn compute(&self, ctx: &DensityContext) -> f64 {
        compute_spline(&self.spline, ctx)
    }

    fn compute_cached(&self, ctx: &DensityContext, cache: &mut EvalCache) -> f64 {
        compute_spline_cached(&self.spline, ctx, cache)
    }
}

/// End islands density function.
///
/// Matches vanilla's `DensityFunctions.EndIslands`.
/// TODO: Implement `SimplexNoise` + island generation algorithm.
#[derive(Debug, Clone, Copy)]
pub struct EndIslands;

impl DensityFunctionOps for EndIslands {
    // TODO: Implement `SimplexNoise` + island generation algorithm.
    fn compute(&self, _ctx: &DensityContext) -> f64 {
        0.0
    }

    fn compute_cached(&self, _ctx: &DensityContext, _cache: &mut EvalCache) -> f64 {
        0.0
    }
}

/// Blend alpha density function (returns 1.0, placeholder for blending).
///
/// Matches vanilla's `DensityFunctions.BlendAlpha`.
#[derive(Debug, Clone, Copy)]
pub struct BlendAlpha;

impl DensityFunctionOps for BlendAlpha {
    // BlendAlpha returns 1.0 (no blending). Vanilla also returns 1.0
    // unless an active Blender overrides it in NoiseChunk.
    fn compute(&self, _ctx: &DensityContext) -> f64 {
        1.0
    }

    fn compute_cached(&self, _ctx: &DensityContext, _cache: &mut EvalCache) -> f64 {
        1.0
    }
}

/// Blend offset density function (returns 0.0, placeholder for blending).
///
/// Matches vanilla's `DensityFunctions.BlendOffset`.
#[derive(Debug, Clone, Copy)]
pub struct BlendOffset;

impl DensityFunctionOps for BlendOffset {
    // BlendOffset returns 0.0 (no offset). Vanilla also returns 0.0
    // unless an active Blender overrides it in NoiseChunk.
    fn compute(&self, _ctx: &DensityContext) -> f64 {
        0.0
    }

    fn compute_cached(&self, _ctx: &DensityContext, _cache: &mut EvalCache) -> f64 {
        0.0
    }
}

// ── DensityFunction enum (dispatch wrapper) ─────────────────────────────────

/// A density function that can be evaluated at a position to get a density value.
///
/// Density functions form a tree structure where complex functions are composed
/// from simpler ones. Each variant wraps a separate struct matching vanilla's
/// per-type class/record pattern. This matches vanilla's `DensityFunction` interface.
///
/// Evaluation is done via the [`DensityFunctionOps`] trait, which provides
/// [`compute`](DensityFunctionOps::compute) and
/// [`compute_cached`](DensityFunctionOps::compute_cached).
#[derive(Debug, Clone)]
#[enum_dispatch(DensityFunctionOps)]
pub enum DensityFunction {
    /// A constant value.
    Constant(Constant),

    /// A reference to another density function by ID.
    Reference(Reference),

    /// A Y-axis clamped gradient.
    YClampedGradient(YClampedGradient),

    /// Sample from a noise generator.
    Noise(Noise),

    /// Sample from a shifted noise generator.
    ShiftedNoise(ShiftedNoise),

    /// Shift noise generator A for coordinate offsetting.
    ShiftA(ShiftA),

    /// Shift noise generator B for coordinate offsetting.
    ShiftB(ShiftB),

    /// Generic shift noise generator for coordinate offsetting.
    Shift(Shift),

    /// Two-argument operation (add, mul, min, max).
    TwoArgumentSimple(TwoArgumentSimple),

    /// Mapped (pure transformer) operation (abs, square, cube, etc.).
    Mapped(Mapped),

    /// Clamp the value to a range.
    Clamp(Clamp),

    /// Choose between two functions based on input range.
    RangeChoice(RangeChoice),

    /// Cubic spline evaluation.
    Spline(Spline),

    /// Blended (interpolated) 3D noise.
    BlendedNoise(BlendedNoise),

    /// Weird scaled sampler (for cave generation).
    WeirdScaledSampler(WeirdScaledSampler),

    /// End islands density function.
    EndIslands(EndIslands),

    /// Blend alpha (returns 1.0, placeholder for blending).
    BlendAlpha(BlendAlpha),

    /// Blend offset (returns 0.0, placeholder for blending).
    BlendOffset(BlendOffset),

    /// Blend density (for chunk blending).
    BlendDensity(BlendDensity),

    /// Cache/marker wrapper (optimization hints).
    Marker(Marker),
}

// ── Convenience constructors ────────────────────────────────────────────────

impl DensityFunction {
    /// Create a constant density function.
    #[must_use]
    pub const fn constant(value: f64) -> Self {
        Self::Constant(Constant { value })
    }

    /// Create a reference density function (unresolved).
    #[must_use]
    pub const fn reference(id: String) -> Self {
        Self::Reference(Reference { id, resolved: None })
    }

    /// Resolve all `Reference` nodes in this tree using the given registry,
    /// and bake noise generators from `noises`.
    ///
    /// Call this once after building the full density function tree.
    #[must_use]
    pub fn resolve(
        &self,
        registry: &FxHashMap<String, Arc<DensityFunction>>,
        noises: &FxHashMap<String, NormalNoise>,
    ) -> Self {
        self.resolve_inner(registry, noises)
    }

    fn resolve_inner(
        &self,
        registry: &FxHashMap<String, Arc<DensityFunction>>,
        noises: &FxHashMap<String, NormalNoise>,
    ) -> Self {
        match self {
            Self::Constant(_)
            | Self::EndIslands(_)
            | Self::BlendAlpha(_)
            | Self::BlendOffset(_)
            | Self::YClampedGradient(_) => self.clone(),

            Self::Reference(r) => {
                let resolved = registry
                    .get(&r.id)
                    .map(|df| Arc::new(df.resolve_inner(registry, noises)));
                Self::Reference(Reference {
                    id: r.id.clone(),
                    resolved,
                })
            }

            Self::Noise(n) => Self::Noise(Noise {
                noise_id: n.noise_id.clone(),
                xz_scale: n.xz_scale,
                y_scale: n.y_scale,
                noise: noises.get(&n.noise_id).cloned(),
            }),

            Self::ShiftedNoise(sn) => Self::ShiftedNoise(ShiftedNoise {
                shift_x: Arc::new(sn.shift_x.resolve_inner(registry, noises)),
                shift_y: Arc::new(sn.shift_y.resolve_inner(registry, noises)),
                shift_z: Arc::new(sn.shift_z.resolve_inner(registry, noises)),
                xz_scale: sn.xz_scale,
                y_scale: sn.y_scale,
                noise_id: sn.noise_id.clone(),
                noise: noises.get(&sn.noise_id).cloned(),
            }),

            Self::ShiftA(s) => Self::ShiftA(ShiftA {
                noise_id: s.noise_id.clone(),
                noise: noises.get(&s.noise_id).cloned(),
            }),

            Self::ShiftB(s) => Self::ShiftB(ShiftB {
                noise_id: s.noise_id.clone(),
                noise: noises.get(&s.noise_id).cloned(),
            }),

            Self::Shift(s) => Self::Shift(Shift {
                noise_id: s.noise_id.clone(),
                noise: noises.get(&s.noise_id).cloned(),
            }),

            Self::TwoArgumentSimple(t) => Self::TwoArgumentSimple(TwoArgumentSimple {
                op: t.op,
                argument1: Arc::new(t.argument1.resolve_inner(registry, noises)),
                argument2: Arc::new(t.argument2.resolve_inner(registry, noises)),
            }),

            Self::Mapped(m) => Self::Mapped(Mapped {
                op: m.op,
                input: Arc::new(m.input.resolve_inner(registry, noises)),
            }),

            Self::Clamp(c) => Self::Clamp(Clamp {
                input: Arc::new(c.input.resolve_inner(registry, noises)),
                min: c.min,
                max: c.max,
            }),

            Self::RangeChoice(rc) => Self::RangeChoice(RangeChoice {
                input: Arc::new(rc.input.resolve_inner(registry, noises)),
                min_inclusive: rc.min_inclusive,
                max_exclusive: rc.max_exclusive,
                when_in_range: Arc::new(rc.when_in_range.resolve_inner(registry, noises)),
                when_out_of_range: Arc::new(rc.when_out_of_range.resolve_inner(registry, noises)),
            }),

            Self::Spline(s) => Self::Spline(Spline {
                spline: Arc::new(resolve_spline(&s.spline, registry, noises)),
            }),

            Self::BlendedNoise(bn) => Self::BlendedNoise(BlendedNoise {
                xz_scale: bn.xz_scale,
                y_scale: bn.y_scale,
                xz_factor: bn.xz_factor,
                y_factor: bn.y_factor,
                smear_scale_multiplier: bn.smear_scale_multiplier,
                noise: noises.get("offset").cloned(),
            }),

            Self::WeirdScaledSampler(ws) => Self::WeirdScaledSampler(WeirdScaledSampler {
                input: Arc::new(ws.input.resolve_inner(registry, noises)),
                noise_id: ws.noise_id.clone(),
                rarity_value_mapper: ws.rarity_value_mapper,
                noise: noises.get(&ws.noise_id).cloned(),
            }),

            Self::BlendDensity(bd) => Self::BlendDensity(BlendDensity {
                input: Arc::new(bd.input.resolve_inner(registry, noises)),
            }),

            Self::Marker(m) => Self::Marker(Marker {
                kind: m.kind,
                wrapped: Arc::new(m.wrapped.resolve_inner(registry, noises)),
            }),
        }
    }
}

/// Resolve noise/registry references within a cubic spline.
fn resolve_spline(
    spline: &CubicSpline,
    registry: &FxHashMap<String, Arc<DensityFunction>>,
    noises: &FxHashMap<String, NormalNoise>,
) -> CubicSpline {
    CubicSpline {
        coordinate: Arc::new(spline.coordinate.resolve_inner(registry, noises)),
        points: spline
            .points
            .iter()
            .map(|p| SplinePoint {
                location: p.location,
                value: match &p.value {
                    SplineValue::Constant(v) => SplineValue::Constant(*v),
                    SplineValue::Spline(nested) => {
                        SplineValue::Spline(Arc::new(resolve_spline(nested, registry, noises)))
                    }
                },
                derivative: p.derivative,
            })
            .collect(),
    }
}

// ── Cache for density function evaluation ───────────────────────────────────

/// Cache for density function evaluation, matching vanilla's per-node caching.
///
/// Create one per chunk or per evaluation batch and pass to
/// [`DensityFunctionOps::compute_cached`].
/// Cache keys are derived from stable `Arc` pointer addresses in the density function tree.
pub struct EvalCache {
    /// `CacheOnce`: caches last (x, y, z) → value per node.
    once: FxHashMap<usize, (i32, i32, i32, f64)>,
    /// `FlatCache`/`Cache2D`: caches (x, z) → value per node, keyed by (`node_key`, x, z).
    flat: FxHashMap<(usize, i32, i32), f64>,
}

impl EvalCache {
    /// Create a new empty evaluation cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            once: FxHashMap::default(),
            flat: FxHashMap::default(),
        }
    }
}

impl Default for EvalCache {
    fn default() -> Self {
        Self::new()
    }
}

// ── Noise parameters ────────────────────────────────────────────────────────

/// Parameters for creating a noise generator.
#[derive(Debug, Clone)]
pub struct NoiseParameters {
    /// The first octave level.
    pub first_octave: i32,
    /// Amplitude multipliers for each octave.
    pub amplitudes: Vec<f64>,
}

impl NoiseParameters {
    /// Create new noise parameters.
    #[must_use]
    pub const fn new(first_octave: i32, amplitudes: Vec<f64>) -> Self {
        Self {
            first_octave,
            amplitudes,
        }
    }
}

// ── Shared evaluation helpers ───────────────────────────────────────────────

/// Apply a mapped (unary) operation.
fn apply_mapped(op: MappedType, v: f64) -> f64 {
    match op {
        MappedType::Abs => v.abs(),
        MappedType::Square => v * v,
        MappedType::Cube => v * v * v,
        MappedType::HalfNegative => {
            if v > 0.0 {
                v
            } else {
                v * 0.5
            }
        }
        MappedType::QuarterNegative => {
            if v > 0.0 {
                v
            } else {
                v * 0.25
            }
        }
        MappedType::Squeeze => {
            let c = clamp(v, -1.0, 1.0);
            c / 2.0 - c * c * c / 24.0
        }
    }
}

// ── Spline evaluation (non-cached) ─────────────────────────────────────────

/// Evaluate a cubic spline.
fn compute_spline(spline: &CubicSpline, ctx: &DensityContext) -> f64 {
    let input = spline.coordinate.compute(ctx) as f32;
    f64::from(compute_spline_at(spline, input, ctx))
}

/// Evaluate a cubic spline at a given input value.
///
/// Matches vanilla's `CubicSpline.Multipoint.apply()`:
/// - If input is before the first point, linear extrapolation using first derivative
/// - If input is after the last point, linear extrapolation using last derivative
/// - Otherwise, Hermite cubic interpolation between adjacent points
fn compute_spline_at(spline: &CubicSpline, input: f32, ctx: &DensityContext) -> f32 {
    if spline.points.is_empty() {
        return 0.0;
    }

    let last_index = spline.points.len() - 1;

    let start = find_interval_start(&spline.points, input);

    if start < 0 {
        let p = &spline.points[0];
        let value = get_spline_point_value(p, ctx);
        return value + p.derivative * (input - p.location);
    }

    let start = start as usize;
    if start == last_index {
        let p = &spline.points[last_index];
        let value = get_spline_point_value(p, ctx);
        return value + p.derivative * (input - p.location);
    }

    hermite_interpolate(spline, start, input, ctx)
}

/// Get the value at a spline point.
fn get_spline_point_value(point: &SplinePoint, ctx: &DensityContext) -> f32 {
    match &point.value {
        SplineValue::Constant(v) => *v,
        SplineValue::Spline(nested) => {
            let nested_input = nested.coordinate.compute(ctx) as f32;
            compute_spline_at(nested, nested_input, ctx)
        }
    }
}

// ── Spline evaluation (cached) ─────────────────────────────────────────────

/// Evaluate a cubic spline with caching.
fn compute_spline_cached(spline: &CubicSpline, ctx: &DensityContext, cache: &mut EvalCache) -> f64 {
    let input = spline.coordinate.compute_cached(ctx, cache) as f32;
    f64::from(compute_spline_at_cached(spline, input, ctx, cache))
}

/// Evaluate a cubic spline at a given input value with caching.
fn compute_spline_at_cached(
    spline: &CubicSpline,
    input: f32,
    ctx: &DensityContext,
    cache: &mut EvalCache,
) -> f32 {
    if spline.points.is_empty() {
        return 0.0;
    }

    let last_index = spline.points.len() - 1;

    let start = find_interval_start(&spline.points, input);

    if start < 0 {
        let p = &spline.points[0];
        let value = get_spline_point_value_cached(p, ctx, cache);
        return value + p.derivative * (input - p.location);
    }

    let start = start as usize;
    if start == last_index {
        let p = &spline.points[last_index];
        let value = get_spline_point_value_cached(p, ctx, cache);
        return value + p.derivative * (input - p.location);
    }

    hermite_interpolate_cached(spline, start, input, ctx, cache)
}

/// Get the value at a spline point with caching.
fn get_spline_point_value_cached(
    point: &SplinePoint,
    ctx: &DensityContext,
    cache: &mut EvalCache,
) -> f32 {
    match &point.value {
        SplineValue::Constant(v) => *v,
        SplineValue::Spline(nested) => {
            let nested_input = nested.coordinate.compute_cached(ctx, cache) as f32;
            compute_spline_at_cached(nested, nested_input, ctx, cache)
        }
    }
}

// ── Shared spline helpers ───────────────────────────────────────────────────

/// Binary search to find the interval start: largest i where points[i].location <= input.
///
/// Returns -1 if input is before all points.
fn find_interval_start(points: &[SplinePoint], input: f32) -> i32 {
    let mut lo = 0i32;
    let mut hi = points.len() as i32;
    while lo < hi {
        let mid = i32::midpoint(lo, hi);
        if input < points[mid as usize].location {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo - 1
}

/// Hermite cubic interpolation between adjacent spline points (non-cached).
fn hermite_interpolate(
    spline: &CubicSpline,
    start: usize,
    input: f32,
    ctx: &DensityContext,
) -> f32 {
    let p0 = &spline.points[start];
    let p1 = &spline.points[start + 1];
    let x1 = p0.location;
    let x2 = p1.location;
    let t = (input - x1) / (x2 - x1);
    let y1 = get_spline_point_value(p0, ctx);
    let y2 = get_spline_point_value(p1, ctx);
    let d1 = p0.derivative;
    let d2 = p1.derivative;

    // Vanilla formula: lerp(t, y1, y2) + t * (1 - t) * lerp(t, a, b)
    let h = x2 - x1;
    let a = d1 * h - (y2 - y1);
    let b = -d2 * h + (y2 - y1);
    let lerp_y = y1 + t * (y2 - y1);
    let lerp_ab = a + t * (b - a);
    lerp_y + t * (1.0 - t) * lerp_ab
}

/// Hermite cubic interpolation between adjacent spline points (cached).
fn hermite_interpolate_cached(
    spline: &CubicSpline,
    start: usize,
    input: f32,
    ctx: &DensityContext,
    cache: &mut EvalCache,
) -> f32 {
    let p0 = &spline.points[start];
    let p1 = &spline.points[start + 1];
    let x1 = p0.location;
    let x2 = p1.location;
    let t = (input - x1) / (x2 - x1);
    let y1 = get_spline_point_value_cached(p0, ctx, cache);
    let y2 = get_spline_point_value_cached(p1, ctx, cache);
    let d1 = p0.derivative;
    let d2 = p1.derivative;

    let h = x2 - x1;
    let a = d1 * h - (y2 - y1);
    let b = -d2 * h + (y2 - y1);
    let lerp_y = y1 + t * (y2 - y1);
    let lerp_ab = a + t * (b - a);
    lerp_y + t * (1.0 - t) * lerp_ab
}

// ── Supporting types ────────────────────────────────────────────────────────

/// Rarity value mapper for cave generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RarityValueMapper {
    /// Mapper type `"type_1"` for tunnels.
    Tunnels,
    /// Mapper type `"type_2"` for caves.
    Caves,
}

impl RarityValueMapper {
    /// Get the scaling factor for this mapper based on rarity value.
    ///
    /// From vanilla NoiseRouterData.QuantizedSpaghettiRarity.
    #[must_use]
    pub fn get_values(&self, rarity: f64) -> f64 {
        match self {
            // Type 1: getSpaghettiRarity3D (tunnels)
            Self::Tunnels => {
                if rarity < -0.5 {
                    0.75
                } else if rarity < 0.0 {
                    1.0
                } else if rarity < 0.5 {
                    1.5
                } else {
                    2.0
                }
            }
            // Type 2: getSpaghettiRarity2D (caves)
            Self::Caves => {
                if rarity < -0.75 {
                    0.5
                } else if rarity < -0.5 {
                    0.75
                } else if rarity < 0.5 {
                    1.0
                } else if rarity < 0.75 {
                    2.0
                } else {
                    3.0
                }
            }
        }
    }
}

/// A cubic spline for density function interpolation.
#[derive(Debug, Clone)]
pub struct CubicSpline {
    /// The coordinate extractor (which density function to use as input)
    pub coordinate: Arc<DensityFunction>,
    /// The spline points
    pub points: Vec<SplinePoint>,
}

/// A point in a cubic spline.
#[derive(Debug, Clone)]
pub struct SplinePoint {
    /// The location (input value) of this point.
    pub location: f32,
    /// The value or nested spline at this point.
    pub value: SplineValue,
    /// The derivative at this point.
    pub derivative: f32,
}

/// A spline point value can be either a constant or a nested spline.
#[derive(Debug, Clone)]
pub enum SplineValue {
    /// A constant value.
    Constant(f32),
    /// A nested spline.
    Spline(Arc<CubicSpline>),
}

impl CubicSpline {
    /// Create a new cubic spline.
    #[must_use]
    pub const fn new(coordinate: Arc<DensityFunction>, points: Vec<SplinePoint>) -> Self {
        Self { coordinate, points }
    }
}

/// Context for evaluating density functions at a position.
#[derive(Debug, Clone, Copy)]
pub struct DensityContext {
    /// X coordinate (block position)
    pub x: i32,
    /// Y coordinate (block position)
    pub y: i32,
    /// Z coordinate (block position)
    pub z: i32,
}

impl DensityContext {
    /// Create a new density context.
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

/// A noise router containing all the density functions for world generation.
#[derive(Debug, Clone)]
pub struct NoiseRouter {
    /// Barrier noise for aquifers
    pub barrier_noise: Arc<DensityFunction>,
    /// Fluid level floodedness
    pub fluid_level_floodedness: Arc<DensityFunction>,
    /// Fluid level spread
    pub fluid_level_spread: Arc<DensityFunction>,
    /// Lava noise
    pub lava: Arc<DensityFunction>,
    /// Temperature (for biome selection)
    pub temperature: Arc<DensityFunction>,
    /// Vegetation/humidity (for biome selection)
    pub vegetation: Arc<DensityFunction>,
    /// Continentalness (for biome selection)
    pub continentalness: Arc<DensityFunction>,
    /// Erosion (for biome selection)
    pub erosion: Arc<DensityFunction>,
    /// Depth (for biome selection)
    pub depth: Arc<DensityFunction>,
    /// Ridges/weirdness (for biome selection)
    pub ridges: Arc<DensityFunction>,
    /// Preliminary surface level (for aquifers and surface rules)
    pub preliminary_surface_level: Arc<DensityFunction>,
    /// Final density (for terrain generation)
    pub final_density: Arc<DensityFunction>,
    /// Vein toggle
    pub vein_toggle: Arc<DensityFunction>,
    /// Vein ridged
    pub vein_ridged: Arc<DensityFunction>,
    /// Vein gap
    pub vein_gap: Arc<DensityFunction>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::Random;
    use crate::random::xoroshiro::Xoroshiro;

    fn make_test_noises() -> FxHashMap<String, NormalNoise> {
        let mut rng = Xoroshiro::from_seed(12345);
        let splitter = rng.next_positional();

        let mut noises = FxHashMap::default();
        let noise = NormalNoise::create(&splitter, "test_noise", -4, &[1.0, 1.0, 1.0, 1.0]);
        noises.insert("test_noise".to_string(), noise);
        noises
    }

    #[test]
    fn test_constant() {
        let func = DensityFunction::constant(42.0);
        let ctx = DensityContext::new(0, 64, 0);
        assert!((func.compute(&ctx) - 42.0).abs() < 1e-10);
    }

    #[test]
    fn test_rarity_value_mapper_tunnels() {
        let mapper = RarityValueMapper::Tunnels;
        assert!((mapper.get_values(-0.6) - 0.75).abs() < 0.01);
        assert!((mapper.get_values(-0.3) - 1.0).abs() < 0.01);
        assert!((mapper.get_values(0.0) - 1.5).abs() < 0.01);
        assert!((mapper.get_values(0.3) - 1.5).abs() < 0.01);
        assert!((mapper.get_values(0.6) - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_rarity_value_mapper_caves() {
        let mapper = RarityValueMapper::Caves;
        assert!((mapper.get_values(-0.8) - 0.5).abs() < 0.01);
        assert!((mapper.get_values(-0.6) - 0.75).abs() < 0.01);
        assert!((mapper.get_values(0.0) - 1.0).abs() < 0.01);
        assert!((mapper.get_values(0.6) - 2.0).abs() < 0.01);
        assert!((mapper.get_values(0.8) - 3.0).abs() < 0.01);
    }

    #[test]
    fn test_y_clamped_gradient() {
        let func = DensityFunction::YClampedGradient(YClampedGradient {
            from_y: -64,
            to_y: 320,
            from_value: 1.0,
            to_value: -1.0,
        });

        let ctx = DensityContext::new(0, -64, 0);
        assert!((func.compute(&ctx) - 1.0).abs() < 1e-10);

        let ctx = DensityContext::new(0, 320, 0);
        assert!((func.compute(&ctx) - (-1.0)).abs() < 1e-10);

        let ctx = DensityContext::new(0, 128, 0);
        assert!(func.compute(&ctx).abs() < 0.1);
    }

    #[test]
    fn test_add() {
        let a = Arc::new(DensityFunction::constant(10.0));
        let b = Arc::new(DensityFunction::constant(5.0));
        let func = DensityFunction::TwoArgumentSimple(TwoArgumentSimple {
            op: TwoArgType::Add,
            argument1: a,
            argument2: b,
        });
        let ctx = DensityContext::new(0, 64, 0);
        assert!((func.compute(&ctx) - 15.0).abs() < 1e-10);
    }

    #[test]
    fn test_mul() {
        let a = Arc::new(DensityFunction::constant(3.0));
        let b = Arc::new(DensityFunction::constant(4.0));
        let func = DensityFunction::TwoArgumentSimple(TwoArgumentSimple {
            op: TwoArgType::Mul,
            argument1: a,
            argument2: b,
        });
        let ctx = DensityContext::new(0, 64, 0);
        assert!((func.compute(&ctx) - 12.0).abs() < 1e-10);
    }

    #[test]
    fn test_clamp() {
        let func = DensityFunction::Clamp(Clamp {
            input: Arc::new(DensityFunction::constant(10.0)),
            min: -1.0,
            max: 1.0,
        });
        let ctx = DensityContext::new(0, 64, 0);
        assert!((func.compute(&ctx) - 1.0).abs() < 1e-10);

        let func = DensityFunction::Clamp(Clamp {
            input: Arc::new(DensityFunction::constant(-10.0)),
            min: -1.0,
            max: 1.0,
        });
        assert!((func.compute(&ctx) - (-1.0)).abs() < 1e-10);
    }

    #[test]
    fn test_half_negative() {
        let ctx = DensityContext::new(0, 64, 0);

        let func = DensityFunction::Mapped(Mapped {
            op: MappedType::HalfNegative,
            input: Arc::new(DensityFunction::constant(2.0)),
        });
        assert!((func.compute(&ctx) - 2.0).abs() < 1e-10);

        let func = DensityFunction::Mapped(Mapped {
            op: MappedType::HalfNegative,
            input: Arc::new(DensityFunction::constant(-2.0)),
        });
        assert!((func.compute(&ctx) - (-1.0)).abs() < 1e-10);
    }

    #[test]
    fn test_squeeze() {
        let ctx = DensityContext::new(0, 64, 0);

        let func = DensityFunction::Mapped(Mapped {
            op: MappedType::Squeeze,
            input: Arc::new(DensityFunction::constant(0.0)),
        });
        assert!(func.compute(&ctx).abs() < 1e-10);

        let func = DensityFunction::Mapped(Mapped {
            op: MappedType::Squeeze,
            input: Arc::new(DensityFunction::constant(1.0)),
        });
        let v = func.compute(&ctx);
        assert!((v - (0.5 - 1.0 / 24.0)).abs() < 1e-10);
    }

    #[test]
    fn test_noise_with_baked_noise() {
        let noises = make_test_noises();
        let func = DensityFunction::Noise(Noise {
            noise_id: "test_noise".to_string(),
            xz_scale: 1.0,
            y_scale: 1.0,
            noise: noises.get("test_noise").cloned(),
        });
        let ctx = DensityContext::new(100, 64, 200);
        // Just verify it returns a non-zero value (noise is deterministic)
        let v = func.compute(&ctx);
        // noise can be 0 but unlikely at these coords; mainly verify it doesn't panic
        let _ = v;
        // More importantly, it shouldn't panic
    }

    #[test]
    fn test_reference_resolved() {
        let target = Arc::new(DensityFunction::constant(7.5));
        let func = DensityFunction::Reference(Reference {
            id: "test:ref".to_string(),
            resolved: Some(target),
        });
        let ctx = DensityContext::new(0, 64, 0);
        assert!((func.compute(&ctx) - 7.5).abs() < 1e-10);
    }

    #[test]
    fn test_reference_unresolved() {
        let func = DensityFunction::Reference(Reference {
            id: "test:missing".to_string(),
            resolved: None,
        });
        let ctx = DensityContext::new(0, 64, 0);
        assert!((func.compute(&ctx) - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_resolve_bakes_noises() {
        let noises = make_test_noises();
        let registry = FxHashMap::default();

        let func = DensityFunction::Noise(Noise {
            noise_id: "test_noise".to_string(),
            xz_scale: 1.0,
            y_scale: 1.0,
            noise: None, // not yet baked
        });

        // Before resolve, noise is None → returns 0.0
        let ctx = DensityContext::new(100, 64, 200);
        assert!((func.compute(&ctx) - 0.0).abs() < 1e-10);

        // After resolve, noise is baked → returns actual noise value
        let resolved = func.resolve(&registry, &noises);
        let v = resolved.compute(&ctx);
        // The resolved function should have a baked noise
        if let DensityFunction::Noise(n) = &resolved {
            assert!(n.noise.is_some());
        } else {
            panic!("Expected Noise variant");
        }
        // Value may or may not be 0.0 depending on noise, but it shouldn't panic
        let _ = v;
    }
}
