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
//! Gated behind the `codegen` feature flag.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::mem;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHasher};

use proc_macro2::{Ident, Literal, TokenStream};
use quote::{format_ident, quote};

use super::{
    BlendedNoise as BlendedNoiseConfig, CubicSpline, DensityFunction, MappedType, MarkerType,
    RarityValueMapper, SplineValue, TwoArgType,
};

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
    let mut ctx = TranspileContext::new(&input.prefix);
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

// ── Internal types ──────────────────────────────────────────────────────────

/// Tracks state during transpilation.
struct TranspileContext {
    /// All noise IDs referenced by any density function.
    noise_ids: BTreeSet<String>,
    /// Named functions that are flat-cached (xz-only).
    flat_cached: BTreeSet<String>,
    /// Router entries that are Y-independent (inferred flat).
    /// Their results are cached in the column cache.
    flat_routers: BTreeSet<String>,
    /// Named functions in topological order (dependencies first).
    topo_order: Vec<String>,
    /// Named functions that are actually reachable from router entries.
    used_names: BTreeSet<String>,
    /// Counter for generating unique spline function names.
    spline_counter: usize,
    /// Generated spline helper functions.
    spline_fns: Vec<TokenStream>,
    /// Generated ident for the noises struct (e.g., `OverworldNoises`).
    noises_ident: Ident,
    /// Generated ident for the column cache struct (e.g., `OverworldColumnCache`).
    cache_ident: Ident,
    /// `BlendedNoise` configuration (if any density function uses it).
    blended_noise_config: Option<BlendedNoiseConfig>,
    /// Whether this dimension uses legacy random source (Java LCG).
    legacy_random_source: bool,
    /// Whether any density function uses `EndIslands`.
    uses_end_islands: bool,
    /// When true, `BlendedNoise` emits the `blended_noise_value` parameter
    /// instead of calling `noises.blended_noise.compute(x, y, z)`.
    fill_mode: bool,
    /// When true, `Interpolated` markers emit `interpolated[i]` parameter references
    /// instead of recursing into the wrapped function.
    interpolated_param_mode: bool,
    /// Counter for assigning indices to `Interpolated` markers in param mode.
    interpolated_param_counter: usize,
    /// Named functions that (transitively) contain `Interpolated` markers.
    /// In param mode, these are inlined instead of called as functions.
    interpolated_refs: BTreeSet<String>,
    /// Named functions that (transitively) contain `BlendedNoise`.
    /// In fill mode, these are inlined so the precomputed value is used.
    blended_noise_refs: BTreeSet<String>,
    /// CSE bindings keyed by structural hash. When a subexpression has been
    /// hoisted into a `let` binding, subsequent occurrences emit the variable
    /// name instead of recomputing. Covers `Reference`, `Noise`,
    /// `ShiftedNoise`, and other expensive nodes.
    cse_bindings: FxHashMap<u64, Ident>,
    /// CSE bindings for the SIMD (`_4x`) codegen path. Kept separate from
    /// `cse_bindings` because SIMD bindings hold `f64x4` values: if the scalar
    /// 4×-lane fallback (`gen_simd_scalar_fallback`) looked one up it would emit
    /// an `f64x4` where an `f64` is expected. Same fingerprint keys, disjoint
    /// codegen scopes.
    cse_bindings_simd: FxHashMap<u64, Ident>,
    /// Counter for generating unique CSE variable names.
    cse_counter: usize,
    /// Inline `Noise` nodes with `y_scale == 0.0` found inside non-flat
    /// functions. These are Y-independent but get recomputed per Y corner;
    /// caching them in the column cache avoids ~48 redundant evaluations per
    /// column. Keyed by structural hash, value is `(index, noise_id, xz_scale)`.
    inline_flat_noises: BTreeMap<u64, (usize, String, f64)>,
}

