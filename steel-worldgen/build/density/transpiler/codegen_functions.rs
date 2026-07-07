//! Codegen for named and router density functions.
//!
//! Emits private `compute_*` functions in topological order, public `router_*`
//! entry points, and the interpolation fill/combine helpers for
//! `final_density` / vein router channels.

use std::collections::BTreeMap;
use std::mem;

use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};

use crate::density::DensityFunction;

use super::TranspilerInput;
use super::context::TranspileContext;
use super::graph::{collect_interpolated_inners, is_flat_cached, unwrap_markers};
use super::naming::{
    named_fn_ident, named_fn_ident_4x, router_cache_field_ident, router_compute_fn_ident,
    sanitize_name,
};

impl TranspileContext {
    pub(super) fn gen_named_functions(&mut self, input: &TranspilerInput) -> TokenStream {
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
    pub(super) fn gen_router_functions(&mut self, input: &TranspilerInput) -> TokenStream {
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
                        let x = cache.x as f64;
                        let z = cache.z as f64;
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
    pub(super) fn gen_all_interpolation_functions(
        &mut self,
        input: &TranspilerInput,
    ) -> TokenStream {
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
                let x = cache.x as f64;
                let z = cache.z as f64;
                let y = y as f64;
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
                let x = cache.x as f64;
                let z = cache.z as f64;
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
                let x = cache.x as f64;
                let z = cache.z as f64;
                let y = y as f64;
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
                let x = cache.x as f64;
                let z = cache.z as f64;
                let y = y as f64;
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
                let x = cache.x as f64;
                let z = cache.z as f64;
                let y = y as f64;
                #combine_vein_ridged_body
            }

            #(#fill_spline_fns)*
            #(#fill_spline_fns_4x)*
            #(#combine_fd_splines)*
            #(#combine_vein_toggle_splines)*
            #(#combine_vein_ridged_splines)*
        }
    }
}
