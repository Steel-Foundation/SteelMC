//! Core density expression codegen (scalar and SIMD).
//!
//! `gen_expr` and `gen_expr_simd` dispatch on each `DensityFunction` variant to
//! emit inline Rust. Handles CSE hoisting, spline helpers, interpolated-parameter
//! rewriting, and scalar fallbacks for SIMD paths.

use std::collections::BTreeSet;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use proc_macro2::{Ident, Literal, TokenStream};
use quote::{format_ident, quote};

use crate::density::{
    CubicSpline, DensityFunction, MappedType, MarkerType, RarityValueMapper, SplineValue,
    TwoArgType,
};

use super::TranspilerInput;
use super::bounds::compute_bounds;
use super::context::TranspileContext;
use super::fingerprint::{collect_expensive_subexprs, fingerprint, is_cse_candidate};
use super::naming::{named_fn_field_ident, named_fn_ident, named_fn_ident_4x, noise_field_ident};

impl TranspileContext {
    #[expect(
        clippy::too_many_lines,
        reason = "one match arm per DensityFunction variant; splitting the dispatch would obscure the per-variant codegen"
    )]
    pub(super) fn gen_expr(
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
                quote! { map_clamped(y, #from_y, #to_y, #from_val, #to_val) }
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
                    quote! { noises.#field.get_value_xz(x * #xz_scale, z * #xz_scale) }
                } else {
                    quote! { noises.#field.get_value(x * #xz_scale, y * #y_scale, z * #xz_scale) }
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
                            x * #xz_scale + dx,
                            z * #xz_scale + dz,

                        )
                    }}
                } else {
                    quote! {{
                        let dx = #dx;
                        let dy = #dy;
                        let dz = #dz;
                        noises.#field.get_value(
                            x * #xz_scale + dx,
                            y * #y_scale + dy,
                            z * #xz_scale + dz,
                        )
                    }}
                }
            }

            DensityFunction::ShiftA(s) => {
                let field = noise_field_ident(&s.noise_id);
                quote! { noises.#field.get_value_xz(x * 0.25, z * 0.25) * 4.0 }
            }

            DensityFunction::ShiftB(s) => {
                let field = noise_field_ident(&s.noise_id);
                quote! { noises.#field.get_value_xy(z * 0.25, x * 0.25) * 4.0 }
            }

            DensityFunction::Shift(s) => {
                let field = noise_field_ident(&s.noise_id);
                if is_flat {
                    quote! { noises.#field.get_value_xz(x * 0.25, z * 0.25) * 4.0 }
                } else {
                    quote! { noises.#field.get_value(x * 0.25, y * 0.25, z * 0.25) * 4.0 }
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
                        x / scale, y / scale, z / scale,
                    ).abs()
                }}
            }

            DensityFunction::BlendAlpha(_) => quote! { 1.0 },
            DensityFunction::BlendOffset(_) => quote! { 0.0 },
            // EndIslands ignores y internally, so we can pass 0 in flat contexts
            DensityFunction::EndIslands => {
                if is_flat {
                    quote! { noises.end_islands.sample(x, 0.0, z) }
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
                let cell_height = Literal::f64_unsuffixed(f64::from(fts.cell_height));
                let lower_bound = Literal::f64_unsuffixed(f64::from(fts.lower_bound));
                quote! {{
                    let __upper = #upper_expr;
                    let __top_y = (__upper / #cell_height).floor() * #cell_height;
                    if __top_y <= #lower_bound {
                        #lower_bound
                    } else {
                        let mut __result = #lower_bound;
                        let mut y = __top_y;
                        while y >= #lower_bound {
                            let __d = #density_expr;
                            if __d > 0.0 {
                                __result = y;
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
    pub(super) fn gen_spline_expr(
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
    pub(super) fn gen_spline_helper(
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
    pub(super) fn gen_expr_simd(
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
                            x * #xz_scale, z * #xz_scale,
                        ))
                    }
                } else {
                    quote! {
                        noises.#field.get_value_y_simd(
                            x * #xz_scale,
                            ys * f64x4::splat(#y_scale),
                            z * #xz_scale,
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
                        x * 0.25, z * 0.25,
                    ) * 4.0)
                }
            }

            DensityFunction::ShiftB(s) => {
                let field = noise_field_ident(&s.noise_id);
                quote! {
                    f64x4::splat(noises.#field.get_value_xy(
                        z * 0.25, x * 0.25,
                    ) * 4.0)
                }
            }

            DensityFunction::Shift(s) => {
                let field = noise_field_ident(&s.noise_id);
                quote! {
                    noises.#field.get_value_y_simd(
                        x * 0.25,
                        ys * f64x4::splat(0.25),
                        z * 0.25,
                    ) * f64x4::splat(4.0)
                }
            }

            DensityFunction::ShiftedNoise(sn) => {
                // `sn.shift_*` are themselves density functions evaluated at
                // (x, y, z). When all three shifts are Y-independent (typical
                // vanilla case — they're flat-cached `shift_x`/`shift_z` and
                // a constant `shift_y`), evaluate them as scalar splats and
                // call `get_value_y_simd(`. Otherwise fall back to scalar 4×.
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
                                x * #xz_scale + dx,
                                z * #xz_scale + dz,
                            ))
                        }}
                    } else {
                        quote! {{
                            let dx = #dx;
                            let dy = #dy;
                            let dz = #dz;
                            noises.#field.get_value_y_simd(
                                x * #xz_scale + dx,
                                ys * f64x4::splat(#y_scale) + f64x4::splat(dy),
                                z * #xz_scale + dz,
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
                        let y = __ys_arr[#i_lit];
                        scale * noises.#field.get_value(
                            x / scale,
                            y / scale,
                            z / scale,
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
                quote! { f64x4::splat(noises.end_islands.sample(x, 0.0, z)) }
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
    pub(super) fn gen_simd_scalar_fallback(
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
                let y = __ys_arr[#i_lit];
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
    pub(super) fn is_y_independent(&self, df: &DensityFunction) -> bool {
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

    pub(super) fn is_spline_y_independent(&self, spline: &CubicSpline) -> bool {
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
    pub(super) fn hoist_common_subexprs(
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
    pub(super) fn hoist_common_subexprs_simd(
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