impl TranspileContext {
    fn new(prefix: &str) -> Self {
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

    // ── Phase 1: Analysis ───────────────────────────────────────────────

    fn analyze(&mut self, input: &TranspilerInput) {
        for df in input.router_entries.values() {
            self.walk_df(df, input);
        }

        // Mark explicitly flat-cached functions
        for name in &self.used_names {
            if let Some(df) = input.registry.get(name)
                && is_flat_cached(df)
            {
                self.flat_cached.insert(name.clone());
            }
        }

        // Infer flatness: a function is flat if it doesn't use y and all its
        // Reference dependencies are also flat. Iterate until convergence.
        loop {
            let mut changed = false;
            for name in &self.used_names.clone() {
                if self.flat_cached.contains(name) {
                    continue;
                }
                let Some(df) = input.registry.get(name) else {
                    continue;
                };
                let inner = unwrap_markers(df);
                if !uses_y(inner)
                    && collect_references(inner)
                        .iter()
                        .all(|dep| self.flat_cached.contains(dep))
                {
                    self.flat_cached.insert(name.clone());
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        // Infer flatness for router entries: a router entry is flat if it doesn't
        // use y and all its Reference dependencies are flat-cached named functions.
        // This catches cases like temperature/vegetation that are Y-independent but
        // lack explicit FlatCache markers in vanilla's JSON.
        for (name, df) in &input.router_entries {
            let inner = unwrap_markers(df);
            if is_flat_cached(df)
                || (!uses_y(inner)
                    && collect_references(inner)
                        .iter()
                        .all(|dep| self.flat_cached.contains(dep)))
            {
                self.flat_routers.insert(name.clone());
            }
        }

        // Collect Y-independent inline Noise nodes inside non-flat functions.
        // These get cached in the column cache to avoid per-Y-corner recomputation.
        {
            let mut seen = BTreeMap::new();
            for name in &self.used_names {
                if self.flat_cached.contains(name) {
                    continue;
                }
                if let Some(df) = input.registry.get(name) {
                    collect_inline_flat_noises(unwrap_markers(df), &mut seen);
                }
            }
            for (name, df) in &input.router_entries {
                if self.flat_routers.contains(name) {
                    continue;
                }
                collect_inline_flat_noises(unwrap_markers(df), &mut seen);
            }
            for (fp, (noise_id, xz_scale)) in seen {
                let idx = self.inline_flat_noises.len();
                self.inline_flat_noises
                    .insert(fp, (idx, noise_id, xz_scale));
            }
        }

        // Compute which named functions transitively contain Interpolated markers.
        // These must be inlined (not called) when generating combine_interpolated.
        for name in &self.used_names {
            if let Some(df) = input.registry.get(name) {
                let mut visited = BTreeSet::new();
                if has_interpolated_markers(df, &input.registry, &mut visited) {
                    self.interpolated_refs.insert(name.clone());
                }
                let mut visited = BTreeSet::new();
                if has_blended_noise(df, &input.registry, &mut visited) {
                    self.blended_noise_refs.insert(name.clone());
                }
            }
        }

        self.topo_order = self.topological_sort(input);
    }

    fn walk_df(&mut self, df: &DensityFunction, input: &TranspilerInput) {
        match df {
            DensityFunction::Constant(_)
            | DensityFunction::BlendAlpha(_)
            | DensityFunction::BlendOffset(_)
            | DensityFunction::YClampedGradient(_) => {}

            DensityFunction::EndIslands => {
                self.uses_end_islands = true;
            }

            DensityFunction::Noise(n) => {
                self.noise_ids.insert(n.noise_id.clone());
            }
            DensityFunction::ShiftedNoise(sn) => {
                self.walk_df(&sn.shift_x, input);
                self.walk_df(&sn.shift_y, input);
                self.walk_df(&sn.shift_z, input);
                self.noise_ids.insert(sn.noise_id.clone());
            }
            DensityFunction::ShiftA(s) => {
                self.noise_ids.insert(s.noise_id.clone());
            }
            DensityFunction::ShiftB(s) => {
                self.noise_ids.insert(s.noise_id.clone());
            }
            DensityFunction::Shift(s) => {
                self.noise_ids.insert(s.noise_id.clone());
            }
            DensityFunction::TwoArgumentSimple(t) => {
                self.walk_df(&t.argument1, input);
                self.walk_df(&t.argument2, input);
            }
            DensityFunction::Mapped(m) => self.walk_df(&m.input, input),
            DensityFunction::Clamp(c) => self.walk_df(&c.input, input),
            DensityFunction::RangeChoice(rc) => {
                self.walk_df(&rc.input, input);
                self.walk_df(&rc.when_in_range, input);
                self.walk_df(&rc.when_out_of_range, input);
            }
            DensityFunction::IntervalSelect(interval) => {
                self.walk_df(&interval.input, input);
                for function in &interval.functions {
                    self.walk_df(function, input);
                }
            }
            DensityFunction::Spline(s) => self.walk_spline(&s.spline, input),
            DensityFunction::BlendedNoise(bn) => {
                self.blended_noise_config = Some(bn.clone());
            }
            DensityFunction::WeirdScaledSampler(ws) => {
                self.walk_df(&ws.input, input);
                self.noise_ids.insert(ws.noise_id.clone());
            }
            DensityFunction::BlendDensity(bd) => self.walk_df(&bd.input, input),
            DensityFunction::Marker(m) => self.walk_df(&m.wrapped, input),
            DensityFunction::FindTopSurface(fts) => {
                self.walk_df(&fts.density, input);
                self.walk_df(&fts.upper_bound, input);
            }
            DensityFunction::Reference(r) => {
                if !self.used_names.contains(&r.id) {
                    self.used_names.insert(r.id.clone());
                    if let Some(ref_df) = input.registry.get(&r.id) {
                        self.walk_df(ref_df, input);
                    }
                }
            }
        }
    }

    fn walk_spline(&mut self, spline: &CubicSpline, input: &TranspilerInput) {
        self.walk_df(&spline.coordinate, input);
        for point in &spline.points {
            if let SplineValue::Spline(nested) = &point.value {
                self.walk_spline(nested, input);
            }
        }
    }

    fn topological_sort(&self, input: &TranspilerInput) -> Vec<String> {
        let mut visited = BTreeSet::new();
        let mut order = Vec::new();
        for name in &self.used_names {
            self.topo_visit(name, input, &mut visited, &mut order);
        }
        order
    }

    fn topo_visit(
        &self,
        name: &str,
        input: &TranspilerInput,
        visited: &mut BTreeSet<String>,
        order: &mut Vec<String>,
    ) {
        if visited.contains(name) {
            return;
        }
        visited.insert(name.to_string());
        if let Some(df) = input.registry.get(name) {
            for dep in collect_references(df) {
                if self.used_names.contains(&dep) {
                    self.topo_visit(&dep, input, visited, order);
                }
            }
        }
        order.push(name.to_string());
    }

    // ── Phase 2: Code generation ────────────────────────────────────────

    fn gen_noises_struct(&self) -> TokenStream {
        let fields: Vec<TokenStream> = self
            .noise_ids
            .iter()
            .map(|id| {
                let field = noise_field_ident(id);
                quote! { pub #field: NormalNoise }
            })
            .collect();

        let blended_field = self.blended_noise_config.as_ref().map(|_| {
            quote! { pub blended_noise: steel_worldgen::noise::BlendedNoise, }
        });

        let end_islands_field = if self.uses_end_islands {
            Some(quote! { pub end_islands: steel_worldgen::noise::EndIslands, })
        } else {
            None
        };

        let noises = &self.noises_ident;
        quote! {
            /// All noise generators needed by this dimension's density functions.
            ///
            /// Created at runtime from a seed via the `create` method.
            pub struct #noises {
                #(#fields,)*
                #blended_field
                #end_islands_field
            }
        }
    }

    fn gen_noises_impl(&self) -> TokenStream {
        let legacy = self.legacy_random_source;
        let field_inits: Vec<TokenStream> = self
            .noise_ids
            .iter()
            .map(|id| {
                let field = noise_field_ident(id);
                let id_lit = Literal::string(id);

                // Vanilla's RandomState intercepts temperature/vegetation noise creation
                // when useLegacyRandomSource=true: uses createLegacyNetherBiome with
                // hardcoded params (-7, [1.0, 1.0]) and direct LegacyRandom(seed+offset).
                if legacy && id == "minecraft:temperature" {
                    quote! {
                        #field: {
                            let mut rng = steel_worldgen::random::RandomSource::Legacy(
                                steel_worldgen::random::legacy_random::LegacyRandom::from_seed(seed)
                            );
                            NormalNoise::create_legacy_nether_biome(&mut rng, -7, &[1.0, 1.0])
                        }
                    }
                } else if legacy && id == "minecraft:vegetation" {
                    quote! {
                        #field: {
                            let mut rng = steel_worldgen::random::RandomSource::Legacy(
                                steel_worldgen::random::legacy_random::LegacyRandom::from_seed(seed.wrapping_add(1))
                            );
                            NormalNoise::create_legacy_nether_biome(&mut rng, -7, &[1.0, 1.0])
                        }
                    }
                } else {
                    quote! {
                        #field: {
                            let p = params.get(#id_lit).expect(concat!("missing noise params: ", #id_lit));
                            NormalNoise::create(splitter, #id_lit, p.first_octave, &p.amplitudes)
                        }
                    }
                }
            })
            .collect();

        let blended_init = self.blended_noise_config.as_ref().map(|bn| {
            let xz_scale = Literal::f64_unsuffixed(bn.xz_scale);
            let y_scale = Literal::f64_unsuffixed(bn.y_scale);
            let xz_factor = Literal::f64_unsuffixed(bn.xz_factor);
            let y_factor = Literal::f64_unsuffixed(bn.y_factor);
            let smear = Literal::f64_unsuffixed(bn.smear_scale_multiplier);

            if legacy {
                // Vanilla's RandomState uses LegacyRandom(seed) directly for BlendedNoise
                // instead of splitter.fromHashOf("minecraft:terrain").
                quote! {
                    blended_noise: {
                        let mut rng = steel_worldgen::random::RandomSource::Legacy(
                            steel_worldgen::random::legacy_random::LegacyRandom::from_seed(seed)
                        );
                        steel_worldgen::noise::BlendedNoise::new(
                            &mut rng,
                            #xz_scale, #y_scale, #xz_factor, #y_factor, #smear,
                        )
                    },
                }
            } else {
                quote! {
                    blended_noise: {
                        use steel_worldgen::random::PositionalRandom;
                        use steel_worldgen::random::name_hash::NameHash;
                        const TERRAIN_HASH: NameHash = NameHash::new("minecraft:terrain");
                        let mut terrain_random = splitter.with_hash_of(&TERRAIN_HASH);
                        steel_worldgen::noise::BlendedNoise::new(
                            &mut terrain_random,
                            #xz_scale, #y_scale, #xz_factor, #y_factor, #smear,
                        )
                    },
                }
            }
        });

        let end_islands_init = if self.uses_end_islands {
            Some(quote! {
                end_islands: steel_worldgen::noise::EndIslands::new(seed),
            })
        } else {
            None
        };

        let noises = &self.noises_ident;
        quote! {
            impl #noises {
                /// Create all noise generators from a world seed, positional splitter, and noise parameters.
                pub fn create(
                    seed: u64,
                    splitter: &RandomSplitter,
                    params: &rustc_hash::FxHashMap<String, steel_worldgen::density::NoiseParameters>,
                ) -> Self {
                    let _ = seed; // Suppress unused warning when EndIslands is not used
                    Self {
                        #(#field_inits,)*
                        #blended_init
                        #end_islands_init
                    }
                }
            }
        }
    }

    /// Generate the column cache struct with pre-computed grid support.
    ///
    /// Matches vanilla's `NoiseChunk.FlatCache`: when `init_grid()` is called,
    /// all flat-cached values are pre-computed for the chunk's quart grid.
    /// `ensure()` then does O(1) grid lookups for in-bounds positions and
    /// falls back to on-the-fly computation for out-of-bounds positions.
    #[expect(clippy::too_many_lines, reason = "splitting would hurt readability")]
    fn gen_column_cache(&mut self, input: &TranspilerInput) -> TokenStream {
        // Grid dimensions: (16 / cell_width + 1)² entries, known at compile time.
        let grid_side = 16 / input.cell_width + 1;
        let grid_total = (grid_side * grid_side) as usize;
        let grid_side_lit = Literal::i32_unsuffixed(grid_side);
        let grid_total_lit = Literal::usize_unsuffixed(grid_total);

        let flat_names: Vec<&String> = self
            .topo_order
            .iter()
            .filter(|n| self.flat_cached.contains(*n))
            .collect();

        // Active-value fields (one f64 per flat-cached function)
        let cache_fields: Vec<TokenStream> = flat_names
            .iter()
            .map(|name| {
                let field = named_fn_field_ident(name);
                quote! { pub #field: f64 }
            })
            .collect();

        // Grid storage fields (fixed-size array per flat-cached function)
        let grid_fields: Vec<TokenStream> = flat_names
            .iter()
            .map(|name| {
                let field = grid_field_ident(name);
                quote! { #field: [f64; #grid_total_lit] }
            })
            .collect();

        // Compute statements for ensure() fallback path (same as before)
        let ensure_stmts: Vec<TokenStream> = flat_names
            .iter()
            .map(|name| {
                let field = named_fn_field_ident(name);
                let compute_fn = named_fn_ident(name);
                quote! {
                    let val = #compute_fn(noises, &*self, x, z);
                    self.#field = val;
                }
            })
            .collect();

        // Grid load statements: copy from grid[idx] into active fields
        let grid_load_stmts: Vec<TokenStream> = flat_names
            .iter()
            .map(|name| {
                let active = named_fn_field_ident(name);
                let grid = grid_field_ident(name);
                quote! { self.#active = self.#grid[idx]; }
            })
            .collect();

        // Grid store statements: copy active field into grid[idx]
        let grid_store_stmts: Vec<TokenStream> = flat_names
            .iter()
            .map(|name| {
                let active = named_fn_field_ident(name);
                let grid = grid_field_ident(name);
                quote! { self.#grid[idx] = self.#active; }
            })
            .collect();

        let default_fields: Vec<TokenStream> = flat_names
            .iter()
            .map(|name| {
                let field = named_fn_field_ident(name);
                quote! { #field: 0.0 }
            })
            .collect();

        let grid_default_fields: Vec<TokenStream> = flat_names
            .iter()
            .map(|name| {
                let field = grid_field_ident(name);
                quote! { #field: [0.0; #grid_total_lit] }
            })
            .collect();

        // Flat router entries
        let router_fields: Vec<TokenStream> = self
            .flat_routers
            .iter()
            .map(|name| {
                let field = router_cache_field_ident(name);
                quote! { pub #field: f64 }
            })
            .collect();

        let router_grid_fields: Vec<TokenStream> = self
            .flat_routers
            .iter()
            .map(|name| {
                let field = router_grid_field_ident(name);
                quote! { #field: [f64; #grid_total_lit] }
            })
            .collect();

        let router_ensure_stmts: Vec<TokenStream> = self
            .flat_routers
            .iter()
            .map(|name| {
                let field = router_cache_field_ident(name);
                let compute_fn = router_compute_fn_ident(name);
                quote! {
                    let val = #compute_fn(noises, &*self, x, z);
                    self.#field = val;
                }
            })
            .collect();

        let router_grid_load_stmts: Vec<TokenStream> = self
            .flat_routers
            .iter()
            .map(|name| {
                let active = router_cache_field_ident(name);
                let grid = router_grid_field_ident(name);
                quote! { self.#active = self.#grid[idx]; }
            })
            .collect();

        let router_grid_store_stmts: Vec<TokenStream> = self
            .flat_routers
            .iter()
            .map(|name| {
                let active = router_cache_field_ident(name);
                let grid = router_grid_field_ident(name);
                quote! { self.#grid[idx] = self.#active; }
            })
            .collect();

        let router_default_fields: Vec<TokenStream> = self
            .flat_routers
            .iter()
            .map(|name| {
                let field = router_cache_field_ident(name);
                quote! { #field: 0.0 }
            })
            .collect();

        let router_grid_default_fields: Vec<TokenStream> = self
            .flat_routers
            .iter()
            .map(|name| {
                let field = router_grid_field_ident(name);
                quote! { #field: [0.0; #grid_total_lit] }
            })
            .collect();

        // Inline Y-independent noise cache
        let inline_noise_fields: Vec<TokenStream> = self
            .inline_flat_noises
            .values()
            .map(|(idx, _, _)| {
                let field = format_ident!("inline_noise_{}", idx);
                quote! { pub #field: f64 }
            })
            .collect();

        let inline_noise_grid_fields: Vec<TokenStream> = self
            .inline_flat_noises
            .values()
            .map(|(idx, _, _)| {
                let field = format_ident!("grid_inline_noise_{}", idx);
                quote! { #field: [f64; #grid_total_lit] }
            })
            .collect();

        let inline_noise_ensure_stmts: Vec<TokenStream> = self
            .inline_flat_noises
            .values()
            .map(|(idx, noise_id, xz_scale)| {
                let field = format_ident!("inline_noise_{}", idx);
                let noise_field = noise_field_ident(noise_id);
                let scale = Literal::f64_unsuffixed(*xz_scale);
                quote! {
                    self.#field = noises.#noise_field.get_value_xz(
                        f64::from(x) * #scale, f64::from(z) * #scale,
                    );
                }
            })
            .collect();

        let inline_noise_grid_load_stmts: Vec<TokenStream> = self
            .inline_flat_noises
            .values()
            .map(|(idx, _, _)| {
                let active = format_ident!("inline_noise_{}", idx);
                let grid = format_ident!("grid_inline_noise_{}", idx);
                quote! { self.#active = self.#grid[idx]; }
            })
            .collect();

        let inline_noise_grid_store_stmts: Vec<TokenStream> = self
            .inline_flat_noises
            .values()
            .map(|(idx, _, _)| {
                let active = format_ident!("inline_noise_{}", idx);
                let grid = format_ident!("grid_inline_noise_{}", idx);
                quote! { self.#grid[idx] = self.#active; }
            })
            .collect();

        let inline_noise_default_fields: Vec<TokenStream> = self
            .inline_flat_noises
            .values()
            .map(|(idx, _, _)| {
                let field = format_ident!("inline_noise_{}", idx);
                quote! { #field: 0.0 }
            })
            .collect();

        let inline_noise_grid_default_fields: Vec<TokenStream> = self
            .inline_flat_noises
            .values()
            .map(|(idx, _, _)| {
                let field = format_ident!("grid_inline_noise_{}", idx);
                quote! { #field: [0.0; #grid_total_lit] }
            })
            .collect();

        let noises = &self.noises_ident;
        let cache = &self.cache_ident;
        quote! {
            /// Column-level cache for flat-cached (xz-only) density function results.
            ///
            /// Supports two modes matching vanilla's `NoiseChunk.FlatCache`:
            /// - **Grid mode** (`init_grid()` called): Pre-computes a 2D grid of all
            ///   in-chunk quart positions. `ensure()` does O(1) grid lookups for
            ///   in-bounds positions, falls back to on-the-fly for out-of-bounds.
            /// - **No-grid mode** (default): Single-entry lazy cache that recomputes
            ///   when quart-quantized coordinates change. Used by climate samplers.
            #[derive(Clone)]
            pub struct #cache {
                /// Raw x block coordinate (for non-flat router functions).
                pub x: i32,
                /// Raw z block coordinate (for non-flat router functions).
                pub z: i32,
                /// Effective x used to evaluate flat-cached values.
                qx: i32,
                /// Effective z used to evaluate flat-cached values.
                qz: i32,
                valid: bool,
                // ── Grid backing store ──
                grid_first_quart_x: i32,
                grid_first_quart_z: i32,
                has_grid: bool,
                // Active value fields (read by compute functions)
                #(#cache_fields,)*
                #(#router_fields,)*
                #(#inline_noise_fields,)*
                // Grid arrays (SoA layout, fixed-size per dimension)
                #(#grid_fields,)*
                #(#router_grid_fields,)*
                #(#inline_noise_grid_fields),*
            }

            impl #cache {
                /// Grid side length (quart positions per axis).
                const GRID_SIDE: i32 = #grid_side_lit;

                /// Create a new column cache without a pre-computed grid.
                #[must_use]
                pub fn new() -> Self {
                    Self {
                        x: 0,
                        z: 0,
                        qx: i32::MIN,
                        qz: i32::MIN,
                        valid: false,
                        grid_first_quart_x: 0,
                        grid_first_quart_z: 0,
                        has_grid: false,
                        #(#default_fields,)*
                        #(#router_default_fields,)*
                        #(#inline_noise_default_fields,)*
                        #(#grid_default_fields,)*
                        #(#router_grid_default_fields,)*
                        #(#inline_noise_grid_default_fields),*
                    }
                }

                /// Pre-compute flat-cached values for all quart positions in a chunk.
                ///
                /// After this call, `ensure()` for in-bounds positions copies from
                /// the grid (O(1)). Out-of-bounds positions fall back to on-the-fly
                /// evaluation at raw (non-quantized) coordinates.
                pub fn init_grid(&mut self, chunk_block_x: i32, chunk_block_z: i32,
                                 noises: &#noises) {
                    self.grid_first_quart_x = chunk_block_x >> 2;
                    self.grid_first_quart_z = chunk_block_z >> 2;
                    self.has_grid = true;
                    self.valid = false;

                    // Pre-compute all grid positions in topological order.
                    // For each position, write to active fields first (so
                    // dependent compute functions can read them), then copy
                    // into the grid arrays.
                    for rel_z in 0..Self::GRID_SIDE {
                        for rel_x in 0..Self::GRID_SIDE {
                            let x = (self.grid_first_quart_x + rel_x) << 2;
                            let z = (self.grid_first_quart_z + rel_z) << 2;
                            let idx = (rel_z * Self::GRID_SIDE + rel_x) as usize;

                            #(#ensure_stmts)*
                            #(#grid_store_stmts)*
                            #(#router_ensure_stmts)*
                            #(#router_grid_store_stmts)*
                            #(#inline_noise_ensure_stmts)*
                            #(#inline_noise_grid_store_stmts)*
                        }
                    }
                }

                /// Ensure the cache is populated for the given `(x, z)` block coordinates.
                ///
                /// With a grid: in-bounds positions load from the pre-computed grid,
                /// out-of-bounds positions compute at raw (non-quantized) coordinates.
                /// Without a grid: always quantizes and lazy-computes (single-entry cache).
                pub fn ensure(&mut self, x: i32, z: i32, noises: &#noises) {
                    self.x = x;
                    self.z = z;

                    let quart_x = x >> 2;
                    let quart_z = z >> 2;

                    if self.has_grid {
                        let rel_x = quart_x - self.grid_first_quart_x;
                        let rel_z = quart_z - self.grid_first_quart_z;
                        if rel_x >= 0 && rel_z >= 0
                            && rel_x < Self::GRID_SIDE
                            && rel_z < Self::GRID_SIDE
                        {
                            // In-bounds: load from grid
                            let eval_x = quart_x << 2;
                            let eval_z = quart_z << 2;
                            if self.valid && self.qx == eval_x && self.qz == eval_z {
                                return;
                            }
                            let idx = (rel_z * Self::GRID_SIDE + rel_x) as usize;
                            #(#grid_load_stmts)*
                            #(#router_grid_load_stmts)*
                            #(#inline_noise_grid_load_stmts)*
                            self.qx = eval_x;
                            self.qz = eval_z;
                            self.valid = true;
                            return;
                        }
                        // Out-of-bounds: raw coords, compute on-the-fly
                        if self.valid && self.qx == x && self.qz == z {
                            return;
                        }
                        self.qx = x;
                        self.qz = z;
                        let x = x;
                        let z = z;
                        #(#ensure_stmts)*
                        #(#router_ensure_stmts)*
                        #(#inline_noise_ensure_stmts)*
                        self.valid = true;
                        return;
                    }

                    // No grid: quantize and lazy-compute
                    let eval_x = quart_x << 2;
                    let eval_z = quart_z << 2;
                    if self.valid && self.qx == eval_x && self.qz == eval_z {
                        return;
                    }
                    self.qx = eval_x;
                    self.qz = eval_z;
                    let x = eval_x;
                    let z = eval_z;
                    #(#ensure_stmts)*
                    #(#router_ensure_stmts)*
                    #(#inline_noise_ensure_stmts)*
                    self.valid = true;
                }
            }
        }
    }

    /// Generate the function parameter list for a density function.
    fn fn_params(&self, is_flat: bool) -> TokenStream {
        let noises = &self.noises_ident;
        let cache = &self.cache_ident;
        if is_flat {
            quote! { noises: &#noises, cache: &#cache, x: i32, z: i32 }
        } else {
            quote! { noises: &#noises, cache: &#cache, x: i32, y: i32, z: i32 }
        }
    }

    /// Generate the SIMD (4-Y batched) parameter list for a non-flat density
    /// function. Flat functions don't have a 4x form (callers splat from cache).
    fn fn_params_4x(&self) -> TokenStream {
        let noises = &self.noises_ident;
        let cache = &self.cache_ident;
        quote! { noises: &#noises, cache: &#cache, x: i32, ys: f64x4, z: i32 }
    }

    /// Generate the function parameter list for a router entry point.
    /// Router functions read x/z from the cache, so flat variants omit explicit coords.
    fn fn_params_router(&self, is_flat: bool) -> TokenStream {
        let noises = &self.noises_ident;
        let cache = &self.cache_ident;
        if is_flat {
            quote! { noises: &#noises, cache: &#cache }
        } else {
            quote! { noises: &#noises, cache: &#cache, x: i32, y: i32, z: i32 }
        }
    }

    /// Generate all named density functions.
    fn gen_named_functions(&mut self, input: &TranspilerInput) -> TokenStream {
        let mut fns = Vec::new();

        for name in self.topo_order.clone() {
            let Some(df) = input.registry.get(&name) else {
                continue;
            };
            let inner = unwrap_markers(df).clone();
            let fn_name = named_fn_ident(&name);
            let is_flat = self.flat_cached.contains(&name);

            let body = self.gen_expr(&inner, input, is_flat);

            let params = self.fn_params(is_flat);

            let doc = Literal::string(&format!("`{name}`"));
            fns.push(quote! {
                #[doc = #doc]
                #[inline]
                fn #fn_name(#params) -> f64 {
                    #body
                }
            });
        }

        let spline_fns = mem::take(&mut self.spline_fns);

        // SIMD (4-Y batched) parallel compute functions for non-flat named
        // functions. Flat functions splat from the column cache, so they don't
        // need a 4x form. Some non-flat functions may only be reachable from
        // scalar paths (non-fill routers); the `dead_code` allow keeps those
        // cases warning-free.
        let mut fns_4x = Vec::new();
        for name in self.topo_order.clone() {
            if self.flat_cached.contains(&name) {
                continue;
            }
            let Some(df) = input.registry.get(&name) else {
                continue;
            };
            let inner = unwrap_markers(df).clone();
            let fn_name_4x = named_fn_ident_4x(&name);

            let body = self.gen_expr_simd(&inner, input, false);

            let params = self.fn_params_4x();

            let doc = Literal::string(&format!("`{name}` (SIMD form, batches 4 Y values)"));
            fns_4x.push(quote! {
                #[doc = #doc]
                #[allow(dead_code)]
                #[inline]
                fn #fn_name_4x(#params) -> f64x4 {
                    #body
                }
            });
        }

        let spline_fns_4x = mem::take(&mut self.spline_fns);

        quote! {
            #(#fns)*
            #(#spline_fns)*
            #(#fns_4x)*
            #(#spline_fns_4x)*
        }
    }

    /// Generate the router entry point functions.
    ///
    /// Flat (Y-independent) routers read their result from the column cache.
    /// A private `compute_router_*` function is also emitted for each flat
    /// router, called by `ensure()` to populate the cache.
    fn gen_router_functions(&mut self, input: &TranspilerInput) -> TokenStream {
        let mut fns = Vec::new();

        for (name, df) in &input.router_entries {
            let fn_name = format_ident!("router_{}", sanitize_name(name));
            let doc = Literal::string(&format!("Noise router entry: `{name}`"));

            if self.flat_routers.contains(name) {
                // Flat router: result is cached in the column cache.
                // Generate private compute function used by ensure().
                let compute_fn_name = router_compute_fn_ident(name);
                let inner = unwrap_markers(df);
                let compute_body = self.gen_expr(inner, input, true);
                let compute_params = self.fn_params(true);

                fns.push(quote! {
                    #[inline]
                    fn #compute_fn_name(#compute_params) -> f64 {
                        #compute_body
                    }
                });

                // Public router function returns the cached value.
                // Keeps the full (noises, cache, x, y, z) signature for API consistency.
                let cache_field = router_cache_field_ident(name);
                let full_params = self.fn_params_router(false);
                fns.push(quote! {
                    #[doc = #doc]
                    #[inline]
                    pub fn #fn_name(#full_params) -> f64 {
                        cache.#cache_field
                    }
                });
            } else {
                let inner = unwrap_markers(df);
                let is_flat = is_flat_cached(df);
                let body = self.gen_expr(inner, input, is_flat);
                let params = self.fn_params_router(is_flat);

                fns.push(quote! {
                    #[doc = #doc]
                    #[inline]
                    pub fn #fn_name(#params) -> f64 {
                        let x = cache.x;
                        let z = cache.z;
                        #body
                    }
                });
            }
        }

        // Generate interpolation functions for all router entries with Interpolated markers.
        let interp_fns = self.gen_all_interpolation_functions(input);
        fns.push(interp_fns);

        let spline_fns = mem::take(&mut self.spline_fns);
        quote! {
            #(#fns)*
            #(#spline_fns)*
        }
    }

    /// Generate interpolation functions for ALL router entries that contain
    /// `Interpolated` markers: `fill_cell_corner_densities`, `combine_interpolated`,
    /// and per-entry combine functions for `vein_toggle`/`vein_ridged`.
    ///
    /// All entries share a single contiguous channel array. Channel indices are
    /// assigned in order: `final_density` channels first, then `vein_toggle`, then
    /// `vein_ridged`.
    #[expect(clippy::too_many_lines, reason = "splitting would hurt readability")]
    fn gen_all_interpolation_functions(&mut self, input: &TranspilerInput) -> TokenStream {
        let noises = self.noises_ident.clone();
        let cache = self.cache_ident.clone();

        // Entries that may contain Interpolated markers.
        // Order matters: final_density first, then vein functions.
        let entry_names = ["final_density", "vein_toggle", "vein_ridged"];

        // Phase 1: Collect ALL interpolated inners across all entries
        #[expect(
            clippy::items_after_statements,
            reason = "struct is local to this code path and defined inline for clarity"
        )]
        struct EntryInfo {
            start: usize,
            df: DensityFunction,
        }
        let mut all_inners: Vec<DensityFunction> = Vec::new();
        let mut entries: BTreeMap<String, EntryInfo> = BTreeMap::new();

        for name in entry_names {
            if let Some(df) = input.router_entries.get(name) {
                let start = all_inners.len();
                let inners = collect_interpolated_inners(df, &input.registry);
                if !inners.is_empty() {
                    all_inners.extend(inners);
                    entries.insert(
                        name.to_owned(),
                        EntryInfo {
                            start,
                            df: df.clone(),
                        },
                    );
                }
            }
        }

        let total_count = all_inners.len();
        let total_count_lit = Literal::usize_unsuffixed(total_count);

        // Phase 2: Generate fill_cell_corner_densities with ALL channels
        self.fill_mode = true;
        let mut inner_stmts = Vec::with_capacity(total_count);
        for (i, inner_df) in all_inners.iter().enumerate() {
            let idx = Literal::usize_unsuffixed(i);
            let inner = unwrap_markers(inner_df);
            let expr = self.gen_expr(inner, input, false);
            inner_stmts.push(quote! { out[#idx] = #expr; });
        }
        self.fill_mode = false;
        let fill_spline_fns = mem::take(&mut self.spline_fns);

        // Phase 2b: Generate fill_cell_corner_densities_4x — SIMD form that
        // batches 4 cell-corner Y values per call. Output layout is lane-major:
        // `out[lane * INTERPOLATED_COUNT + ch] = lane_ch_value`. This pairs
        // with `noise_chunk::fill_slice`'s 4-batched corner loop.
        self.fill_mode = true;
        let mut inner_stmts_4x = Vec::with_capacity(total_count);
        for (i, inner_df) in all_inners.iter().enumerate() {
            let idx = Literal::usize_unsuffixed(i);
            let inner = unwrap_markers(inner_df);
            let expr_simd = self.gen_expr_simd(inner, input, false);
            inner_stmts_4x.push(quote! {
                {
                    let __r = #expr_simd;
                    out[#idx] = __r[0];
                    out[#idx + INTERPOLATED_COUNT] = __r[1];
                    out[#idx + 2 * INTERPOLATED_COUNT] = __r[2];
                    out[#idx + 3 * INTERPOLATED_COUNT] = __r[3];
                }
            });
        }
        self.fill_mode = false;
        let fill_spline_fns_4x = mem::take(&mut self.spline_fns);

        // Phase 3: Generate combine_interpolated for final_density
        let combine_fd_body = if let Some(info) = entries.get("final_density") {
            self.interpolated_param_mode = true;
            self.interpolated_param_counter = info.start;
            let body = self.gen_expr(&info.df, input, false);
            self.interpolated_param_mode = false;
            body
        } else {
            quote! { 0.0 }
        };
        let combine_fd_splines = mem::take(&mut self.spline_fns);

        // Phase 4: Generate combine functions for vein entries
        let combine_vein_toggle_body = if let Some(info) = entries.get("vein_toggle") {
            self.interpolated_param_mode = true;
            self.interpolated_param_counter = info.start;
            let body = self.gen_expr(&info.df, input, false);
            self.interpolated_param_mode = false;
            body
        } else {
            // No interpolated markers in vein_toggle — fall back to direct eval
            quote! { 0.0 }
        };
        let combine_vein_toggle_splines = mem::take(&mut self.spline_fns);

        let combine_vein_ridged_body = if let Some(info) = entries.get("vein_ridged") {
            self.interpolated_param_mode = true;
            self.interpolated_param_counter = info.start;
            let body = self.gen_expr(&info.df, input, false);
            self.interpolated_param_mode = false;
            body
        } else {
            quote! { 0.0 }
        };
        let combine_vein_ridged_splines = mem::take(&mut self.spline_fns);

        // Determine whether vein interpolation is present
        let has_vein_interp =
            entries.contains_key("vein_toggle") || entries.contains_key("vein_ridged");
        let has_vein_interp_tok: TokenStream = if has_vein_interp {
            quote! { true }
        } else {
            quote! { false }
        };

        quote! {
            /// Total number of independently interpolated channels across all
            /// router entries (final_density + vein_toggle + vein_ridged).
            pub const INTERPOLATED_COUNT: usize = #total_count_lit;

            /// Whether vein functions have interpolation channels.
            pub const VEIN_INTERP_ENABLED: bool = #has_vein_interp_tok;

            /// Evaluate the inner functions of all `Interpolated` markers at a cell corner.
            ///
            /// `out` must have length `INTERPOLATED_COUNT`.
            #[expect(unused_variables, reason = "generated function has a fixed signature; blended_noise_value is unused in dimensions without blended noise")]
            pub fn fill_cell_corner_densities(
                noises: &#noises,
                cache: &#cache,
                x: i32,
                y: i32,
                z: i32,
                blended_noise_value: f64,
                out: &mut [f64],
            ) {
                let x = cache.x;
                let z = cache.z;
                #(#inner_stmts)*
            }

            /// SIMD form of [`fill_cell_corner_densities`] that batches 4
            /// cell-corner Y values at fixed `(x, z)`.
            ///
            /// `out` layout: lane-major SoA. Lane `i`'s `INTERPOLATED_COUNT`
            /// channels live at `out[i * INTERPOLATED_COUNT..(i + 1) * INTERPOLATED_COUNT]`.
            /// `out` must have length `4 * INTERPOLATED_COUNT`.
            ///
            /// Per-lane semantics are bit-identical to four scalar
            /// [`fill_cell_corner_densities`] calls at the same Y values.
            #[expect(unused_variables, reason = "generated function has a fixed signature; not all dimensions use every parameter")]
            pub fn fill_cell_corner_densities_4x(
                noises: &#noises,
                cache: &#cache,
                x: i32,
                ys: f64x4,
                z: i32,
                blended_noise_value_v: f64x4,
                out: &mut [f64],
            ) {
                let x = cache.x;
                let z = cache.z;
                #(#inner_stmts_4x)*
            }

            /// Combine interpolated values for `final_density`.
            #[expect(unused_variables, reason = "generated function has a fixed signature; not all parameters are used in every dimension")]
            pub fn combine_interpolated(
                noises: &#noises,
                cache: &#cache,
                interpolated: &[f64],
                _x: i32,
                y: i32,
                _z: i32,
            ) -> f64 {
                let x = cache.x;
                let z = cache.z;
                #combine_fd_body
            }

            /// Combine interpolated values for `vein_toggle`.
            #[expect(unused_variables, reason = "generated function has a fixed signature; not all parameters are used in every dimension")]
            pub fn combine_vein_toggle(
                noises: &#noises,
                cache: &#cache,
                interpolated: &[f64],
                _x: i32,
                y: i32,
                _z: i32,
            ) -> f64 {
                let x = cache.x;
                let z = cache.z;
                #combine_vein_toggle_body
            }

            /// Combine interpolated values for `vein_ridged`.
            #[expect(unused_variables, reason = "generated function has a fixed signature; not all parameters are used in every dimension")]
            pub fn combine_vein_ridged(
                noises: &#noises,
                cache: &#cache,
                interpolated: &[f64],
                _x: i32,
                y: i32,
                _z: i32,
            ) -> f64 {
                let x = cache.x;
                let z = cache.z;
                #combine_vein_ridged_body
            }

            #(#fill_spline_fns)*
            #(#fill_spline_fns_4x)*
            #(#combine_fd_splines)*
            #(#combine_vein_toggle_splines)*
            #(#combine_vein_ridged_splines)*
        }
    }

    // ── Expression generation ───────────────────────────────────────────

    /// Generate a `TokenStream` expression that computes a density function value.
    ///
    /// `is_flat` indicates this expression tree is xz-only (no y available).
    #[expect(clippy::too_many_lines, reason = "splitting would hurt readability")]
    fn gen_expr(
        &mut self,
        df: &DensityFunction,
        input: &TranspilerInput,
        is_flat: bool,
    ) -> TokenStream {
        // Unified CSE: if this node was hoisted by an enclosing scope, emit
        // the variable instead of recomputing.
        if is_cse_candidate(df) {
            let fp = fingerprint(df);
            if let Some(var) = self.cse_bindings.get(&fp) {
                return quote! { #var };
            }
        }

        match df {
            DensityFunction::Constant(c) => {
                let val = Literal::f64_unsuffixed(c.value);
                quote! { #val }
            }

            DensityFunction::YClampedGradient(g) => {
                let from_y = Literal::f64_unsuffixed(f64::from(g.from_y));
                let to_y = Literal::f64_unsuffixed(f64::from(g.to_y));
                let from_val = Literal::f64_unsuffixed(g.from_value);
                let to_val = Literal::f64_unsuffixed(g.to_value);
                quote! { map_clamped(f64::from(y), #from_y, #to_y, #from_val, #to_val) }
            }

            DensityFunction::Noise(n) => {
                // Y-independent noise inside a 3D function: read from column cache
                if !is_flat && n.y_scale == 0.0 {
                    let fp = fingerprint(df);
                    if let Some((idx, _, _)) = self.inline_flat_noises.get(&fp) {
                        let cache_field = format_ident!("inline_noise_{}", idx);
                        return quote! { cache.#cache_field };
                    }
                }
                let field = noise_field_ident(&n.noise_id);
                let xz_scale = Literal::f64_unsuffixed(n.xz_scale);
                let y_scale = Literal::f64_unsuffixed(n.y_scale);
                if is_flat || n.y_scale == 0.0 {
                    quote! { noises.#field.get_value_xz(f64::from(x) * #xz_scale, f64::from(z) * #xz_scale) }
                } else {
                    quote! { noises.#field.get_value(f64::from(x) * #xz_scale, f64::from(y) * #y_scale, f64::from(z) * #xz_scale) }
                }
            }

            DensityFunction::ShiftedNoise(sn) => {
                let dx = self.gen_expr(&sn.shift_x, input, is_flat);
                let dy = self.gen_expr(&sn.shift_y, input, is_flat);
                let dz = self.gen_expr(&sn.shift_z, input, is_flat);
                let field = noise_field_ident(&sn.noise_id);
                let xz_scale = Literal::f64_unsuffixed(sn.xz_scale);
                let y_scale = Literal::f64_unsuffixed(sn.y_scale);
                // Vanilla formula: x * xz_scale + dx (multiply THEN add shift)
                if is_flat || sn.y_scale == 0.0 {
                    quote! {{
                        let dx = #dx;
                        let dz = #dz;
                        noises.#field.get_value_xz(
                            f64::from(x) * #xz_scale + dx,
                            f64::from(z) * #xz_scale + dz,
                        )
                    }}
                } else {
                    quote! {{
                        let dx = #dx;
                        let dy = #dy;
                        let dz = #dz;
                        noises.#field.get_value(
                            f64::from(x) * #xz_scale + dx,
                            f64::from(y) * #y_scale + dy,
                            f64::from(z) * #xz_scale + dz,
                        )
                    }}
                }
            }

            DensityFunction::ShiftA(s) => {
                let field = noise_field_ident(&s.noise_id);
                quote! { noises.#field.get_value_xz(f64::from(x) * 0.25, f64::from(z) * 0.25) * 4.0 }
            }

            DensityFunction::ShiftB(s) => {
                let field = noise_field_ident(&s.noise_id);
                quote! { noises.#field.get_value_xy(f64::from(z) * 0.25, f64::from(x) * 0.25) * 4.0 }
            }

            DensityFunction::Shift(s) => {
                let field = noise_field_ident(&s.noise_id);
                if is_flat {
                    quote! { noises.#field.get_value_xz(f64::from(x) * 0.25, f64::from(z) * 0.25) * 4.0 }
                } else {
                    quote! { noises.#field.get_value(f64::from(x) * 0.25, f64::from(y) * 0.25, f64::from(z) * 0.25) * 4.0 }
                }
            }

            DensityFunction::TwoArgumentSimple(t) => {
                let (hoisted, hoisted_fps) =
                    self.hoist_common_subexprs(&[&t.argument1, &t.argument2], input, is_flat);

                let a = self.gen_expr(&t.argument1, input, is_flat);
                let b = self.gen_expr(&t.argument2, input, is_flat);

                for fp in &hoisted_fps {
                    self.cse_bindings.remove(fp);
                }

                // For min/max, compute a static bound on the right operand and
                // emit a short-circuit when the left already proves the result
                // (saves evaluating the right subtree on the lucky path).
                // Mirrors C2ME's `MaxShortNode`/`MinShortNode` rewriters.
                //
                // Inner min/max emitted as `if a < b { a } else { b }` (and `>`
                // for max), not `f64::min`/`f64::max`. The stdlib calls lower to
                // an IEEE-minNum intrinsic with explicit NaN handling (~5 x86
                // insns); the comparison form lowers to a single `vminsd`/cmov.
                // Density functions never produce NaN in vanilla parameter
                // ranges (verified by `chunk_stage_hashes`), so the two are
                // bit-identical here.
                let op = match t.op {
                    TwoArgType::Add => quote! { ((#a) + (#b)) },
                    TwoArgType::Mul => quote! { ((#a) * (#b)) },
                    TwoArgType::Min => {
                        let (b_lo, _b_hi) = compute_bounds(&t.argument2, input);
                        if b_lo.is_finite() {
                            // If `a <= b_lo`, then `b >= b_lo >= a`, so `min(a, b) = a`.
                            let b_lo_lit = Literal::f64_unsuffixed(b_lo);
                            quote! {{
                                let __sc_a = #a;
                                if __sc_a <= #b_lo_lit {
                                    __sc_a
                                } else {
                                    let __sc_b = #b;
                                    if __sc_a < __sc_b { __sc_a } else { __sc_b }
                                }
                            }}
                        } else {
                            quote! {{
                                let __sc_a = #a;
                                let __sc_b = #b;
                                if __sc_a < __sc_b { __sc_a } else { __sc_b }
                            }}
                        }
                    }
                    TwoArgType::Max => {
                        let (_b_lo, b_hi) = compute_bounds(&t.argument2, input);
                        if b_hi.is_finite() {
                            // If `a >= b_hi`, then `b <= b_hi <= a`, so `max(a, b) = a`.
                            let b_hi_lit = Literal::f64_unsuffixed(b_hi);
                            quote! {{
                                let __sc_a = #a;
                                if __sc_a >= #b_hi_lit {
                                    __sc_a
                                } else {
                                    let __sc_b = #b;
                                    if __sc_a > __sc_b { __sc_a } else { __sc_b }
                                }
                            }}
                        } else {
                            quote! {{
                                let __sc_a = #a;
                                let __sc_b = #b;
                                if __sc_a > __sc_b { __sc_a } else { __sc_b }
                            }}
                        }
                    }
                };

                if hoisted.is_empty() {
                    op
                } else {
                    quote! {{
                        #(#hoisted)*
                        #op
                    }}
                }
            }

            DensityFunction::Mapped(m) => {
                let v = self.gen_expr(&m.input, input, is_flat);
                match m.op {
                    MappedType::Abs => quote! { (#v).abs() },
                    MappedType::Square => quote! { { let v = #v; v * v } },
                    MappedType::Cube => quote! { { let v = #v; v * v * v } },
                    MappedType::HalfNegative => {
                        quote! { { let v = #v; if v > 0.0 { v } else { v * 0.5 } } }
                    }
                    MappedType::QuarterNegative => {
                        quote! { { let v = #v; if v > 0.0 { v } else { v * 0.25 } } }
                    }
                    MappedType::Invert => quote! { (1.0 / (#v)) },
                    MappedType::Squeeze => {
                        quote! { { let c = clamp(#v, -1.0, 1.0); c / 2.0 - c * c * c / 24.0 } }
                    }
                }
            }

            DensityFunction::Clamp(c) => {
                let inner = self.gen_expr(&c.input, input, is_flat);
                let min = Literal::f64_unsuffixed(c.min);
                let max = Literal::f64_unsuffixed(c.max);
                quote! { clamp(#inner, #min, #max) }
            }

            DensityFunction::RangeChoice(rc) => {
                let min = Literal::f64_unsuffixed(rc.min_inclusive);
                let max = Literal::f64_unsuffixed(rc.max_exclusive);

                // Generate input expression BEFORE registering any CSE
                // bindings (otherwise a self-referencing input produces
                // `let v = v;`).
                let input_expr = self.gen_expr(&rc.input, input, is_flat);

                // CSE: if input is a CSE candidate, register `v` so the same
                // subexpression inside the branches reuses the binding.
                let input_fp = if is_cse_candidate(&rc.input) {
                    let fp = fingerprint(&rc.input);
                    self.cse_bindings.insert(fp, format_ident!("v"));
                    Some(fp)
                } else {
                    None
                };

                // CSE: hoist subexpressions common to both branches.
                let (hoisted, hoisted_fps) = self.hoist_common_subexprs(
                    &[&rc.when_in_range, &rc.when_out_of_range],
                    input,
                    is_flat,
                );

                let in_range = self.gen_expr(&rc.when_in_range, input, is_flat);
                let out_range = self.gen_expr(&rc.when_out_of_range, input, is_flat);

                // Clean up all CSE bindings
                if let Some(ref fp) = input_fp {
                    self.cse_bindings.remove(fp);
                }
                for fp in &hoisted_fps {
                    self.cse_bindings.remove(fp);
                }

                // Drop bound checks proven dead by static input bounds — vanilla
                // RangeChoice often uses sentinel bounds like `-1_000_000` for
                // "unbounded below" that the input's actual range never violates.
                let (in_lo, in_hi) = compute_bounds(&rc.input, input);
                let lower_dead = in_lo >= rc.min_inclusive;
                let upper_dead = in_hi < rc.max_exclusive;

                let cond = match (lower_dead, upper_dead) {
                    (true, true) => quote! { true },
                    (true, false) => quote! { v < #max },
                    (false, true) => quote! { v >= #min },
                    (false, false) => quote! { v >= #min && v < #max },
                };

                quote! {{
                    #(#hoisted)*
                    let v = #input_expr;
                    if #cond { #in_range } else { #out_range }
                }}
            }

            DensityFunction::IntervalSelect(interval) => {
                let input_expr = self.gen_expr(&interval.input, input, is_flat);

                let input_fp = if is_cse_candidate(&interval.input) {
                    let fp = fingerprint(&interval.input);
                    self.cse_bindings.insert(fp, format_ident!("v"));
                    Some(fp)
                } else {
                    None
                };

                let branches: Vec<_> = interval.functions.iter().collect();
                let (hoisted, hoisted_fps) = self.hoist_common_subexprs(&branches, input, is_flat);

                let function_exprs: Vec<_> = interval
                    .functions
                    .iter()
                    .map(|function| self.gen_expr(function, input, is_flat))
                    .collect();

                if let Some(ref fp) = input_fp {
                    self.cse_bindings.remove(fp);
                }
                for fp in &hoisted_fps {
                    self.cse_bindings.remove(fp);
                }

                let Some((last_expr, earlier_exprs)) = function_exprs.split_last() else {
                    panic!("minecraft:interval_select requires at least one function");
                };
                let mut branch_expr = quote! { #last_expr };
                for (threshold, function_expr) in
                    interval.thresholds.iter().zip(earlier_exprs.iter()).rev()
                {
                    let threshold = Literal::f64_unsuffixed(*threshold);
                    let else_expr = branch_expr;
                    branch_expr = quote! {
                        if v < #threshold { #function_expr } else { #else_expr }
                    };
                }

                quote! {{
                    #(#hoisted)*
                    let v = #input_expr;
                    #branch_expr
                }}
            }

            DensityFunction::Spline(s) => self.gen_spline_expr(&s.spline, input, is_flat),

            DensityFunction::BlendedNoise(_) => {
                if self.fill_mode {
                    quote! { blended_noise_value }
                } else {
                    quote! { noises.blended_noise.compute(x, y, z) }
                }
            }

            DensityFunction::WeirdScaledSampler(ws) => {
                let input_expr = self.gen_expr(&ws.input, input, is_flat);
                let field = noise_field_ident(&ws.noise_id);
                let mapper = match ws.rarity_value_mapper {
                    RarityValueMapper::Tunnels => quote! { RarityValueMapper::Tunnels },
                    RarityValueMapper::Caves => quote! { RarityValueMapper::Caves },
                };
                quote! {{
                    let rarity = #input_expr;
                    let scale = #mapper.get_values(rarity);
                    scale * noises.#field.get_value(
                        f64::from(x) / scale, f64::from(y) / scale, f64::from(z) / scale,
                    ).abs()
                }}
            }

            DensityFunction::BlendAlpha(_) => quote! { 1.0 },
            DensityFunction::BlendOffset(_) => quote! { 0.0 },
            // EndIslands ignores y internally, so we can pass 0 in flat contexts
            DensityFunction::EndIslands => {
                if is_flat {
                    quote! { noises.end_islands.sample(x, 0, z) }
                } else {
                    quote! { noises.end_islands.sample(x, y, z) }
                }
            }
            DensityFunction::BlendDensity(bd) => self.gen_expr(&bd.input, input, is_flat),
            DensityFunction::Marker(m) => {
                if self.interpolated_param_mode && m.kind == MarkerType::Interpolated {
                    let idx = Literal::usize_unsuffixed(self.interpolated_param_counter);
                    self.interpolated_param_counter += 1;
                    quote! { interpolated[#idx] }
                } else {
                    self.gen_expr(&m.wrapped, input, is_flat)
                }
            }

            DensityFunction::FindTopSurface(fts) => {
                // upper_bound is flat (xz-only)
                let upper_expr = self.gen_expr(&fts.upper_bound, input, is_flat);
                // density uses y — generate with is_flat=false so it references our loop var
                let density_expr = self.gen_expr(&fts.density, input, false);
                let cell_height = Literal::i32_unsuffixed(fts.cell_height);
                let lower_bound = Literal::i32_unsuffixed(fts.lower_bound);
                quote! {{
                    let __upper = #upper_expr;
                    let __top_y = ((__upper / f64::from(#cell_height)).floor() as i32) * #cell_height;
                    if __top_y <= #lower_bound {
                        f64::from(#lower_bound)
                    } else {
                        let mut __result = f64::from(#lower_bound);
                        let mut y = __top_y;
                        while y >= #lower_bound {
                            let __d = #density_expr;
                            if __d > 0.0 {
                                __result = f64::from(y);
                                break;
                            }
                            y -= #cell_height;
                        }
                        __result
                    }
                }}
            }

            DensityFunction::Reference(r) => {
                // Note: the unified CSE check at the top of gen_expr handles
                // Reference nodes too, so we only reach here if there's no
                // active CSE binding for this reference.
                if self.interpolated_param_mode && self.interpolated_refs.contains(&r.id) {
                    // In param mode, inline references that contain Interpolated markers
                    // so that the markers within are replaced with interpolated[i].
                    if let Some(ref_df) = input.registry.get(&r.id) {
                        self.gen_expr(ref_df, input, is_flat)
                    } else {
                        quote! { 0.0 }
                    }
                } else if self.fill_mode && self.blended_noise_refs.contains(&r.id) {
                    // In fill mode, inline references that contain BlendedNoise
                    // so the precomputed blended_noise_value is used.
                    if let Some(ref_df) = input.registry.get(&r.id) {
                        self.gen_expr(ref_df, input, is_flat)
                    } else {
                        quote! { 0.0 }
                    }
                } else if self.flat_cached.contains(&r.id) {
                    // Flat-cached references are always read from the column cache
                    let field = named_fn_field_ident(&r.id);
                    quote! { cache.#field }
                } else {
                    // 3D named function — call it
                    let fn_name = named_fn_ident(&r.id);
                    quote! { #fn_name(noises, cache, x, y, z) }
                }
            }
        }
    }

    /// Generate a spline evaluation expression.
    /// Generate a spline expression as an inlined `if`/`else` chain over the
    /// piecewise intervals — no closure indirection, no binary search, and only
    /// the spline points the chosen interval needs are evaluated. Mirrors C2ME's
    /// `SplineAstNode` flat codegen.
    ///
    /// Math is bit-identical to [`spline_eval::evaluate_spline`] so vanilla
    /// determinism is preserved (same operation order, same intermediate types).
    fn gen_spline_expr(
        &mut self,
        spline: &CubicSpline,
        input: &TranspilerInput,
        is_flat: bool,
    ) -> TokenStream {
        let coord = self.gen_expr(&spline.coordinate, input, is_flat);
        let n_points = spline.points.len();

        // Compute each point's value expression once (could be a constant or a
        // nested spline helper call). We only emit the value expression in the
        // arms that actually need it — adjacent intervals share a point's value
        // via a `let` binding.
        let value_exprs: Vec<TokenStream> = spline
            .points
            .iter()
            .map(|p| match &p.value {
                SplineValue::Constant(c) => {
                    let lit = Literal::f32_unsuffixed(*c);
                    quote! { #lit }
                }
                SplineValue::Spline(nested) => {
                    let helper = self.gen_spline_helper(nested, input, is_flat);
                    if is_flat {
                        quote! { #helper(noises, cache, x, z) }
                    } else {
                        quote! { #helper(noises, cache, x, y, z) }
                    }
                }
            })
            .collect();

        // Empty spline: vanilla returns 0.0.
        if n_points == 0 {
            return quote! {{ let _ = (#coord) as f32; 0.0_f64 }};
        }

        // Single-point spline: degenerate — just return value + derivative * (coord - loc),
        // matching `evaluate_spline`'s extrapolation arms.
        if n_points == 1 {
            let loc = Literal::f32_unsuffixed(spline.points[0].location);
            let der = Literal::f32_unsuffixed(spline.points[0].derivative);
            let v = &value_exprs[0];
            return quote! {{
                let __coord = (#coord) as f32;
                f64::from(#v + #der * (__coord - #loc))
            }};
        }

        // ≥ 2 points: chain of mutually-exclusive intervals.
        //   coord < L_0                    → extrapolate before
        //   L_i ≤ coord < L_{i+1}          → hermite interp on [i, i+1)
        //   coord ≥ L_{last}               → extrapolate after
        let last = n_points - 1;
        let l0 = Literal::f32_unsuffixed(spline.points[0].location);
        let d0 = Literal::f32_unsuffixed(spline.points[0].derivative);
        let l_last = Literal::f32_unsuffixed(spline.points[last].location);
        let d_last = Literal::f32_unsuffixed(spline.points[last].derivative);
        let v0 = &value_exprs[0];
        let v_last = &value_exprs[last];

        // Build interval arms in the order: extrapolate-before, [0,1), [1,2), ..., extrapolate-after.
        let mut arms: Vec<TokenStream> = Vec::new();
        // Extrapolate-before: coord < L_0
        arms.push(quote! {
            if __coord < #l0 {
                f64::from(#v0 + #d0 * (__coord - #l0))
            }
        });
        for i in 0..last {
            let li = Literal::f32_unsuffixed(spline.points[i].location);
            let li1 = Literal::f32_unsuffixed(spline.points[i + 1].location);
            let di = Literal::f32_unsuffixed(spline.points[i].derivative);
            let di1 = Literal::f32_unsuffixed(spline.points[i + 1].derivative);
            let vi = &value_exprs[i];
            let vi1 = &value_exprs[i + 1];
            // Hermite cubic, op-order matching `spline_eval::hermite_interpolate`
            // exactly so generated code is bit-identical.
            arms.push(quote! {
                else if __coord < #li1 {
                    let __y1 = #vi;
                    let __y2 = #vi1;
                    let __t = (__coord - #li) / (#li1 - #li);
                    let __h = #li1 - #li;
                    let __a = #di * __h - (__y2 - __y1);
                    let __b = -#di1 * __h + (__y2 - __y1);
                    let __lerp_y = __y1 + __t * (__y2 - __y1);
                    let __lerp_ab = __a + __t * (__b - __a);
                    f64::from(__lerp_y + __t * (1.0_f32 - __t) * __lerp_ab)
                }
            });
        }
        // Extrapolate-after: coord ≥ L_last
        arms.push(quote! {
            else {
                f64::from(#v_last + #d_last * (__coord - #l_last))
            }
        });

        quote! {{
            let __coord = (#coord) as f32;
            #(#arms)*
        }}
    }

    /// Generate a helper function for a nested spline, returning its ident.
    fn gen_spline_helper(
        &mut self,
        spline: &Arc<CubicSpline>,
        input: &TranspilerInput,
        is_flat: bool,
    ) -> Ident {
        let id = self.spline_counter;
        self.spline_counter += 1;
        let fn_name = format_ident!("spline_helper_{}", id);

        let body = self.gen_spline_expr(spline, input, is_flat);

        let params = self.fn_params(is_flat);

        self.spline_fns.push(quote! {
            #[inline]
            fn #fn_name(#params) -> f32 {
                (#body) as f32
            }
        });

        fn_name
    }

    /// Generate a `TokenStream` expression that computes `df` as `f64x4`
    /// across 4 cell-corner Y values (`ys: f64x4`).
    ///
    /// Variants migrated to true SIMD (`Constant`, `Noise`, `BlendAlpha/Offset`,
    /// `BlendDensity`, `Marker`, `Reference`, `BlendedNoise` in fill mode) emit
    /// per-lane SIMD ops directly. Other variants fall back to a scalar 4×
    /// emission via [`Self::gen_simd_scalar_fallback`].
    ///
    /// Per-lane semantics are bit-identical to the scalar [`Self::gen_expr`]
    /// path, so vanilla determinism is preserved.
    #[expect(
        clippy::too_many_lines,
        reason = "one match arm per DensityFunction variant; splitting the dispatch would obscure the per-variant SIMD codegen"
    )]
    fn gen_expr_simd(
        &mut self,
        df: &DensityFunction,
        input: &TranspilerInput,
        is_flat: bool,
    ) -> TokenStream {
        // Unified CSE (SIMD): if this node was hoisted by an enclosing scope,
        // emit the `f64x4` variable instead of recomputing the subtree.
        if is_cse_candidate(df) {
            let fp = fingerprint(df);
            if let Some(var) = self.cse_bindings_simd.get(&fp) {
                return quote! { #var };
            }
        }

        // Flat (xz-only) expressions don't depend on Y, so all 4 lanes are
        // bit-identical. Splatting the scalar avoids duplicating the per-lane
        // bindings and lets LLVM see the simpler form.
        if is_flat {
            let scalar = self.gen_expr(df, input, true);
            return quote! { f64x4::splat(#scalar) };
        }

        // Splines whose entire structure is Y-independent (e.g. driven by a
        // flat-cached climate Reference) can be evaluated scalar once and
        // splatted across the 4 lanes — saving the 4× scalar fallback the
        // generic path would otherwise emit. This is the only Spline-specific
        // SIMD treatment in the transpiler; lane-divergent Splines fall back
        // to scalar 4× emission via the catch-all arm below.
        if let DensityFunction::Spline(s) = df
            && self.is_spline_y_independent(&s.spline)
        {
            let scalar = self.gen_expr(df, input, true);
            return quote! { f64x4::splat(#scalar) };
        }

        match df {
            DensityFunction::Constant(c) => {
                let val = Literal::f64_unsuffixed(c.value);
                quote! { f64x4::splat(#val) }
            }

            DensityFunction::Noise(n) => {
                // Y-independent noise inside a 3D function: use the cached
                // scalar value, splatted across the 4 lanes.
                if n.y_scale == 0.0 {
                    let fp = fingerprint(df);
                    if let Some((idx, _, _)) = self.inline_flat_noises.get(&fp) {
                        let cache_field = format_ident!("inline_noise_{}", idx);
                        return quote! { f64x4::splat(cache.#cache_field) };
                    }
                }
                let field = noise_field_ident(&n.noise_id);
                let xz_scale = Literal::f64_unsuffixed(n.xz_scale);
                let y_scale = Literal::f64_unsuffixed(n.y_scale);
                if n.y_scale == 0.0 {
                    quote! {
                        f64x4::splat(noises.#field.get_value_xz(
                            f64::from(x) * #xz_scale, f64::from(z) * #xz_scale,
                        ))
                    }
                } else {
                    quote! {
                        noises.#field.get_value_4x(
                            f64::from(x) * #xz_scale,
                            ys * f64x4::splat(#y_scale),
                            f64::from(z) * #xz_scale,
                        )
                    }
                }
            }

            DensityFunction::BlendAlpha(_) => quote! { f64x4::splat(1.0) },
            DensityFunction::BlendOffset(_) => quote! { f64x4::splat(0.0) },

            DensityFunction::BlendDensity(bd) => self.gen_expr_simd(&bd.input, input, is_flat),

            DensityFunction::Marker(m) => {
                // Markers are transparent to SIMD codegen — recurse into the
                // wrapped function. (`Interpolated` markers in
                // `interpolated_param_mode` are rewritten by the scalar combine
                // paths, and the SIMD fill path never runs in
                // `interpolated_param_mode`, so the marker kind is moot here.)
                self.gen_expr_simd(&m.wrapped, input, is_flat)
            }

            DensityFunction::BlendedNoise(_) => {
                if self.fill_mode {
                    quote! { blended_noise_value_v }
                } else {
                    self.gen_simd_scalar_fallback(df, input, is_flat)
                }
            }

            DensityFunction::Reference(r) => {
                // Both interpolated params and fill-mode blended-noise refs inline
                // the referenced function's SIMD expression directly.
                if (self.interpolated_param_mode && self.interpolated_refs.contains(&r.id))
                    || (self.fill_mode && self.blended_noise_refs.contains(&r.id))
                {
                    if let Some(ref_df) = input.registry.get(&r.id) {
                        self.gen_expr_simd(ref_df, input, is_flat)
                    } else {
                        quote! { f64x4::splat(0.0) }
                    }
                } else if self.flat_cached.contains(&r.id) {
                    let field = named_fn_field_ident(&r.id);
                    quote! { f64x4::splat(cache.#field) }
                } else {
                    let fn_name = named_fn_ident_4x(&r.id);
                    quote! { #fn_name(noises, cache, x, ys, z) }
                }
            }

            DensityFunction::YClampedGradient(g) => {
                // Per-lane: map_clamped(f64::from(y), from_y, to_y, from_v, to_v).
                // Scalar form: `if t < 0 { from_v } else if t > 1 { to_v } else
                // { from_v + t * (to_v - from_v) }` — preserved bit-identically
                // via mask-select. `ys` holds integer-valued f64s already.
                let from_y = Literal::f64_unsuffixed(f64::from(g.from_y));
                let to_y = Literal::f64_unsuffixed(f64::from(g.to_y));
                let from_val = Literal::f64_unsuffixed(g.from_value);
                let to_val = Literal::f64_unsuffixed(g.to_value);
                quote! {{
                    let __t = (ys - f64x4::splat(#from_y))
                        / f64x4::splat(#to_y - #from_y);
                    let __min = f64x4::splat(#from_val);
                    let __max = f64x4::splat(#to_val);
                    let __lerped = __min + __t * (__max - __min);
                    let __below = __t.simd_lt(f64x4::splat(0.0));
                    let __above = __t.simd_gt(f64x4::splat(1.0));
                    let __r = __above.select(__max, __lerped);
                    __below.select(__min, __r)
                }}
            }

            DensityFunction::ShiftA(s) => {
                let field = noise_field_ident(&s.noise_id);
                quote! {
                    f64x4::splat(noises.#field.get_value_xz(
                        f64::from(x) * 0.25, f64::from(z) * 0.25,
                    ) * 4.0)
                }
            }

            DensityFunction::ShiftB(s) => {
                let field = noise_field_ident(&s.noise_id);
                quote! {
                    f64x4::splat(noises.#field.get_value_xy(
                        f64::from(z) * 0.25, f64::from(x) * 0.25,
                    ) * 4.0)
                }
            }

            DensityFunction::Shift(s) => {
                let field = noise_field_ident(&s.noise_id);
                quote! {
                    noises.#field.get_value_4x(
                        f64::from(x) * 0.25,
                        ys * f64x4::splat(0.25),
                        f64::from(z) * 0.25,
                    ) * f64x4::splat(4.0)
                }
            }

            DensityFunction::ShiftedNoise(sn) => {
                // `sn.shift_*` are themselves density functions evaluated at
                // (x, y, z). When all three shifts are Y-independent (typical
                // vanilla case — they're flat-cached `shift_x`/`shift_z` and
                // a constant `shift_y`), evaluate them as scalar splats and
                // call `get_value_4x(`. Otherwise fall back to scalar 4×.
                if self.is_y_independent(&sn.shift_x)
                    && self.is_y_independent(&sn.shift_y)
                    && self.is_y_independent(&sn.shift_z)
                {
                    let dx = self.gen_expr(&sn.shift_x, input, is_flat);
                    let dy = self.gen_expr(&sn.shift_y, input, is_flat);
                    let dz = self.gen_expr(&sn.shift_z, input, is_flat);
                    let field = noise_field_ident(&sn.noise_id);
                    let xz_scale = Literal::f64_unsuffixed(sn.xz_scale);
                    let y_scale = Literal::f64_unsuffixed(sn.y_scale);
                    if sn.y_scale == 0.0 {
                        // Y-independent overall — splat the scalar result.
                        quote! {{
                            let dx = #dx;
                            let dz = #dz;
                            f64x4::splat(noises.#field.get_value_xz(
                                f64::from(x) * #xz_scale + dx,
                                f64::from(z) * #xz_scale + dz,
                            ))
                        }}
                    } else {
                        quote! {{
                            let dx = #dx;
                            let dy = #dy;
                            let dz = #dz;
                            noises.#field.get_value_4x(
                                f64::from(x) * #xz_scale + dx,
                                ys * f64x4::splat(#y_scale) + f64x4::splat(dy),
                                f64::from(z) * #xz_scale + dz,
                            )
                        }}
                    }
                } else {
                    self.gen_simd_scalar_fallback(df, input, is_flat)
                }
            }

            DensityFunction::Mapped(m) => {
                let v = self.gen_expr_simd(&m.input, input, is_flat);
                match m.op {
                    MappedType::Abs => quote! { (#v).abs() },
                    MappedType::Square => quote! {{ let __v = #v; __v * __v }},
                    MappedType::Cube => quote! {{ let __v = #v; __v * __v * __v }},
                    MappedType::HalfNegative => {
                        // Scalar: if v > 0 { v } else { v * 0.5 }.
                        // Mask form: gt(0) ? v : v * 0.5
                        quote! {{
                            let __v = #v;
                            let __mask = __v.simd_gt(f64x4::splat(0.0));
                            __mask.select(__v, __v * f64x4::splat(0.5))
                        }}
                    }
                    MappedType::QuarterNegative => {
                        quote! {{
                            let __v = #v;
                            let __mask = __v.simd_gt(f64x4::splat(0.0));
                            __mask.select(__v, __v * f64x4::splat(0.25))
                        }}
                    }
                    MappedType::Invert => quote! { f64x4::splat(1.0) / (#v) },
                    MappedType::Squeeze => {
                        // Scalar: c = clamp(v, -1, 1); c / 2 - c * c * c / 24.
                        quote! {{
                            let __v = #v;
                            let __c = __v
                                .simd_max(f64x4::splat(-1.0))
                                .simd_min(f64x4::splat(1.0));
                            __c / f64x4::splat(2.0)
                                - __c * __c * __c / f64x4::splat(24.0)
                        }}
                    }
                }
            }

            DensityFunction::Clamp(c) => {
                let inner = self.gen_expr_simd(&c.input, input, is_flat);
                let min = Literal::f64_unsuffixed(c.min);
                let max = Literal::f64_unsuffixed(c.max);
                // Scalar `clamp` is `if v < min { min } else if v > max { max }
                // else { v }`. SIMD `simd_max(min).simd_min(max)` matches lane
                // by lane for finite values (no NaN in density values).
                quote! {
                    (#inner)
                        .simd_max(f64x4::splat(#min))
                        .simd_min(f64x4::splat(#max))
                }
            }

            DensityFunction::WeirdScaledSampler(ws) => {
                // Hybrid SIMD: the rarity input is batched 4-wide (it's
                // typically a Y-dependent Noise, so 4 scalar samples → 1 SIMD
                // sample). The outer `noise.get_value(x/scale, y/scale,
                // z/scale)` is per-lane scalar because each lane's scale —
                // derived from its own rarity — produces a different scaled
                // position, which can't be batched without changing the noise
                // API. Per-lane math is identical to the scalar fallback;
                // only the input evaluation moves from 4× scalar to 1× SIMD.
                let input_simd = self.gen_expr_simd(&ws.input, input, is_flat);
                let field = noise_field_ident(&ws.noise_id);
                let mapper = match ws.rarity_value_mapper {
                    RarityValueMapper::Tunnels => quote! { RarityValueMapper::Tunnels },
                    RarityValueMapper::Caves => quote! { RarityValueMapper::Caves },
                };
                let lane = |i: usize| -> TokenStream {
                    let i_lit = Literal::usize_unsuffixed(i);
                    quote! {{
                        let rarity = __rarity_arr[#i_lit];
                        let scale = #mapper.get_values(rarity);
                        #[allow(clippy::cast_possible_truncation)]
                        let y = __ys_arr[#i_lit] as i32;
                        scale * noises.#field.get_value(
                            f64::from(x) / scale,
                            f64::from(y) / scale,
                            f64::from(z) / scale,
                        ).abs()
                    }}
                };
                let r0 = lane(0);
                let r1 = lane(1);
                let r2 = lane(2);
                let r3 = lane(3);
                quote! {{
                    let __rarity_v = #input_simd;
                    let __rarity_arr = __rarity_v.to_array();
                    let __ys_arr = ys.to_array();
                    f64x4::from_array([#r0, #r1, #r2, #r3])
                }}
            }

            DensityFunction::TwoArgumentSimple(t) => {
                // CSE: hoist subexpressions common to both operands (mirrors the
                // scalar path). Without this the SIMD fill recomputes shared cave
                // subtrees (`entrances`, `pillars`, …) once per operand.
                let (hoisted, hoisted_fps) =
                    self.hoist_common_subexprs_simd(&[&t.argument1, &t.argument2], input, is_flat);

                // Add/Mul are uncontroversial — they just become SIMD ops.
                // Min/Max keep their static-bound short-circuit (the SIMD form
                // checks `simd_le`/`simd_ge` across all 4 lanes), which
                // preserves the scalar's "skip the right operand on the lucky
                // path" optimization.
                let body = match t.op {
                    TwoArgType::Add => {
                        let a = self.gen_expr_simd(&t.argument1, input, is_flat);
                        let b = self.gen_expr_simd(&t.argument2, input, is_flat);
                        quote! { ((#a) + (#b)) }
                    }
                    TwoArgType::Mul => {
                        let a = self.gen_expr_simd(&t.argument1, input, is_flat);
                        let b = self.gen_expr_simd(&t.argument2, input, is_flat);
                        quote! { ((#a) * (#b)) }
                    }
                    TwoArgType::Min => {
                        let (b_lo, _) = compute_bounds(&t.argument2, input);
                        let a = self.gen_expr_simd(&t.argument1, input, is_flat);
                        let b = self.gen_expr_simd(&t.argument2, input, is_flat);
                        if b_lo.is_finite() {
                            // If `a <= b_lo` for all lanes, then `b >= b_lo >= a`,
                            // so `min(a, b) = a`; the right operand is skipped.
                            let b_lo_lit = Literal::f64_unsuffixed(b_lo);
                            quote! {{
                                let __sc_a = #a;
                                if __sc_a.simd_le(f64x4::splat(#b_lo_lit)).all() {
                                    __sc_a
                                } else {
                                    __sc_a.simd_min(#b)
                                }
                            }}
                        } else {
                            quote! { (#a).simd_min(#b) }
                        }
                    }
                    TwoArgType::Max => {
                        let (_, b_hi) = compute_bounds(&t.argument2, input);
                        let a = self.gen_expr_simd(&t.argument1, input, is_flat);
                        let b = self.gen_expr_simd(&t.argument2, input, is_flat);
                        if b_hi.is_finite() {
                            let b_hi_lit = Literal::f64_unsuffixed(b_hi);
                            quote! {{
                                let __sc_a = #a;
                                if __sc_a.simd_ge(f64x4::splat(#b_hi_lit)).all() {
                                    __sc_a
                                } else {
                                    __sc_a.simd_max(#b)
                                }
                            }}
                        } else {
                            quote! { (#a).simd_max(#b) }
                        }
                    }
                };

                for fp in &hoisted_fps {
                    self.cse_bindings_simd.remove(fp);
                }

                if hoisted.is_empty() {
                    body
                } else {
                    quote! {{
                        #(#hoisted)*
                        #body
                    }}
                }
            }

            DensityFunction::EndIslands => {
                // EndIslands ignores its `block_y` argument — the result depends
                // only on (block_x, block_z). All 4 lanes get the same value, so
                // we evaluate scalar once and splat. This skips the 25×25
                // simplex-noise neighborhood scan three out of four times.
                quote! { f64x4::splat(noises.end_islands.sample(x, 0, z)) }
            }

            DensityFunction::RangeChoice(rc) => {
                // Mask-select per lane, with a runtime uniformity dispatch:
                // when all 4 lanes agree (the typical case for Y-stratified
                // RangeChoice trees), only the matching branch is evaluated.
                // Only when lanes diverge do we eat the both-branches cost.

                // Generate the input BEFORE registering its CSE binding so a
                // self-referencing input doesn't produce `let __v = __v;`.
                let input_simd = self.gen_expr_simd(&rc.input, input, is_flat);

                // CSE: register the input as `__v` so branches referencing it
                // reuse the bound value, then hoist subexprs common to both
                // branches. Mirrors the scalar `RangeChoice` arm — without it the
                // input (e.g. `pillars`) is re-evaluated inside the branches.
                let input_fp = if is_cse_candidate(&rc.input) {
                    let fp = fingerprint(&rc.input);
                    self.cse_bindings_simd.insert(fp, format_ident!("__v"));
                    Some(fp)
                } else {
                    None
                };
                let (hoisted, hoisted_fps) = self.hoist_common_subexprs_simd(
                    &[&rc.when_in_range, &rc.when_out_of_range],
                    input,
                    is_flat,
                );

                let in_range = self.gen_expr_simd(&rc.when_in_range, input, is_flat);
                let out_range = self.gen_expr_simd(&rc.when_out_of_range, input, is_flat);

                if let Some(fp) = input_fp {
                    self.cse_bindings_simd.remove(&fp);
                }
                for fp in &hoisted_fps {
                    self.cse_bindings_simd.remove(fp);
                }

                let min = Literal::f64_unsuffixed(rc.min_inclusive);
                let max = Literal::f64_unsuffixed(rc.max_exclusive);
                // `__v` is bound first so the hoisted bindings (which may
                // reference the input) and the branches can use it. The hoisted
                // subexprs are common to both branches, so whichever branch the
                // dispatch runs needs them — computing them before the `if` is
                // never wasted work.
                quote! {{
                    let __v = #input_simd;
                    #(#hoisted)*
                    let __in_mask = __v.simd_ge(f64x4::splat(#min))
                        & __v.simd_lt(f64x4::splat(#max));
                    if __in_mask.all() {
                        #in_range
                    } else if !__in_mask.any() {
                        #out_range
                    } else {
                        let __ir = #in_range;
                        let __or = #out_range;
                        __in_mask.select(__ir, __or)
                    }
                }}
            }

            // All other variants: scalar 4× fallback.
            _ => self.gen_simd_scalar_fallback(df, input, is_flat),
        }
    }

    /// Scalar 4× fallback for variants not yet migrated to true SIMD.
    ///
    /// Generates the scalar expression once and duplicates the resulting
    /// `TokenStream` across 4 independent `{ ... }` lane blocks. Each block has
    /// its own scope, so any CSE bindings (`let __cse_N = ...`) inside the
    /// duplicated tokens do not collide across lanes.
    fn gen_simd_scalar_fallback(
        &mut self,
        df: &DensityFunction,
        input: &TranspilerInput,
        is_flat: bool,
    ) -> TokenStream {
        let scalar = self.gen_expr(df, input, is_flat);

        // `blended_noise_value` is only emitted by `gen_expr` when
        // `fill_mode` is set, so only bind the lane scalar when needed.
        let bv_arr_decl = if self.fill_mode {
            quote! { let __bv_arr = blended_noise_value_v.to_array(); }
        } else {
            quote! {}
        };

        let lane_block = |i: usize, scalar: &TokenStream| -> TokenStream {
            let i_lit = Literal::usize_unsuffixed(i);
            let bv_decl = if self.fill_mode {
                quote! { let blended_noise_value = __bv_arr[#i_lit]; }
            } else {
                quote! {}
            };
            quote! {{
                #[allow(clippy::cast_possible_truncation)]
                let y = __ys_arr[#i_lit] as i32;
                #bv_decl
                #scalar
            }}
        };

        let r0 = lane_block(0, &scalar);
        let r1 = lane_block(1, &scalar);
        let r2 = lane_block(2, &scalar);
        let r3 = lane_block(3, &scalar);

        quote! {{
            let __ys_arr = ys.to_array();
            #bv_arr_decl
            let __r0 = #r0;
            let __r1 = #r1;
            let __r2 = #r2;
            let __r3 = #r3;
            f64x4::from_array([__r0, __r1, __r2, __r3])
        }}
    }

    /// Whether `df` evaluates to the same value for all 4 SIMD lanes given a
    /// fixed `(x, z)` — i.e. the subtree does not depend on Y, even
    /// transitively through `Reference` nodes.
    ///
    /// Stronger than the free-standing [`uses_y`] which doesn't recurse
    /// through `Reference`. Here we use the analyzer's `flat_cached` set: any
    /// `Reference` whose target uses Y (directly or transitively) is excluded
    /// from `flat_cached`, so this gives the tight "no Y at all" predicate
    /// the splat optimization needs.
    fn is_y_independent(&self, df: &DensityFunction) -> bool {
        match df {
            DensityFunction::Constant(_)
            | DensityFunction::ShiftA(_)
            | DensityFunction::ShiftB(_)
            | DensityFunction::BlendAlpha(_)
            | DensityFunction::BlendOffset(_)
            | DensityFunction::EndIslands
            | DensityFunction::FindTopSurface(_) => true,

            DensityFunction::Noise(n) => n.y_scale == 0.0,
            DensityFunction::ShiftedNoise(sn) => {
                sn.y_scale == 0.0
                    && self.is_y_independent(&sn.shift_x)
                    && self.is_y_independent(&sn.shift_y)
                    && self.is_y_independent(&sn.shift_z)
            }

            // All inherently Y-dependent. `WeirdScaledSampler` in particular
            // always samples noise at `(x, y, z) / scale`, so it uses Y
            // regardless of its input.
            DensityFunction::YClampedGradient(_)
            | DensityFunction::Shift(_)
            | DensityFunction::BlendedNoise(_)
            | DensityFunction::WeirdScaledSampler(_) => false,

            DensityFunction::Mapped(m) => self.is_y_independent(&m.input),
            DensityFunction::Clamp(c) => self.is_y_independent(&c.input),
            DensityFunction::TwoArgumentSimple(t) => {
                self.is_y_independent(&t.argument1) && self.is_y_independent(&t.argument2)
            }
            DensityFunction::RangeChoice(rc) => {
                self.is_y_independent(&rc.input)
                    && self.is_y_independent(&rc.when_in_range)
                    && self.is_y_independent(&rc.when_out_of_range)
            }
            DensityFunction::IntervalSelect(interval) => {
                self.is_y_independent(&interval.input)
                    && interval
                        .functions
                        .iter()
                        .all(|function| self.is_y_independent(function))
            }
            DensityFunction::BlendDensity(bd) => self.is_y_independent(&bd.input),
            DensityFunction::Marker(m) => self.is_y_independent(&m.wrapped),

            DensityFunction::Spline(s) => self.is_spline_y_independent(&s.spline),

            // A non-flat `Reference` is Y-dependent. The flatness analyzer
            // would have promoted it to `flat_cached` if it were Y-indep.
            DensityFunction::Reference(r) => self.flat_cached.contains(&r.id),
        }
    }

    fn is_spline_y_independent(&self, spline: &CubicSpline) -> bool {
        if !self.is_y_independent(&spline.coordinate) {
            return false;
        }
        spline.points.iter().all(|p| match &p.value {
            SplineValue::Constant(_) => true,
            SplineValue::Spline(nested) => self.is_spline_y_independent(nested),
        })
    }

    /// Find subexpressions common to all `branches` and hoist them into `let`
    /// bindings. Returns the bindings (as `TokenStream`s) and the fingerprints
    /// that were registered (caller must clean them up after generating the
    /// branch expressions).
    fn hoist_common_subexprs(
        &mut self,
        branches: &[&Arc<DensityFunction>],
        input: &TranspilerInput,
        is_flat: bool,
    ) -> (Vec<TokenStream>, Vec<u64>) {
        if branches.len() < 2 {
            return (Vec::new(), Vec::new());
        }

        // In interpolated param mode, references get inlined and Interpolated
        // markers rewritten to `interpolated[i]`, which can make hoisted
        // bindings dead code. Skip CSE in that mode.
        if self.interpolated_param_mode {
            return (Vec::new(), Vec::new());
        }

        // Collect expensive subexprs from each branch
        let branch_exprs: Vec<FxHashMap<u64, DensityFunction>> = branches
            .iter()
            .map(|b| collect_expensive_subexprs(b))
            .collect();

        // Find hashes present in ALL branches
        let common_fps: BTreeSet<u64> = branch_exprs[0]
            .keys()
            .filter(|fp| branch_exprs[1..].iter().all(|m| m.contains_key(*fp)))
            .copied()
            .collect();

        let mut bindings = Vec::new();
        let mut hoisted_fps = Vec::new();
        for fp in common_fps {
            if self.cse_bindings.contains_key(&fp) {
                continue;
            }
            let df = &branch_exprs[0][&fp];
            // Skip flat-cached references — they're already cheap cache reads
            if let DensityFunction::Reference(r) = df
                && self.flat_cached.contains(&r.id)
            {
                continue;
            }
            let var = format_ident!("__cse_{}", self.cse_counter);
            self.cse_counter += 1;
            let expr = self.gen_expr(df, input, is_flat);
            bindings.push(quote! { let #var = #expr; });
            self.cse_bindings.insert(fp, var);
            hoisted_fps.push(fp);
        }

        (bindings, hoisted_fps)
    }

    /// SIMD counterpart of [`Self::hoist_common_subexprs`]. Identical
    /// fingerprint/commonality logic, but emits `f64x4` bindings (values via
    /// `gen_expr_simd`) into the disjoint `cse_bindings_simd` map. The scalar
    /// CSE pass was historically never ported here, so the `_4x` fill path
    /// recomputed shared cave subtrees per operand/branch.
    fn hoist_common_subexprs_simd(
        &mut self,
        branches: &[&Arc<DensityFunction>],
        input: &TranspilerInput,
        is_flat: bool,
    ) -> (Vec<TokenStream>, Vec<u64>) {
        if branches.len() < 2 {
            return (Vec::new(), Vec::new());
        }
        if self.interpolated_param_mode {
            return (Vec::new(), Vec::new());
        }

        let branch_exprs: Vec<FxHashMap<u64, DensityFunction>> = branches
            .iter()
            .map(|b| collect_expensive_subexprs(b))
            .collect();

        let common_fps: BTreeSet<u64> = branch_exprs[0]
            .keys()
            .filter(|fp| branch_exprs[1..].iter().all(|m| m.contains_key(*fp)))
            .copied()
            .collect();

        let mut bindings = Vec::new();
        let mut hoisted_fps = Vec::new();
        for fp in common_fps {
            if self.cse_bindings_simd.contains_key(&fp) {
                continue;
            }
            let df = &branch_exprs[0][&fp];
            // Flat-cached references are already cheap cache reads — don't hoist.
            if let DensityFunction::Reference(r) = df
                && self.flat_cached.contains(&r.id)
            {
                continue;
            }
            let var = format_ident!("__cse_{}", self.cse_counter);
            self.cse_counter += 1;
            let expr = self.gen_expr_simd(df, input, is_flat);
            bindings.push(quote! { let #var = #expr; });
            self.cse_bindings_simd.insert(fp, var);
            hoisted_fps.push(fp);
        }

        (bindings, hoisted_fps)
    }
}

// ── Helper functions ────────────────────────────────────────────────────────

/// Static (lower, upper) bounds for a density function subtree.
///
/// Returned bounds satisfy `lower <= eval(df) <= upper` at runtime for all
/// inputs the function can be sampled at. When tight bounds aren't derivable
/// (e.g., free-form noise with unknown amplitude product, or potentially
/// unbounded operations like reciprocal), the corresponding side is set to
/// `f64::NEG_INFINITY` / `f64::INFINITY` and downstream short-circuit
/// optimizations correctly fall through to the unconditional codegen.
///
/// Mirrors the static-bounds analysis used by C2ME's
/// `MaxShortNode`/`MinShortNode` rewriters, with one extension: we resolve
/// `Reference` nodes through the build-time registry so cross-function
/// bounds propagate.
fn compute_bounds(df: &DensityFunction, input: &TranspilerInput) -> (f64, f64) {
    compute_bounds_inner(df, input, &mut Vec::new())
}

#[expect(
    clippy::too_many_lines,
    reason = "one match arm per DensityFunction variant; splitting the dispatch would obscure the per-variant bounds analysis"
)]
fn compute_bounds_inner(
    df: &DensityFunction,
    input: &TranspilerInput,
    visiting: &mut Vec<String>,
) -> (f64, f64) {
    match df {
        DensityFunction::Constant(c) => (c.value, c.value),

        DensityFunction::Reference(r) => {
            // Avoid infinite recursion through self-referential cycles (shouldn't
            // happen in practice, but DF graphs are cycle-free only by convention).
            if visiting.iter().any(|n| n == &r.id) {
                return (f64::NEG_INFINITY, f64::INFINITY);
            }
            let Some(target) = input.registry.get(&r.id) else {
                return (f64::NEG_INFINITY, f64::INFINITY);
            };
            visiting.push(r.id.clone());
            let bounds = compute_bounds_inner(target, input, visiting);
            visiting.pop();
            bounds
        }

        DensityFunction::YClampedGradient(g) => {
            let lo = g.from_value.min(g.to_value);
            let hi = g.from_value.max(g.to_value);
            (lo, hi)
        }

        DensityFunction::Noise(_)
        | DensityFunction::ShiftedNoise(_)
        | DensityFunction::ShiftA(_)
        | DensityFunction::ShiftB(_)
        | DensityFunction::Shift(_)
        | DensityFunction::Spline(_)
        | DensityFunction::BlendedNoise(_) => (f64::NEG_INFINITY, f64::INFINITY),

        DensityFunction::TwoArgumentSimple(t) => {
            let (a_lo, a_hi) = compute_bounds_inner(&t.argument1, input, visiting);
            let (b_lo, b_hi) = compute_bounds_inner(&t.argument2, input, visiting);
            match t.op {
                TwoArgType::Add => (a_lo + b_lo, a_hi + b_hi),
                TwoArgType::Mul => {
                    // Interval arithmetic for sign-mixed multiplication.
                    let candidates = [a_lo * b_lo, a_lo * b_hi, a_hi * b_lo, a_hi * b_hi];
                    let mut lo = f64::INFINITY;
                    let mut hi = f64::NEG_INFINITY;
                    for c in candidates {
                        if c.is_nan() {
                            return (f64::NEG_INFINITY, f64::INFINITY);
                        }
                        if c < lo {
                            lo = c;
                        }
                        if c > hi {
                            hi = c;
                        }
                    }
                    (lo, hi)
                }
                TwoArgType::Min => (a_lo.min(b_lo), a_hi.min(b_hi)),
                TwoArgType::Max => (a_lo.max(b_lo), a_hi.max(b_hi)),
            }
        }

        DensityFunction::Mapped(m) => {
            let (lo, hi) = compute_bounds_inner(&m.input, input, visiting);
            match m.op {
                MappedType::Abs => {
                    if lo >= 0.0 {
                        (lo, hi)
                    } else if hi <= 0.0 {
                        (-hi, -lo)
                    } else {
                        (0.0, lo.abs().max(hi.abs()))
                    }
                }
                MappedType::Square => {
                    if lo >= 0.0 {
                        (lo * lo, hi * hi)
                    } else if hi <= 0.0 {
                        (hi * hi, lo * lo)
                    } else {
                        (0.0, (lo * lo).max(hi * hi))
                    }
                }
                MappedType::Cube => {
                    // x^3 is monotone over the whole real line, so endpoints suffice.
                    (lo * lo * lo, hi * hi * hi)
                }
                MappedType::HalfNegative => {
                    // `if v > 0 { v } else { v * 0.5 }` — monotone non-decreasing
                    // (slope 0.5 below 0, slope 1 above 0).
                    let map = |v: f64| if v > 0.0 { v } else { v * 0.5 };
                    (map(lo), map(hi))
                }
                MappedType::QuarterNegative => {
                    let map = |v: f64| if v > 0.0 { v } else { v * 0.25 };
                    (map(lo), map(hi))
                }
                MappedType::Invert => {
                    // 1/v is unbounded near 0; only safe if input doesn't straddle 0.
                    if lo > 0.0 || hi < 0.0 {
                        let a = 1.0 / lo;
                        let b = 1.0 / hi;
                        (a.min(b), a.max(b))
                    } else {
                        (f64::NEG_INFINITY, f64::INFINITY)
                    }
                }
                MappedType::Squeeze => {
                    // clamp(-1, 1) → c/2 - c³/24. Endpoints: -1/2 + 1/24, 1/2 - 1/24.
                    let map = |v: f64| {
                        let c = v.clamp(-1.0, 1.0);
                        c / 2.0 - c * c * c / 24.0
                    };
                    let lo_c = lo.clamp(-1.0, 1.0);
                    let hi_c = hi.clamp(-1.0, 1.0);
                    (map(lo_c), map(hi_c))
                }
            }
        }

        DensityFunction::Clamp(c) => (c.min, c.max),

        DensityFunction::RangeChoice(rc) => {
            let (in_lo, in_hi) = compute_bounds_inner(&rc.when_in_range, input, visiting);
            let (out_lo, out_hi) = compute_bounds_inner(&rc.when_out_of_range, input, visiting);
            (in_lo.min(out_lo), in_hi.max(out_hi))
        }

        DensityFunction::IntervalSelect(interval) => {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for function in &interval.functions {
                let (function_lo, function_hi) = compute_bounds_inner(function, input, visiting);
                lo = lo.min(function_lo);
                hi = hi.max(function_hi);
            }
            if lo > hi {
                (f64::NEG_INFINITY, f64::INFINITY)
            } else {
                (lo, hi)
            }
        }

        DensityFunction::WeirdScaledSampler(_) => {
            // result = scale * noise.abs() where scale ∈ [0.5, 3.0] and
            // noise.abs() is non-negative. The upper bound is noise-parameter
            // dependent, so leave it unbounded for branch-elision purposes.
            (0.0, f64::INFINITY)
        }

        DensityFunction::EndIslands => (-100.0, 80.0),

        DensityFunction::BlendAlpha(_) => (1.0, 1.0),
        DensityFunction::BlendOffset(_) => (0.0, 0.0),
        DensityFunction::BlendDensity(bd) => compute_bounds_inner(&bd.input, input, visiting),

        DensityFunction::Marker(m) => compute_bounds_inner(&m.wrapped, input, visiting),

        DensityFunction::FindTopSurface(fts) => {
            // Returns a Y coordinate in [lower_bound, upper_bound rounded down].
            // upper_bound is itself a DF — its static upper bound caps the result.
            let (_, upper) = compute_bounds_inner(&fts.upper_bound, input, visiting);
            (f64::from(fts.lower_bound), upper)
        }
    }
}

/// Check if a density function subtree directly uses the `y` coordinate.
/// Does NOT recurse into References (those are handled by the flat inference loop).
fn uses_y(df: &DensityFunction) -> bool {
    match df {
        // uses y * 0.25
        DensityFunction::YClampedGradient(_)
        | DensityFunction::Shift(_)
        | DensityFunction::BlendedNoise(_) => true,
        DensityFunction::Noise(n) => n.y_scale != 0.0,
        DensityFunction::ShiftedNoise(sn) => sn.y_scale != 0.0 || uses_y(&sn.shift_y),
        DensityFunction::WeirdScaledSampler(ws) => uses_y(&ws.input),
        DensityFunction::TwoArgumentSimple(t) => uses_y(&t.argument1) || uses_y(&t.argument2),
        DensityFunction::Mapped(m) => uses_y(&m.input),
        DensityFunction::Clamp(c) => uses_y(&c.input),
        DensityFunction::RangeChoice(rc) => {
            uses_y(&rc.input) || uses_y(&rc.when_in_range) || uses_y(&rc.when_out_of_range)
        }
        DensityFunction::IntervalSelect(interval) => {
            uses_y(&interval.input) || interval.functions.iter().any(|function| uses_y(function))
        }
        DensityFunction::BlendDensity(bd) => uses_y(&bd.input),
        DensityFunction::Marker(m) => uses_y(&m.wrapped),
        DensityFunction::Spline(s) => uses_y_spline(&s.spline),
        // These don't use Y:
        // - FindTopSurface scans Y internally but result only depends on (x, z)
        // - References are handled at the analysis level
        // - Constants, shifts, blend, and end-islands are Y-independent
        DensityFunction::FindTopSurface(_)
        | DensityFunction::Reference(_)
        | DensityFunction::Constant(_)
        | DensityFunction::ShiftA(_)
        | DensityFunction::ShiftB(_)
        | DensityFunction::BlendAlpha(_)
        | DensityFunction::BlendOffset(_)
        | DensityFunction::EndIslands => false,
    }
}

fn uses_y_spline(spline: &CubicSpline) -> bool {
    if uses_y(&spline.coordinate) {
        return true;
    }
    spline.points.iter().any(|p| {
        if let SplineValue::Spline(nested) = &p.value {
            uses_y_spline(nested)
        } else {
            false
        }
    })
}

const fn is_flat_cached(df: &DensityFunction) -> bool {
    match df {
        DensityFunction::Marker(m) => matches!(m.kind, MarkerType::FlatCache | MarkerType::Cache2D),
        _ => false,
    }
}

fn unwrap_markers(df: &DensityFunction) -> &DensityFunction {
    match df {
        DensityFunction::Marker(m) => unwrap_markers(&m.wrapped),
        other => other,
    }
}

fn collect_references(df: &DensityFunction) -> Vec<String> {
    let mut refs = Vec::new();
    collect_refs_inner(df, &mut refs);
    refs
}

fn collect_refs_inner(df: &DensityFunction, refs: &mut Vec<String>) {
    match df {
        DensityFunction::Reference(r) if !refs.contains(&r.id) => {
            refs.push(r.id.clone());
        }
        DensityFunction::Marker(m) => collect_refs_inner(&m.wrapped, refs),
        DensityFunction::TwoArgumentSimple(t) => {
            collect_refs_inner(&t.argument1, refs);
            collect_refs_inner(&t.argument2, refs);
        }
        DensityFunction::Mapped(m) => collect_refs_inner(&m.input, refs),
        DensityFunction::Clamp(c) => collect_refs_inner(&c.input, refs),
        DensityFunction::RangeChoice(rc) => {
            collect_refs_inner(&rc.input, refs);
            collect_refs_inner(&rc.when_in_range, refs);
            collect_refs_inner(&rc.when_out_of_range, refs);
        }
        DensityFunction::IntervalSelect(interval) => {
            collect_refs_inner(&interval.input, refs);
            for function in &interval.functions {
                collect_refs_inner(function, refs);
            }
        }
        DensityFunction::ShiftedNoise(sn) => {
            collect_refs_inner(&sn.shift_x, refs);
            collect_refs_inner(&sn.shift_y, refs);
            collect_refs_inner(&sn.shift_z, refs);
        }
        DensityFunction::BlendDensity(bd) => collect_refs_inner(&bd.input, refs),
        DensityFunction::WeirdScaledSampler(ws) => collect_refs_inner(&ws.input, refs),
        DensityFunction::Spline(s) => collect_spline_refs(&s.spline, refs),
        DensityFunction::FindTopSurface(fts) => {
            collect_refs_inner(&fts.density, refs);
            collect_refs_inner(&fts.upper_bound, refs);
        }
        _ => {}
    }
}

/// Recursively collect `Noise` nodes with `y_scale == 0.0` in a density
/// function tree. These are Y-independent computations that can be cached
/// per (x, z) column. Keyed by structural hash → `(noise_id, xz_scale)`.
fn collect_inline_flat_noises(df: &DensityFunction, out: &mut BTreeMap<u64, (String, f64)>) {
    if let DensityFunction::Noise(n) = df
        && n.y_scale == 0.0
    {
        let fp = fingerprint(df);
        out.entry(fp)
            .or_insert_with(|| (n.noise_id.clone(), n.xz_scale));
    }
    // Recurse into children (but NOT into References — those are separate functions)
    match df {
        DensityFunction::TwoArgumentSimple(t) => {
            collect_inline_flat_noises(&t.argument1, out);
            collect_inline_flat_noises(&t.argument2, out);
        }
        DensityFunction::Mapped(m) => collect_inline_flat_noises(&m.input, out),
        DensityFunction::Clamp(c) => collect_inline_flat_noises(&c.input, out),
        DensityFunction::RangeChoice(rc) => {
            collect_inline_flat_noises(&rc.input, out);
            collect_inline_flat_noises(&rc.when_in_range, out);
            collect_inline_flat_noises(&rc.when_out_of_range, out);
        }
        DensityFunction::IntervalSelect(interval) => {
            collect_inline_flat_noises(&interval.input, out);
            for function in &interval.functions {
                collect_inline_flat_noises(function, out);
            }
        }
        DensityFunction::WeirdScaledSampler(ws) => collect_inline_flat_noises(&ws.input, out),
        DensityFunction::BlendDensity(bd) => collect_inline_flat_noises(&bd.input, out),
        DensityFunction::Marker(m) => collect_inline_flat_noises(&m.wrapped, out),
        DensityFunction::ShiftedNoise(sn) => {
            collect_inline_flat_noises(&sn.shift_x, out);
            collect_inline_flat_noises(&sn.shift_y, out);
            collect_inline_flat_noises(&sn.shift_z, out);
        }
        _ => {}
    }
}

/// Whether a node is a CSE candidate (worth deduplicating).
const fn is_cse_candidate(df: &DensityFunction) -> bool {
    matches!(
        df,
        DensityFunction::Reference(_)
            | DensityFunction::Noise(_)
            | DensityFunction::ShiftedNoise(_)
    )
}

/// Collect CSE-candidate subexpressions with their structural hashes.
fn collect_expensive_subexprs(df: &DensityFunction) -> FxHashMap<u64, DensityFunction> {
    let mut result = FxHashMap::default();
    collect_expensive_inner(df, &mut result);
    result
}

fn collect_expensive_inner(df: &DensityFunction, out: &mut FxHashMap<u64, DensityFunction>) {
    if is_cse_candidate(df) {
        let fp = fingerprint(df);
        out.entry(fp).or_insert_with(|| df.clone());
    }
    // Recurse into children
    match df {
        DensityFunction::TwoArgumentSimple(t) => {
            collect_expensive_inner(&t.argument1, out);
            collect_expensive_inner(&t.argument2, out);
        }
        DensityFunction::Mapped(m) => collect_expensive_inner(&m.input, out),
        DensityFunction::Clamp(c) => collect_expensive_inner(&c.input, out),
        DensityFunction::RangeChoice(rc) => {
            collect_expensive_inner(&rc.input, out);
            collect_expensive_inner(&rc.when_in_range, out);
            collect_expensive_inner(&rc.when_out_of_range, out);
        }
        DensityFunction::IntervalSelect(interval) => {
            collect_expensive_inner(&interval.input, out);
            for function in &interval.functions {
                collect_expensive_inner(function, out);
            }
        }
        DensityFunction::WeirdScaledSampler(ws) => collect_expensive_inner(&ws.input, out),
        DensityFunction::BlendDensity(bd) => collect_expensive_inner(&bd.input, out),
        DensityFunction::Marker(m) => collect_expensive_inner(&m.wrapped, out),
        _ => {}
    }
}

fn collect_spline_refs(spline: &CubicSpline, refs: &mut Vec<String>) {
    collect_refs_inner(&spline.coordinate, refs);
    for point in &spline.points {
        if let SplineValue::Spline(nested) = &point.value {
            collect_spline_refs(nested, refs);
        }
    }
}

/// Collect the inner functions of all `Interpolated` markers in DFS order,
/// resolving references through the registry.
///
/// The DFS order must match `gen_expr` with `interpolated_param_mode` so that
/// the indices align between `fill_cell_corner_densities` and `combine_interpolated`.
fn collect_interpolated_inners(
    df: &DensityFunction,
    registry: &BTreeMap<String, DensityFunction>,
) -> Vec<DensityFunction> {
    let mut inners = Vec::new();
    collect_interpolated_walk(df, registry, &mut inners);
    inners
}

fn collect_interpolated_walk(
    df: &DensityFunction,
    registry: &BTreeMap<String, DensityFunction>,
    inners: &mut Vec<DensityFunction>,
) {
    match df {
        DensityFunction::Marker(m) if m.kind == MarkerType::Interpolated => {
            // Collect the inner function; do NOT recurse into it
            inners.push((*m.wrapped).clone());
        }
        DensityFunction::Marker(m) => collect_interpolated_walk(&m.wrapped, registry, inners),
        DensityFunction::TwoArgumentSimple(t) => {
            collect_interpolated_walk(&t.argument1, registry, inners);
            collect_interpolated_walk(&t.argument2, registry, inners);
        }
        DensityFunction::Mapped(m) => collect_interpolated_walk(&m.input, registry, inners),
        DensityFunction::Clamp(c) => collect_interpolated_walk(&c.input, registry, inners),
        DensityFunction::RangeChoice(rc) => {
            collect_interpolated_walk(&rc.input, registry, inners);
            collect_interpolated_walk(&rc.when_in_range, registry, inners);
            collect_interpolated_walk(&rc.when_out_of_range, registry, inners);
        }
        DensityFunction::IntervalSelect(interval) => {
            collect_interpolated_walk(&interval.input, registry, inners);
            for function in &interval.functions {
                collect_interpolated_walk(function, registry, inners);
            }
        }
        DensityFunction::BlendDensity(bd) => {
            collect_interpolated_walk(&bd.input, registry, inners);
        }
        DensityFunction::WeirdScaledSampler(ws) => {
            collect_interpolated_walk(&ws.input, registry, inners);
        }
        DensityFunction::Spline(s) => {
            collect_interpolated_spline_walk(&s.spline, registry, inners);
        }
        DensityFunction::ShiftedNoise(sn) => {
            collect_interpolated_walk(&sn.shift_x, registry, inners);
            collect_interpolated_walk(&sn.shift_y, registry, inners);
            collect_interpolated_walk(&sn.shift_z, registry, inners);
        }
        DensityFunction::FindTopSurface(fts) => {
            collect_interpolated_walk(&fts.density, registry, inners);
            collect_interpolated_walk(&fts.upper_bound, registry, inners);
        }
        DensityFunction::Reference(r) => {
            if let Some(ref_df) = registry.get(&r.id) {
                collect_interpolated_walk(ref_df, registry, inners);
            }
        }
        _ => {}
    }
}

fn collect_interpolated_spline_walk(
    spline: &CubicSpline,
    registry: &BTreeMap<String, DensityFunction>,
    inners: &mut Vec<DensityFunction>,
) {
    collect_interpolated_walk(&spline.coordinate, registry, inners);
    for point in &spline.points {
        if let SplineValue::Spline(nested) = &point.value {
            collect_interpolated_spline_walk(nested, registry, inners);
        }
    }
}

/// Check if a density function tree transitively contains `BlendedNoise`.
fn has_blended_noise(
    df: &DensityFunction,
    registry: &BTreeMap<String, DensityFunction>,
    visited: &mut BTreeSet<String>,
) -> bool {
    match df {
        DensityFunction::BlendedNoise(_) => true,
        DensityFunction::TwoArgumentSimple(t) => {
            has_blended_noise(&t.argument1, registry, visited)
                || has_blended_noise(&t.argument2, registry, visited)
        }
        DensityFunction::Mapped(m) => has_blended_noise(&m.input, registry, visited),
        DensityFunction::Clamp(c) => has_blended_noise(&c.input, registry, visited),
        DensityFunction::Marker(m) => has_blended_noise(&m.wrapped, registry, visited),
        DensityFunction::RangeChoice(rc) => {
            has_blended_noise(&rc.input, registry, visited)
                || has_blended_noise(&rc.when_in_range, registry, visited)
                || has_blended_noise(&rc.when_out_of_range, registry, visited)
        }
        DensityFunction::IntervalSelect(interval) => {
            has_blended_noise(&interval.input, registry, visited)
                || interval
                    .functions
                    .iter()
                    .any(|function| has_blended_noise(function, registry, visited))
        }
        DensityFunction::BlendDensity(bd) => has_blended_noise(&bd.input, registry, visited),
        DensityFunction::WeirdScaledSampler(ws) => has_blended_noise(&ws.input, registry, visited),
        DensityFunction::ShiftedNoise(sn) => {
            has_blended_noise(&sn.shift_x, registry, visited)
                || has_blended_noise(&sn.shift_y, registry, visited)
                || has_blended_noise(&sn.shift_z, registry, visited)
        }
        DensityFunction::FindTopSurface(fts) => {
            has_blended_noise(&fts.density, registry, visited)
                || has_blended_noise(&fts.upper_bound, registry, visited)
        }
        DensityFunction::Spline(s) => has_blended_noise_spline(&s.spline, registry, visited),
        DensityFunction::Reference(r) => {
            if visited.contains(&r.id) {
                return false;
            }
            visited.insert(r.id.clone());
            registry
                .get(&r.id)
                .is_some_and(|ref_df| has_blended_noise(ref_df, registry, visited))
        }
        _ => false,
    }
}

fn has_blended_noise_spline(
    spline: &CubicSpline,
    registry: &BTreeMap<String, DensityFunction>,
    visited: &mut BTreeSet<String>,
) -> bool {
    if has_blended_noise(&spline.coordinate, registry, visited) {
        return true;
    }
    spline.points.iter().any(|p| {
        if let SplineValue::Spline(nested) = &p.value {
            has_blended_noise_spline(nested, registry, visited)
        } else {
            false
        }
    })
}

/// Check if a named function (transitively) contains `Interpolated` markers.
fn has_interpolated_markers(
    df: &DensityFunction,
    registry: &BTreeMap<String, DensityFunction>,
    visited: &mut BTreeSet<String>,
) -> bool {
    match df {
        DensityFunction::Marker(m) if m.kind == MarkerType::Interpolated => true,
        DensityFunction::Marker(m) => has_interpolated_markers(&m.wrapped, registry, visited),
        DensityFunction::TwoArgumentSimple(t) => {
            has_interpolated_markers(&t.argument1, registry, visited)
                || has_interpolated_markers(&t.argument2, registry, visited)
        }
        DensityFunction::Mapped(m) => has_interpolated_markers(&m.input, registry, visited),
        DensityFunction::Clamp(c) => has_interpolated_markers(&c.input, registry, visited),
        DensityFunction::RangeChoice(rc) => {
            has_interpolated_markers(&rc.input, registry, visited)
                || has_interpolated_markers(&rc.when_in_range, registry, visited)
                || has_interpolated_markers(&rc.when_out_of_range, registry, visited)
        }
        DensityFunction::IntervalSelect(interval) => {
            has_interpolated_markers(&interval.input, registry, visited)
                || interval
                    .functions
                    .iter()
                    .any(|function| has_interpolated_markers(function, registry, visited))
        }
        DensityFunction::BlendDensity(bd) => has_interpolated_markers(&bd.input, registry, visited),
        DensityFunction::WeirdScaledSampler(ws) => {
            has_interpolated_markers(&ws.input, registry, visited)
        }
        DensityFunction::ShiftedNoise(sn) => {
            has_interpolated_markers(&sn.shift_x, registry, visited)
                || has_interpolated_markers(&sn.shift_y, registry, visited)
                || has_interpolated_markers(&sn.shift_z, registry, visited)
        }
        DensityFunction::FindTopSurface(fts) => {
            has_interpolated_markers(&fts.density, registry, visited)
                || has_interpolated_markers(&fts.upper_bound, registry, visited)
        }
        DensityFunction::Reference(r) => {
            if visited.contains(&r.id) {
                return false;
            }
            visited.insert(r.id.clone());
            registry
                .get(&r.id)
                .is_some_and(|ref_df| has_interpolated_markers(ref_df, registry, visited))
        }
        // Splines could contain interpolated markers in theory
        DensityFunction::Spline(s) => has_interpolated_spline(&s.spline, registry, visited),
        _ => false,
    }
}

fn has_interpolated_spline(
    spline: &CubicSpline,
    registry: &BTreeMap<String, DensityFunction>,
    visited: &mut BTreeSet<String>,
) -> bool {
    if has_interpolated_markers(&spline.coordinate, registry, visited) {
        return true;
    }
    spline.points.iter().any(|p| {
        if let SplineValue::Spline(nested) = &p.value {
            has_interpolated_spline(nested, registry, visited)
        } else {
            false
        }
    })
}

fn noise_field_ident(noise_id: &str) -> Ident {
    format_ident!("n_{}", sanitize_name(noise_id))
}

fn named_fn_field_ident(name: &str) -> Ident {
    format_ident!("df_{}", sanitize_name(name))
}

fn named_fn_ident(name: &str) -> Ident {
    format_ident!("compute_{}", sanitize_name(name))
}

fn named_fn_ident_4x(name: &str) -> Ident {
    format_ident!("compute_{}_4x", sanitize_name(name))
}

fn grid_field_ident(name: &str) -> Ident {
    format_ident!("grid_df_{}", sanitize_name(name))
}

fn router_cache_field_ident(name: &str) -> Ident {
    format_ident!("router_{}", sanitize_name(name))
}

fn router_grid_field_ident(name: &str) -> Ident {
    format_ident!("grid_router_{}", sanitize_name(name))
}

fn router_compute_fn_ident(name: &str) -> Ident {
    format_ident!("compute_router_{}", sanitize_name(name))
}

/// Converts a namespaced ID to a valid Rust identifier.
///
/// `"minecraft:overworld/continents"` → `"overworld__continents"`
/// `"mymod:custom/noise"` → `"custom__noise"`
fn sanitize_name(id: &str) -> String {
    // Take just the path component, stripping any namespace (e.g. "minecraft:", "mymod:")
    let path = match id.split_once(':') {
        Some((_, path)) => path,
        None => id,
    };
    path.replace('/', "__").replace('-', "_")
}

/// Produce a structural hash for a `DensityFunction` subtree.
///
/// Two structurally identical subtrees produce the same hash. Used to detect
/// common subexpressions across sibling branches (e.g., both arguments of a
/// `Max` or `Min`).
fn fingerprint(df: &DensityFunction) -> u64 {
    let mut hasher = FxHasher::default();
    hash_df(df, &mut hasher);
    hasher.finish()
}

/// Hash a `DensityFunction` tree into the given hasher. Each variant is
/// discriminated by a unique tag byte so structurally different trees never
/// collide (within the limits of the hash).
fn hash_df(df: &DensityFunction, h: &mut impl Hasher) {
    mem::discriminant(df).hash(h);
    match df {
        DensityFunction::Constant(c) => c.value.to_bits().hash(h),
        DensityFunction::Reference(r) => r.id.hash(h),
        DensityFunction::YClampedGradient(g) => {
            g.from_y.hash(h);
            g.to_y.hash(h);
            g.from_value.to_bits().hash(h);
            g.to_value.to_bits().hash(h);
        }
        DensityFunction::Noise(n) => {
            n.noise_id.hash(h);
            n.xz_scale.to_bits().hash(h);
            n.y_scale.to_bits().hash(h);
        }
        DensityFunction::ShiftedNoise(sn) => {
            hash_df(&sn.shift_x, h);
            hash_df(&sn.shift_y, h);
            hash_df(&sn.shift_z, h);
            sn.xz_scale.to_bits().hash(h);
            sn.y_scale.to_bits().hash(h);
            sn.noise_id.hash(h);
        }
        DensityFunction::ShiftA(s) => s.noise_id.hash(h),
        DensityFunction::ShiftB(s) => s.noise_id.hash(h),
        DensityFunction::Shift(s) => s.noise_id.hash(h),
        DensityFunction::TwoArgumentSimple(t) => {
            mem::discriminant(&t.op).hash(h);
            hash_df(&t.argument1, h);
            hash_df(&t.argument2, h);
        }
        DensityFunction::Mapped(m) => {
            mem::discriminant(&m.op).hash(h);
            hash_df(&m.input, h);
        }
        DensityFunction::Clamp(c) => {
            c.min.to_bits().hash(h);
            c.max.to_bits().hash(h);
            hash_df(&c.input, h);
        }
        DensityFunction::RangeChoice(rc) => {
            rc.min_inclusive.to_bits().hash(h);
            rc.max_exclusive.to_bits().hash(h);
            hash_df(&rc.input, h);
            hash_df(&rc.when_in_range, h);
            hash_df(&rc.when_out_of_range, h);
        }
        DensityFunction::IntervalSelect(interval) => {
            hash_df(&interval.input, h);
            for threshold in &interval.thresholds {
                threshold.to_bits().hash(h);
            }
            for function in &interval.functions {
                hash_df(function, h);
            }
        }
        DensityFunction::WeirdScaledSampler(ws) => {
            mem::discriminant(&ws.rarity_value_mapper).hash(h);
            ws.noise_id.hash(h);
            hash_df(&ws.input, h);
        }
        DensityFunction::Spline(_)
        | DensityFunction::BlendedNoise(_)
        | DensityFunction::EndIslands
        | DensityFunction::BlendAlpha(_)
        | DensityFunction::BlendOffset(_) => {}
        DensityFunction::BlendDensity(bd) => hash_df(&bd.input, h),
        DensityFunction::Marker(m) => {
            mem::discriminant(&m.kind).hash(h);
            hash_df(&m.wrapped, h);
        }
        DensityFunction::FindTopSurface(fts) => {
            hash_df(&fts.density, h);
            hash_df(&fts.upper_bound, h);
        }
    }
}
