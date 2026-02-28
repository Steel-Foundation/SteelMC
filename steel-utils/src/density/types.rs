//! Density function types matching vanilla Minecraft's DensityFunctions.java
//!
//! Each density function type is its own struct, mirroring vanilla's separate
//! record/class pattern. The [`DensityFunction`] enum wraps them for dispatch.
//!
//! These types are used at build time by the density function transpiler to parse
//! JSON density function trees and generate native Rust code. Runtime evaluation
//! is done by the transpiled code, not by interpreting this tree.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::noise::NormalNoise;

// ── Individual density function structs ──────────────────────────────────────

/// A constant density value.
///
/// Matches vanilla's `DensityFunctions.Constant`.
#[derive(Debug, Clone)]
pub struct Constant {
    /// The constant value.
    pub value: f64,
}

/// A reference to another density function by ID.
///
/// After resolution via [`DensityFunction::resolve`], `resolved` holds the
/// target function. Matches vanilla's `DensityFunctions.HolderHolder`.
#[derive(Debug, Clone)]
pub struct Reference {
    /// The density function ID (for debugging / serialization).
    pub id: String,
    /// Resolved target (set by [`DensityFunction::resolve`]).
    pub resolved: Option<Arc<DensityFunction>>,
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
    /// Invert: 1.0 / v
    Invert,
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

/// Blend density (for chunk blending).
///
/// Matches vanilla's `DensityFunctions.BlendDensity`.
#[derive(Debug, Clone)]
pub struct BlendDensity {
    /// Input density function
    pub input: Arc<DensityFunction>,
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

/// Cubic spline density function wrapper.
///
/// Wraps an `Arc<CubicSpline>` for spline-based density evaluation.
/// Matches vanilla's `DensityFunctions.Spline`.
#[derive(Debug, Clone)]
pub struct Spline {
    /// The cubic spline.
    pub spline: Arc<CubicSpline>,
}

/// Blend alpha density function (returns 1.0, placeholder for blending).
///
/// Matches vanilla's `DensityFunctions.BlendAlpha`.
#[derive(Debug, Clone, Copy)]
pub struct BlendAlpha;

/// Blend offset density function (returns 0.0, placeholder for blending).
///
/// Matches vanilla's `DensityFunctions.BlendOffset`.
#[derive(Debug, Clone, Copy)]
pub struct BlendOffset;

// ── DensityFunction enum (dispatch wrapper) ─────────────────────────────────

/// A density function that can be evaluated at a position to get a density value.
///
/// Density functions form a tree structure where complex functions are composed
/// from simpler ones. Each variant wraps a separate struct matching vanilla's
/// per-type class/record pattern.
///
/// This tree is used at build time by the transpiler to generate native Rust code.
/// Runtime evaluation is done by the transpiled output, not by interpreting this tree.
#[derive(Debug, Clone)]
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

    /// End islands density function (transpiler emits 0.0; actual algorithm
    /// lives in `steel-core::worldgen::end_islands::EndIslands`).
    EndIslands,

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
            | Self::EndIslands
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
                noise: noises.get("minecraft:offset").cloned(),
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
    let points: Vec<SplinePoint> = spline
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
        .collect();
    CubicSpline::new(
        Arc::new(spline.coordinate.resolve_inner(registry, noises)),
        points,
    )
}

// ── Supporting types ────────────────────────────────────────────────────────

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

/// Rarity value mapper for cave generation.
///
/// Used at runtime by transpiled `WeirdScaledSampler` code.
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
    /// Pre-extracted point locations for binary search (avoids allocation per eval).
    pub locations: Vec<f32>,
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
    pub fn new(coordinate: Arc<DensityFunction>, points: Vec<SplinePoint>) -> Self {
        let locations = points.iter().map(|p| p.location).collect();
        Self {
            coordinate,
            points,
            locations,
        }
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
    fn test_resolve_bakes_noises() {
        use crate::random::Random;
        use crate::random::xoroshiro::Xoroshiro;

        let mut rng = Xoroshiro::from_seed(12345);
        let splitter = rng.next_positional();

        let mut noises = FxHashMap::default();
        let noise = NormalNoise::create(&splitter, "test_noise", -4, &[1.0, 1.0, 1.0, 1.0]);
        noises.insert("test_noise".to_string(), noise);

        let registry = FxHashMap::default();

        let func = DensityFunction::Noise(Noise {
            noise_id: "test_noise".to_string(),
            xz_scale: 1.0,
            y_scale: 1.0,
            noise: None, // not yet baked
        });

        // After resolve, noise should be baked
        let resolved = func.resolve(&registry, &noises);
        if let DensityFunction::Noise(n) = &resolved {
            assert!(n.noise.is_some());
        } else {
            panic!("Expected Noise variant");
        }
    }
}
