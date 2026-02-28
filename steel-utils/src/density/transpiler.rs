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
use std::sync::Arc;

use proc_macro2::{Ident, Literal, TokenStream};
use quote::{format_ident, quote};

use super::{
    CubicSpline, DensityFunction, MappedType, MarkerType, RarityValueMapper, SplineValue,
    TwoArgType,
};

/// Input to the transpiler.
pub struct TranspilerInput {
    /// Named density functions (registry entries like `"minecraft:overworld/continents"`).
    pub registry: BTreeMap<String, DensityFunction>,
    /// Noise router entry points (like `"temperature"`, `"final_density"`).
    pub router_entries: BTreeMap<String, DensityFunction>,
}

/// Compile density function trees into a `TokenStream` of Rust code.
///
/// The generated code contains:
/// - `OverworldNoises` struct with one `NormalNoise` field per noise used
/// - `OverworldColumnCache` struct with fields for flat-cached (xz-only) values
/// - Private `compute_*` functions for each named density function
/// - Public `router_*` functions for each noise router entry point
#[must_use] 
pub fn transpile(input: &TranspilerInput) -> TokenStream {
    let mut ctx = TranspileContext::new();

    // Phase 1: Analyze the graph
    ctx.analyze(input);

    // Phase 2: Generate code
    let noises_struct = ctx.gen_noises_struct();
    let noises_impl = ctx.gen_noises_impl();
    let named_fns = ctx.gen_named_functions(input);
    let column_cache = ctx.gen_column_cache(input);
    let router_fns = ctx.gen_router_functions(input);

    quote! {
        #![doc = r" Generated density function code for terrain generation."]
        #![doc = r""]
        #![doc = r" Compiled from vanilla datapack JSON at build time by the density function transpiler."]
        #![doc = r" Do not edit manually."]

        use steel_utils::density::spline_eval;
        use steel_utils::density::RarityValueMapper;
        use steel_utils::math::{clamp, map_clamped};
        use steel_utils::noise::NormalNoise;
        use steel_utils::random::RandomSplitter;

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
    /// Named functions in topological order (dependencies first).
    topo_order: Vec<String>,
    /// Named functions that are actually reachable from router entries.
    used_names: BTreeSet<String>,
    /// Counter for generating unique spline function names.
    spline_counter: usize,
    /// Generated spline helper functions.
    spline_fns: Vec<TokenStream>,
}

impl TranspileContext {
    const fn new() -> Self {
        Self {
            noise_ids: BTreeSet::new(),
            flat_cached: BTreeSet::new(),
            topo_order: Vec::new(),
            used_names: BTreeSet::new(),
            spline_counter: 0,
            spline_fns: Vec::new(),
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
                && is_flat_cached(df) {
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

        self.topo_order = self.topological_sort(input);
    }

    fn walk_df(&mut self, df: &DensityFunction, input: &TranspilerInput) {
        match df {
            DensityFunction::Constant(_)
            | DensityFunction::BlendAlpha(_)
            | DensityFunction::BlendOffset(_)
            | DensityFunction::EndIslands(_)
            | DensityFunction::YClampedGradient(_) => {}

            DensityFunction::Noise(n) => {
                self.noise_ids.insert(n.noise_id.clone());
            }
            DensityFunction::ShiftedNoise(sn) => {
                self.walk_df(&sn.shift_x, input);
                self.walk_df(&sn.shift_y, input);
                self.walk_df(&sn.shift_z, input);
                self.noise_ids.insert(sn.noise_id.clone());
            }
            DensityFunction::ShiftA(s) => { self.noise_ids.insert(s.noise_id.clone()); }
            DensityFunction::ShiftB(s) => { self.noise_ids.insert(s.noise_id.clone()); }
            DensityFunction::Shift(s) => { self.noise_ids.insert(s.noise_id.clone()); }
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
            DensityFunction::Spline(s) => self.walk_spline(&s.spline, input),
            DensityFunction::BlendedNoise(_) => {
                self.noise_ids.insert("minecraft:offset".to_string());
            }
            DensityFunction::WeirdScaledSampler(ws) => {
                self.walk_df(&ws.input, input);
                self.noise_ids.insert(ws.noise_id.clone());
            }
            DensityFunction::BlendDensity(bd) => self.walk_df(&bd.input, input),
            DensityFunction::Marker(m) => self.walk_df(&m.wrapped, input),
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

        quote! {
            /// All noise generators needed by the overworld density functions.
            ///
            /// Created at runtime from a seed via [`OverworldNoises::create`].
            pub struct OverworldNoises {
                #(#fields),*
            }
        }
    }

    fn gen_noises_impl(&self) -> TokenStream {
        let field_inits: Vec<TokenStream> = self
            .noise_ids
            .iter()
            .map(|id| {
                let field = noise_field_ident(id);
                let id_lit = Literal::string(id);
                quote! {
                    #field: {
                        let p = params.get(#id_lit).expect(concat!("missing noise params: ", #id_lit));
                        NormalNoise::create(splitter, #id_lit, p.first_octave, &p.amplitudes)
                    }
                }
            })
            .collect();

        quote! {
            impl OverworldNoises {
                /// Create all noise generators from a seed's positional splitter and noise parameters.
                pub fn create(
                    splitter: &RandomSplitter,
                    params: &rustc_hash::FxHashMap<String, steel_utils::density::NoiseParameters>,
                ) -> Self {
                    Self {
                        #(#field_inits),*
                    }
                }
            }
        }
    }

    /// Generate the `OverworldColumnCache` struct and its `ensure` method.
    fn gen_column_cache(&mut self, _input: &TranspilerInput) -> TokenStream {
        let flat_names: Vec<&String> = self
            .topo_order
            .iter()
            .filter(|n| self.flat_cached.contains(*n))
            .collect();

        let cache_fields: Vec<TokenStream> = flat_names
            .iter()
            .map(|name| {
                let field = named_fn_field_ident(name);
                quote! { pub #field: f64 }
            })
            .collect();

        // Generate the ensure body: compute each flat-cached value in topo order.
        // We reborrow &*self temporarily for function calls, then write to self.field.
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

        let default_fields: Vec<TokenStream> = flat_names
            .iter()
            .map(|name| {
                let field = named_fn_field_ident(name);
                quote! { #field: 0.0 }
            })
            .collect();

        quote! {
            /// Column-level cache for flat-cached (xz-only) density function results.
            ///
            /// Call [`ensure`](Self::ensure) before reading values. Values are recomputed
            /// only when `(x, z)` changes.
            pub struct OverworldColumnCache {
                /// Cached x block coordinate.
                pub x: i32,
                /// Cached z block coordinate.
                pub z: i32,
                valid: bool,
                #(#cache_fields),*
            }

            impl OverworldColumnCache {
                /// Create a new, empty column cache.
                #[must_use]
                pub fn new() -> Self {
                    Self {
                        x: 0,
                        z: 0,
                        valid: false,
                        #(#default_fields),*
                    }
                }

                /// Ensure the cache is populated for the given `(x, z)` block coordinates.
                ///
                /// If the cache already holds values for this column, this is a no-op.
                /// Computes all flat-cached density functions in topological order.
                pub fn ensure(&mut self, x: i32, z: i32, noises: &OverworldNoises) {
                    if self.valid && self.x == x && self.z == z {
                        return;
                    }
                    self.x = x;
                    self.z = z;
                    #(#ensure_stmts)*
                    self.valid = true;
                }
            }
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

            let params = if is_flat {
                quote! { noises: &OverworldNoises, cache: &OverworldColumnCache, x: i32, z: i32 }
            } else {
                quote! { noises: &OverworldNoises, cache: &OverworldColumnCache, x: i32, y: i32, z: i32 }
            };

            let doc = Literal::string(&format!("`{name}`"));
            fns.push(quote! {
                #[doc = #doc]
                #[inline]
                fn #fn_name(#params) -> f64 {
                    #body
                }
            });
        }

        let spline_fns = std::mem::take(&mut self.spline_fns);

        quote! {
            #(#fns)*
            #(#spline_fns)*
        }
    }

    /// Generate the router entry point functions.
    fn gen_router_functions(&mut self, input: &TranspilerInput) -> TokenStream {
        let mut fns = Vec::new();

        for (name, df) in &input.router_entries {
            let fn_name = format_ident!("router_{}", sanitize_name(name));
            let inner = unwrap_markers(df);
            let is_flat = is_flat_cached(df);

            let body = self.gen_expr(inner, input, is_flat);

            let params = if is_flat {
                quote! { noises: &OverworldNoises, cache: &OverworldColumnCache }
            } else {
                quote! { noises: &OverworldNoises, cache: &OverworldColumnCache, x: i32, y: i32, z: i32 }
            };

            let doc = Literal::string(&format!("Noise router entry: `{name}`"));
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

        let spline_fns = std::mem::take(&mut self.spline_fns);
        quote! {
            #(#fns)*
            #(#spline_fns)*
        }
    }

    // ── Expression generation ───────────────────────────────────────────

    /// Generate a `TokenStream` expression that computes a density function value.
    ///
    /// `is_flat` indicates this expression tree is xz-only (no y available).
    fn gen_expr(
        &mut self,
        df: &DensityFunction,
        input: &TranspilerInput,
        is_flat: bool,
    ) -> TokenStream {
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
                let field = noise_field_ident(&n.noise_id);
                let xz_scale = Literal::f64_unsuffixed(n.xz_scale);
                let y_scale = Literal::f64_unsuffixed(n.y_scale);
                if is_flat || n.y_scale == 0.0 {
                    quote! { noises.#field.get_value(f64::from(x) * #xz_scale, 0.0, f64::from(z) * #xz_scale) }
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
                        noises.#field.get_value(
                            f64::from(x) * #xz_scale + dx,
                            0.0,
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
                quote! { noises.#field.get_value(f64::from(x) * 0.25, 0.0, f64::from(z) * 0.25) * 4.0 }
            }

            DensityFunction::ShiftB(s) => {
                let field = noise_field_ident(&s.noise_id);
                quote! { noises.#field.get_value(f64::from(z) * 0.25, f64::from(x) * 0.25, 0.0) * 4.0 }
            }

            DensityFunction::Shift(s) => {
                let field = noise_field_ident(&s.noise_id);
                if is_flat {
                    quote! { noises.#field.get_value(f64::from(x) * 0.25, 0.0, f64::from(z) * 0.25) * 4.0 }
                } else {
                    quote! { noises.#field.get_value(f64::from(x) * 0.25, f64::from(y) * 0.25, f64::from(z) * 0.25) * 4.0 }
                }
            }

            DensityFunction::TwoArgumentSimple(t) => {
                let a = self.gen_expr(&t.argument1, input, is_flat);
                let b = self.gen_expr(&t.argument2, input, is_flat);
                match t.op {
                    TwoArgType::Add => quote! { ((#a) + (#b)) },
                    TwoArgType::Mul => quote! { ((#a) * (#b)) },
                    TwoArgType::Min => quote! { f64::min(#a, #b) },
                    TwoArgType::Max => quote! { f64::max(#a, #b) },
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
                let input_expr = self.gen_expr(&rc.input, input, is_flat);
                let in_range = self.gen_expr(&rc.when_in_range, input, is_flat);
                let out_range = self.gen_expr(&rc.when_out_of_range, input, is_flat);
                let min = Literal::f64_unsuffixed(rc.min_inclusive);
                let max = Literal::f64_unsuffixed(rc.max_exclusive);
                quote! {{
                    let v = #input_expr;
                    if v >= #min && v < #max { #in_range } else { #out_range }
                }}
            }

            DensityFunction::Spline(s) => self.gen_spline_expr(&s.spline, input, is_flat),

            DensityFunction::BlendedNoise(bn) => {
                let field = noise_field_ident("minecraft:offset");
                let xz = Literal::f64_unsuffixed(bn.xz_scale / bn.xz_factor);
                let ys = Literal::f64_unsuffixed(bn.y_scale / bn.y_factor);
                let smear = Literal::f64_unsuffixed(bn.smear_scale_multiplier);
                quote! { noises.#field.get_value(f64::from(x) * #xz, f64::from(y) * #ys, f64::from(z) * #xz) * #smear }
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
            DensityFunction::BlendDensity(bd) => self.gen_expr(&bd.input, input, is_flat),
            DensityFunction::EndIslands(_) => quote! { 0.0 },
            DensityFunction::Marker(m) => self.gen_expr(&m.wrapped, input, is_flat),

            DensityFunction::Reference(r) => {
                if self.flat_cached.contains(&r.id) {
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
    fn gen_spline_expr(
        &mut self,
        spline: &CubicSpline,
        input: &TranspilerInput,
        is_flat: bool,
    ) -> TokenStream {
        let coord = self.gen_expr(&spline.coordinate, input, is_flat);
        let n_points = spline.points.len();
        let n_lit = Literal::usize_unsuffixed(n_points);

        let locations: Vec<Literal> = spline
            .points
            .iter()
            .map(|p| Literal::f32_unsuffixed(p.location))
            .collect();
        let derivatives: Vec<Literal> = spline
            .points
            .iter()
            .map(|p| Literal::f32_unsuffixed(p.derivative))
            .collect();

        // Generate value expressions for each point
        let value_arms: Vec<TokenStream> = spline
            .points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let idx = Literal::usize_unsuffixed(i);
                let val_expr = match &p.value {
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
                };
                quote! { #idx => #val_expr }
            })
            .collect();

        quote! {{
            static LOCATIONS: [f32; #n_lit] = [#(#locations),*];
            static DERIVATIVES: [f32; #n_lit] = [#(#derivatives),*];
            let coord = (#coord) as f32;
            f64::from(spline_eval::evaluate_spline(&LOCATIONS, &DERIVATIVES, coord, |__i| {
                match __i {
                    #(#value_arms,)*
                    _ => unreachable!()
                }
            }))
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

        let params = if is_flat {
            quote! { noises: &OverworldNoises, cache: &OverworldColumnCache, x: i32, z: i32 }
        } else {
            quote! { noises: &OverworldNoises, cache: &OverworldColumnCache, x: i32, y: i32, z: i32 }
        };

        self.spline_fns.push(quote! {
            #[inline]
            fn #fn_name(#params) -> f32 {
                (#body) as f32
            }
        });

        fn_name
    }
}

// ── Helper functions ────────────────────────────────────────────────────────

/// Check if a density function subtree directly uses the `y` coordinate.
/// Does NOT recurse into References (those are handled by the flat inference loop).
fn uses_y(df: &DensityFunction) -> bool {
    match df {
        DensityFunction::YClampedGradient(_) => true,
        DensityFunction::Noise(n) => n.y_scale != 0.0,
        DensityFunction::ShiftedNoise(sn) => sn.y_scale != 0.0 || uses_y(&sn.shift_y),
        DensityFunction::Shift(_) => true, // uses y * 0.25
        DensityFunction::BlendedNoise(_) => true,
        DensityFunction::WeirdScaledSampler(ws) => uses_y(&ws.input),
        DensityFunction::TwoArgumentSimple(t) => uses_y(&t.argument1) || uses_y(&t.argument2),
        DensityFunction::Mapped(m) => uses_y(&m.input),
        DensityFunction::Clamp(c) => uses_y(&c.input),
        DensityFunction::RangeChoice(rc) => {
            uses_y(&rc.input) || uses_y(&rc.when_in_range) || uses_y(&rc.when_out_of_range)
        }
        DensityFunction::BlendDensity(bd) => uses_y(&bd.input),
        DensityFunction::Marker(m) => uses_y(&m.wrapped),
        DensityFunction::Spline(s) => uses_y_spline(&s.spline),
        // References are handled at the analysis level, not here
        DensityFunction::Reference(_) => false,
        // These don't use y
        DensityFunction::Constant(_)
        | DensityFunction::ShiftA(_)
        | DensityFunction::ShiftB(_)
        | DensityFunction::BlendAlpha(_)
        | DensityFunction::BlendOffset(_)
        | DensityFunction::EndIslands(_) => false,
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
        DensityFunction::Reference(r)
            if !refs.contains(&r.id) => {
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
        DensityFunction::ShiftedNoise(sn) => {
            collect_refs_inner(&sn.shift_x, refs);
            collect_refs_inner(&sn.shift_y, refs);
            collect_refs_inner(&sn.shift_z, refs);
        }
        DensityFunction::BlendDensity(bd) => collect_refs_inner(&bd.input, refs),
        DensityFunction::WeirdScaledSampler(ws) => collect_refs_inner(&ws.input, refs),
        DensityFunction::Spline(s) => collect_spline_refs(&s.spline, refs),
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

fn noise_field_ident(noise_id: &str) -> Ident {
    format_ident!("n_{}", sanitize_name(noise_id))
}

fn named_fn_field_ident(name: &str) -> Ident {
    format_ident!("df_{}", sanitize_name(name))
}

fn named_fn_ident(name: &str) -> Ident {
    format_ident!("compute_{}", sanitize_name(name))
}

/// `"minecraft:overworld/continents"` → `"overworld__continents"`
fn sanitize_name(id: &str) -> String {
    let stripped = id.strip_prefix("minecraft:").unwrap_or(id);
    stripped.replace('/', "__").replace('-', "_")
}
