//! Climate sampler for vanilla world generation.
//!
//! This module bridges the extracted density functions from steel-registry
//! with the climate sampling system in steel-utils.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use steel_registry::density_functions::{
    self, CubicSpline as GenCubicSpline, DensityFunction as GenDensityFunction,
    OVERWORLD_NOISE_ROUTER, RarityValueMapper as GenRarityValueMapper,
};
use steel_registry::noise_parameters::get_noise_parameters;
use steel_utils::climate::{TargetPoint, quantize_coord};
use steel_utils::density::{
    BlendAlpha, BlendDensity, BlendOffset, BlendedNoise, Clamp, Constant, CubicSpline,
    DensityContext, DensityFunction, DensityFunctionOps, EndIslands, EvalCache, Mapped, MappedType,
    Marker, MarkerType, Noise, RangeChoice, RarityValueMapper, Reference, Shift, ShiftA, ShiftB,
    ShiftedNoise, Spline, SplinePoint, SplineValue, TwoArgType, TwoArgumentSimple,
    WeirdScaledSampler, YClampedGradient,
};
use steel_utils::noise::NormalNoise;
use steel_utils::random::{Random, xoroshiro::Xoroshiro};

/// Climate sampler that uses the extracted vanilla density functions.
pub struct VanillaClimateSampler {
    /// Temperature density function (resolved with baked noises)
    temperature: Arc<DensityFunction>,
    /// Humidity/vegetation density function
    humidity: Arc<DensityFunction>,
    /// Continentalness density function
    continentalness: Arc<DensityFunction>,
    /// Erosion density function
    erosion: Arc<DensityFunction>,
    /// Depth density function
    depth: Arc<DensityFunction>,
    /// Weirdness/ridges density function
    weirdness: Arc<DensityFunction>,
}

impl VanillaClimateSampler {
    /// Create a new vanilla climate sampler with the given seed.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        // Create random number generator from seed
        let mut rng = Xoroshiro::from_seed(seed);
        let splitter = rng.next_positional();

        // Build noise generators from vanilla datapack parameters
        let noise_param_map = get_noise_parameters();
        let mut noises = FxHashMap::default();
        for (id, params) in &noise_param_map {
            let noise = NormalNoise::create(&splitter, id, params.first_octave, &params.amplitudes);
            noises.insert(id.clone(), noise);
        }

        // Build density function registry from generated code (unresolved)
        let registry = build_density_function_registry();

        // Get the overworld noise router and convert to steel-utils types (unresolved)
        let router = &*OVERWORLD_NOISE_ROUTER;

        // Convert and then resolve each density function (bakes noises + registry)
        let temperature =
            Arc::new(convert_density_function(&router.temperature).resolve(&registry, &noises));
        let humidity =
            Arc::new(convert_density_function(&router.vegetation).resolve(&registry, &noises));
        let continentalness =
            Arc::new(convert_density_function(&router.continents).resolve(&registry, &noises));
        let erosion =
            Arc::new(convert_density_function(&router.erosion).resolve(&registry, &noises));
        let depth = Arc::new(convert_density_function(&router.depth).resolve(&registry, &noises));
        let weirdness =
            Arc::new(convert_density_function(&router.ridges).resolve(&registry, &noises));

        Self {
            temperature,
            humidity,
            continentalness,
            erosion,
            depth,
            weirdness,
        }
    }

    /// Sample climate at a quart position using a reusable cache.
    ///
    /// The cache should persist across multiple calls (e.g. for an entire chunk)
    /// to benefit from `FlatCache` (caches by x,z) and `CacheOnce` (caches last position).
    #[must_use]
    pub fn sample(
        &self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
        cache: &mut EvalCache,
    ) -> TargetPoint {
        let block_x = quart_x << 2;
        let block_y = quart_y << 2;
        let block_z = quart_z << 2;

        let ctx = DensityContext::new(block_x, block_y, block_z);

        let temp = self.temperature.compute_cached(&ctx, cache) as f32;
        let humidity = self.humidity.compute_cached(&ctx, cache) as f32;
        let cont = self.continentalness.compute_cached(&ctx, cache) as f32;
        let erosion = self.erosion.compute_cached(&ctx, cache) as f32;
        let depth = self.depth.compute_cached(&ctx, cache) as f32;
        let weirdness = self.weirdness.compute_cached(&ctx, cache) as f32;

        TargetPoint::new(
            quantize_coord(f64::from(temp)),
            quantize_coord(f64::from(humidity)),
            quantize_coord(f64::from(cont)),
            quantize_coord(f64::from(erosion)),
            quantize_coord(f64::from(depth)),
            quantize_coord(f64::from(weirdness)),
        )
    }
}

