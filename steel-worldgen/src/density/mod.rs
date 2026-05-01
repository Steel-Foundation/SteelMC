//! Density function types and transpiler for world generation.
//!
//! Density functions form a tree structure parsed from JSON at build time.
//! The transpiler compiles these trees into native Rust code — runtime evaluation
//! is done by the transpiled output, not by interpreting this tree.
//!
//! # Key Types
//!
//! - [`DensityFunction`] - The density function enum with all operation types
//! - [`NoiseRouter`] - Collection of all density functions for world generation
//! - [`CubicSpline`] - Cubic spline interpolation for smooth terrain transitions
//! - [`RarityValueMapper`] - Used at runtime by transpiled cave generation code
//! - [`DimensionNoises`] - Trait for dimension-specific noise generators
//! - [`NoiseSettings`] - Trait for dimension-specific settings from datapack

pub mod spline_eval;
mod traits;
mod types;

pub use traits::{ColumnCache, DimensionNoises, NoiseSettings};
pub use types::{NoiseParameters, RarityValueMapper};
