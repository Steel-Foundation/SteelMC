//! Transpiler state accumulated across analysis and codegen phases.
//!
//! Holds discovered noise IDs, flat-cache metadata, CSE bindings, spline helper
//! fragments, and mode flags (`fill_mode`, `interpolated_param_mode`) toggled
//! while generating interpolation/combine functions.

use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::FxHashMap;

use proc_macro2::{Ident, TokenStream};
use quote::format_ident;

use crate::density::BlendedNoise as BlendedNoiseConfig;
// ── Internal types ──────────────────────────────────────────────────────────

/// Tracks state during transpilation.
pub(super) struct TranspileContext {
    /// All noise IDs referenced by any density function.
    pub(super) noise_ids: BTreeSet<String>,
    /// Named functions that are flat-cached (xz-only).
    pub(super) flat_cached: BTreeSet<String>,
    /// Router entries that are Y-independent (inferred flat).
    /// Their results are cached in the column cache.
    pub(super) flat_routers: BTreeSet<String>,
    /// Named functions in topological order (dependencies first).
    pub(super) topo_order: Vec<String>,
    /// Named functions that are actually reachable from router entries.
    pub(super) used_names: BTreeSet<String>,
    /// Counter for generating unique spline function names.
    pub(super) spline_counter: usize,
    /// Generated spline helper functions.
    pub(super) spline_fns: Vec<TokenStream>,
    /// Generated ident for the noises struct (e.g., `OverworldNoises`).
    pub(super) noises_ident: Ident,
    /// Generated ident for the column cache struct (e.g., `OverworldColumnCache`).
    pub(super) cache_ident: Ident,
    /// `BlendedNoise` configuration (if any density function uses it).
    pub(super) blended_noise_config: Option<BlendedNoiseConfig>,
    /// Whether this dimension uses legacy random source (Java LCG).
    pub(super) legacy_random_source: bool,
    /// Whether any density function uses `EndIslands`.
    pub(super) uses_end_islands: bool,
    /// When true, `BlendedNoise` emits the `blended_noise_value` parameter
    /// instead of calling `noises.blended_noise.compute(x, y, z)`.
    pub(super) fill_mode: bool,
    /// When true, `Interpolated` markers emit `interpolated[i]` parameter references
    /// instead of recursing into the wrapped function.
    pub(super) interpolated_param_mode: bool,
    /// Counter for assigning indices to `Interpolated` markers in param mode.
    pub(super) interpolated_param_counter: usize,
    /// Named functions that (transitively) contain `Interpolated` markers.
    /// In param mode, these are inlined instead of called as functions.
    pub(super) interpolated_refs: BTreeSet<String>,
    /// Named functions that (transitively) contain `BlendedNoise`.
    /// In fill mode, these are inlined so the precomputed value is used.
    pub(super) blended_noise_refs: BTreeSet<String>,
    /// CSE bindings keyed by structural hash. When a subexpression has been
    /// hoisted into a `let` binding, subsequent occurrences emit the variable
    /// name instead of recomputing. Covers `Reference`, `Noise`,
    /// `ShiftedNoise`, and other expensive nodes.
    pub(super) cse_bindings: FxHashMap<u64, Ident>,
    /// CSE bindings for the SIMD (`_4x`) codegen path. Kept separate from
    /// `cse_bindings` because SIMD bindings hold `f64x4` values: if the scalar
    /// 4×-lane fallback (`gen_simd_scalar_fallback`) looked one up it would emit
    /// an `f64x4` where an `f64` is expected. Same fingerprint keys, disjoint
    /// codegen scopes.
    pub(super) cse_bindings_simd: FxHashMap<u64, Ident>,
    /// Counter for generating unique CSE variable names.
    pub(super) cse_counter: usize,
    /// Inline `Noise` nodes with `y_scale == 0.0` found inside non-flat
    /// functions. These are Y-independent but get recomputed per Y corner;
    /// caching them in the column cache avoids ~48 redundant evaluations per
    /// column. Keyed by structural hash, value is `(index, noise_id, xz_scale)`.
    pub(super) inline_flat_noises: BTreeMap<u64, (usize, String, f64)>,
}

impl TranspileContext {
    pub(super) fn new(prefix: &str) -> Self {
        Self {
            noise_ids: BTreeSet::new(),
            flat_cached: BTreeSet::new(),
            flat_routers: BTreeSet::new(),
            topo_order: Vec::new(),
            used_names: BTreeSet::new(),
            spline_counter: 0,
            spline_fns: Vec::new(),
            noises_ident: format_ident!("{prefix}Noises"),
            cache_ident: format_ident!("{prefix}ColumnCache"),
            blended_noise_config: None,
            legacy_random_source: false,
            uses_end_islands: false,
            fill_mode: false,
            interpolated_param_mode: false,
            interpolated_param_counter: 0,
            interpolated_refs: BTreeSet::new(),
            blended_noise_refs: BTreeSet::new(),
            cse_bindings: FxHashMap::default(),
            cse_bindings_simd: FxHashMap::default(),
            cse_counter: 0,
            inline_flat_noises: BTreeMap::new(),
        }
    }
}