/// Build the density function registry from generated code.
///
/// These are returned as unresolved `DensityFunction` values; call
/// [`DensityFunction::resolve`] on each root to bake noises + cross-references.
fn build_density_function_registry() -> FxHashMap<String, Arc<DensityFunction>> {
    let mut registry = FxHashMap::default();

    // Get all density functions from the generated registry
    let ids = [
        "minecraft:overworld/continents",
        "minecraft:overworld/erosion",
        "minecraft:overworld/depth",
        "minecraft:overworld/ridges",
        "minecraft:overworld/ridges_folded",
        "minecraft:overworld/offset",
        "minecraft:overworld/factor",
        "minecraft:overworld/jaggedness",
        "minecraft:overworld/sloped_cheese",
        "minecraft:overworld/base_3d_noise",
        "minecraft:overworld/caves/entrances",
        "minecraft:overworld/caves/spaghetti_2d",
        "minecraft:overworld/caves/spaghetti_2d_thickness_modulator",
        "minecraft:overworld/caves/spaghetti_roughness_function",
        "minecraft:overworld/caves/pillars",
        "minecraft:overworld/caves/noodle",
        "minecraft:shift_x",
        "minecraft:shift_z",
        "minecraft:y",
        "minecraft:zero",
    ];

    for id in ids {
        if let Some(gen_func) = density_functions::get_density_function(id) {
            registry.insert(id.to_string(), convert_density_function(&gen_func));
        }
    }

    registry
}

