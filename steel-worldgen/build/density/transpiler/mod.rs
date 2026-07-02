//! Density function transpiler: compiles `DensityFunction` trees into native Rust functions.
//!
//! This module takes a registry of named `DensityFunction` trees and noise router entry
//! points, and generates Rust source code (`proc_macro2::TokenStream`) that evaluates
//! them as compiled functions — eliminating runtime tree interpretation, HashMap-based
//! caching, and Arc pointer chasing.
//!
//! # Usage
//!
//! ```ignore
//! let input = TranspilerInput {
//!     registry,       // BTreeMap<String, DensityFunction>
//!     router_entries, // BTreeMap<String, DensityFunction>
//! };
//! let tokens: TokenStream = transpile(&input);
//! ```
//!
//! # Module layout
//!
//! - `context`: transpilation state (`TranspileContext`)
//! - `analyze`: phase 1 — graph walk, flatness inference, topo sort
//! - `graph`: tree walks for references, Y usage, interpolated/blended markers
//! - `fingerprint`: structural hashing and CSE candidate collection
//! - `bounds`: static value bounds for branch-elision in codegen
//! - `naming`: registry IDs → valid Rust identifiers
//! - `codegen_structs`: `{Prefix}Noises` and `{Prefix}ColumnCache` emission
//! - `codegen_functions`: named `compute_*` and public `router_*` functions
//! - `codegen_expr`: per-variant expression codegen (scalar and SIMD)
//!
//! Gated behind the `codegen` feature flag.

use std::collections::BTreeMap;

use proc_macro2::TokenStream;
use quote::quote;

use crate::density::DensityFunction;

mod analyze;
mod bounds;
mod codegen_expr;
mod codegen_functions;
mod codegen_structs;
mod context;
mod fingerprint;
mod graph;
mod naming;

/// Input to the transpiler.
pub struct TranspilerInput {
    /// Named density functions (registry entries like `"minecraft:overworld/continents"`).
    pub registry: BTreeMap<String, DensityFunction>,
    /// Noise router entry points (like `"temperature"`, `"final_density"`).
    pub router_entries: BTreeMap<String, DensityFunction>,
    /// Prefix for generated struct names (e.g., `"Overworld"` → `OverworldNoises`, `OverworldColumnCache`).
    pub prefix: String,
    /// Cell width in blocks (XZ direction). Determines the `FlatCache` grid size:
    /// `grid_side = (16 / cell_width) + 1`, total entries = `grid_side²`.
    pub cell_width: i32,
    /// Whether this dimension uses Java's LCG random (`true`) or Xoroshiro (`false`).
    ///
    /// When `true`, vanilla's `RandomState` intercepts noise creation:
    /// - Temperature/vegetation use `NormalNoise.createLegacyNetherBiome()` with
    ///   hardcoded params `(-7, [1.0, 1.0])` and direct `LegacyRandom(seed)`.
    /// - `BlendedNoise` uses `LegacyRandom(seed)` instead of the positional splitter.
    pub legacy_random_source: bool,
}

/// Compile density function trees into a `TokenStream` of Rust code.
///
/// The generated code contains:
/// - `{Prefix}Noises` struct with one `NormalNoise` field per noise used
/// - `{Prefix}ColumnCache` struct with fields for flat-cached (xz-only) values
/// - Private `compute_*` functions for each named density function
/// - Public `router_*` functions for each noise router entry point
#[must_use]
pub fn transpile(input: &TranspilerInput) -> TokenStream {
    let mut ctx = context::TranspileContext::new(&input.prefix);
    ctx.legacy_random_source = input.legacy_random_source;

    // Phase 1: Analyze the graph
    ctx.analyze(input);

    // Phase 2: Generate code
    let noises_struct = ctx.gen_noises_struct();
    let noises_impl = ctx.gen_noises_impl();
    let named_fns = ctx.gen_named_functions(input);
    let column_cache = ctx.gen_column_cache(input);
    let router_fns = ctx.gen_router_functions(input);

    // Imports are emitted here so each dimension's output is self-contained
    // when wrapped in a module by the caller.
    quote! {
        use std::simd::f64x4;
        use std::simd::Select;
        use std::simd::cmp::SimdPartialOrd;
        use std::simd::num::SimdFloat;

        use steel_worldgen::density::spline_eval;
        use steel_worldgen::density::RarityValueMapper;
        use steel_math::{clamp, map_clamped};
        use steel_worldgen::noise::NormalNoise;
        use steel_worldgen::random::{PositionalRandom, RandomSplitter};

        #noises_struct
        #noises_impl
        #column_cache
        #named_fns
        #router_fns
    }
}
