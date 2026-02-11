//! Density function system for world generation.
//!
//! This module provides the density function evaluation system used for terrain generation
//! in vanilla Minecraft. Density functions form a tree structure that is evaluated at
//! each position to determine terrain shape, biome climate values, and other world features.
//!
//! # Key Types
//!
//! - [`DensityFunction`] - The density function enum with all operation types
//! - [`DensityFunctionOps`] - Trait providing `compute()` and `compute_cached()` methods
//! - [`DensityContext`] - The position context for evaluation
//! - [`EvalCache`] - Cache for avoiding redundant evaluations
//! - [`NoiseRouter`] - Collection of all density functions for world generation
//! - [`CubicSpline`] - Cubic spline interpolation for smooth terrain transitions
//!
//! # Struct-per-type pattern
//!
//! Each density function type has its own struct (e.g. [`Constant`], [`Noise`], [`Mapped`]),
//! mirroring vanilla Minecraft's separate record/class pattern. The [`DensityFunction`] enum
//! wraps them for dispatch.
//!
//! # Evaluation
//!
//! Density functions implement the [`DensityFunctionOps`] trait. Noise generators and
//! registry references are baked into the tree at construction time via
//! [`DensityFunction::resolve`], so evaluation needs only a [`DensityContext`].

mod types;

pub use types::{
    BlendAlpha, BlendDensity, BlendOffset, BlendedNoise, Clamp, Constant, CubicSpline,
    DensityContext, DensityFunction, DensityFunctionOps, EndIslands, EvalCache, Mapped, MappedType,
    Marker, MarkerType, Noise, NoiseParameters, NoiseRouter, RangeChoice, RarityValueMapper,
    Reference, Shift, ShiftA, ShiftB, ShiftedNoise, Spline, SplinePoint, SplineValue, TwoArgType,
    TwoArgumentSimple, WeirdScaledSampler, YClampedGradient,
};