/// Convert a generated `DensityFunction` to the steel-utils `DensityFunction` type.
///
/// The returned function is *unresolved* — noise fields are `None` and references
/// store only their string ID. Call [`DensityFunction::resolve`] to bake everything.
#[allow(clippy::too_many_lines)]
fn convert_density_function(df: &GenDensityFunction) -> Arc<DensityFunction> {
    Arc::new(match df {
        GenDensityFunction::Constant(v) => DensityFunction::Constant(Constant { value: *v }),

        GenDensityFunction::Reference(id) => DensityFunction::Reference(Reference {
            id: id.to_string(),
            resolved: None,
        }),

        GenDensityFunction::YClampedGradient {
            from_y,
            to_y,
            from_value,
            to_value,
        } => DensityFunction::YClampedGradient(YClampedGradient {
            from_y: *from_y,
            to_y: *to_y,
            from_value: *from_value,
            to_value: *to_value,
        }),

        GenDensityFunction::Noise {
            noise_id,
            xz_scale,
            y_scale,
        } => DensityFunction::Noise(Noise {
            noise_id: noise_id.to_string(),
            xz_scale: *xz_scale,
            y_scale: *y_scale,
            noise: None,
        }),

        GenDensityFunction::ShiftedNoise {
            shift_x,
            shift_y,
            shift_z,
            xz_scale,
            y_scale,
            noise_id,
        } => DensityFunction::ShiftedNoise(ShiftedNoise {
            shift_x: convert_density_function(shift_x),
            shift_y: convert_density_function(shift_y),
            shift_z: convert_density_function(shift_z),
            xz_scale: *xz_scale,
            y_scale: *y_scale,
            noise_id: noise_id.to_string(),
            noise: None,
        }),

        GenDensityFunction::ShiftA { noise_id } => DensityFunction::ShiftA(ShiftA {
            noise_id: noise_id.to_string(),
            noise: None,
        }),

        GenDensityFunction::ShiftB { noise_id } => DensityFunction::ShiftB(ShiftB {
            noise_id: noise_id.to_string(),
            noise: None,
        }),

        GenDensityFunction::Shift { noise_id } => DensityFunction::Shift(Shift {
            noise_id: noise_id.to_string(),
            noise: None,
        }),

        GenDensityFunction::Clamp { input, min, max } => DensityFunction::Clamp(Clamp {
            input: convert_density_function(input),
            min: *min,
            max: *max,
        }),

        // Mapped operations (individual variants in generated code -> unified Mapped struct)
        GenDensityFunction::Abs(input) => DensityFunction::Mapped(Mapped {
            op: MappedType::Abs,
            input: convert_density_function(input),
        }),
        GenDensityFunction::Square(input) => DensityFunction::Mapped(Mapped {
            op: MappedType::Square,
            input: convert_density_function(input),
        }),
        GenDensityFunction::Cube(input) => DensityFunction::Mapped(Mapped {
            op: MappedType::Cube,
            input: convert_density_function(input),
        }),
        GenDensityFunction::HalfNegative(input) => DensityFunction::Mapped(Mapped {
            op: MappedType::HalfNegative,
            input: convert_density_function(input),
        }),
        GenDensityFunction::QuarterNegative(input) => DensityFunction::Mapped(Mapped {
            op: MappedType::QuarterNegative,
            input: convert_density_function(input),
        }),
        GenDensityFunction::Squeeze(input) => DensityFunction::Mapped(Mapped {
            op: MappedType::Squeeze,
            input: convert_density_function(input),
        }),

        // Two-argument operations (individual variants -> unified TwoArgumentSimple struct)
        GenDensityFunction::Add(a, b) => DensityFunction::TwoArgumentSimple(TwoArgumentSimple {
            op: TwoArgType::Add,
            argument1: convert_density_function(a),
            argument2: convert_density_function(b),
        }),
        GenDensityFunction::Mul(a, b) => DensityFunction::TwoArgumentSimple(TwoArgumentSimple {
            op: TwoArgType::Mul,
            argument1: convert_density_function(a),
            argument2: convert_density_function(b),
        }),
        GenDensityFunction::Min(a, b) => DensityFunction::TwoArgumentSimple(TwoArgumentSimple {
            op: TwoArgType::Min,
            argument1: convert_density_function(a),
            argument2: convert_density_function(b),
        }),
        GenDensityFunction::Max(a, b) => DensityFunction::TwoArgumentSimple(TwoArgumentSimple {
            op: TwoArgType::Max,
            argument1: convert_density_function(a),
            argument2: convert_density_function(b),
        }),

        GenDensityFunction::Spline(spline) => DensityFunction::Spline(Spline {
            spline: Arc::new(convert_spline(spline)),
        }),

        GenDensityFunction::RangeChoice {
            input,
            min_inclusive,
            max_exclusive,
            when_in_range,
            when_out_of_range,
        } => DensityFunction::RangeChoice(RangeChoice {
            input: convert_density_function(input),
            min_inclusive: *min_inclusive,
            max_exclusive: *max_exclusive,
            when_in_range: convert_density_function(when_in_range),
            when_out_of_range: convert_density_function(when_out_of_range),
        }),

        // Cache/marker variants (individual variants -> unified Marker struct)
        GenDensityFunction::Interpolated(arg) => DensityFunction::Marker(Marker {
            kind: MarkerType::Interpolated,
            wrapped: convert_density_function(arg),
        }),
        GenDensityFunction::FlatCache(arg) => DensityFunction::Marker(Marker {
            kind: MarkerType::FlatCache,
            wrapped: convert_density_function(arg),
        }),
        GenDensityFunction::CacheOnce(arg) => DensityFunction::Marker(Marker {
            kind: MarkerType::CacheOnce,
            wrapped: convert_density_function(arg),
        }),
        GenDensityFunction::Cache2d(arg) => DensityFunction::Marker(Marker {
            kind: MarkerType::Cache2D,
            wrapped: convert_density_function(arg),
        }),
        GenDensityFunction::CacheAllInCell(arg) => DensityFunction::Marker(Marker {
            kind: MarkerType::CacheAllInCell,
            wrapped: convert_density_function(arg),
        }),

        GenDensityFunction::BlendOffset => DensityFunction::BlendOffset(BlendOffset),
        GenDensityFunction::BlendAlpha => DensityFunction::BlendAlpha(BlendAlpha),
        GenDensityFunction::BlendDensity(input) => DensityFunction::BlendDensity(BlendDensity {
            input: convert_density_function(input),
        }),

        // TODO: Implement Beardifier for structure terrain adaptation.
        // Constant(0.0) is correct when structures are not yet generated.
        GenDensityFunction::Beardifier => DensityFunction::Constant(Constant { value: 0.0 }),
        GenDensityFunction::EndIslands => DensityFunction::EndIslands(EndIslands),

        GenDensityFunction::WeirdScaledSampler {
            input,
            noise_id,
            rarity_value_mapper,
        } => DensityFunction::WeirdScaledSampler(WeirdScaledSampler {
            input: convert_density_function(input),
            noise_id: noise_id.to_string(),
            rarity_value_mapper: match rarity_value_mapper {
                GenRarityValueMapper::Tunnels => RarityValueMapper::Tunnels,
                GenRarityValueMapper::Caves => RarityValueMapper::Caves,
            },
            noise: None,
        }),

        GenDensityFunction::OldBlendedNoise {
            xz_scale,
            y_scale,
            xz_factor,
            y_factor,
            smear_scale_multiplier,
        } => DensityFunction::BlendedNoise(BlendedNoise {
            xz_scale: *xz_scale,
            y_scale: *y_scale,
            xz_factor: *xz_factor,
            y_factor: *y_factor,
            smear_scale_multiplier: *smear_scale_multiplier,
            noise: None,
        }),
    })
}

/// Convert a generated `CubicSpline` to the steel-utils `CubicSpline` type.
fn convert_spline(spline: &GenCubicSpline) -> CubicSpline {
    match spline {
        GenCubicSpline::Constant(v) => CubicSpline {
            coordinate: Arc::new(DensityFunction::constant(0.0)),
            points: vec![SplinePoint {
                location: 0.0,
                value: SplineValue::Constant(*v),
                derivative: 0.0,
            }],
        },
        GenCubicSpline::Multipoint { coordinate, points } => CubicSpline {
            coordinate: convert_density_function(coordinate),
            points: points
                .iter()
                .map(|p| SplinePoint {
                    location: p.location,
                    value: convert_spline_value(&p.value),
                    derivative: p.derivative,
                })
                .collect(),
        },
    }
}

/// Convert a generated spline value.
fn convert_spline_value(spline: &GenCubicSpline) -> SplineValue {
    match spline {
        GenCubicSpline::Constant(v) => SplineValue::Constant(*v),
        GenCubicSpline::Multipoint { .. } => SplineValue::Spline(Arc::new(convert_spline(spline))),
    }
}
