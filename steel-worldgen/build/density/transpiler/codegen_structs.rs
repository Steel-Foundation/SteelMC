//! Codegen for runtime support structs: `{Prefix}Noises` and `{Prefix}ColumnCache`.
//!
//! Emits noise generator fields, seed-based constructors (including legacy-random
//! overrides), and the per-column flat-cache with optional grid backing stores.

use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};

use super::TranspilerInput;
use super::context::TranspileContext;
use super::naming::{
    grid_field_ident, named_fn_field_ident, named_fn_ident, noise_field_ident,
    router_cache_field_ident, router_compute_fn_ident, router_grid_field_ident,
};

impl TranspileContext {
    pub(super) fn gen_noises_struct(&self) -> TokenStream {
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

    pub(super) fn gen_noises_impl(&self) -> TokenStream {
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
    pub(super) fn gen_column_cache(&mut self, input: &TranspilerInput) -> TokenStream {
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
                        x * #scale, z * #scale,
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
                            let x = ((self.grid_first_quart_x + rel_x) << 2) as f64;
                            let z = ((self.grid_first_quart_z + rel_z) << 2) as f64;
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
                        let x = x as f64;
                        let z = z as f64;
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
                    let x = eval_x as f64;
                    let z = eval_z as f64;
                    #(#ensure_stmts)*
                    #(#router_ensure_stmts)*
                    #(#inline_noise_ensure_stmts)*
                    self.valid = true;
                }
            }
        }
    }

    /// Generate the function parameter list for a density function.
    pub(super) fn fn_params(&self, is_flat: bool) -> TokenStream {
        let noises = &self.noises_ident;
        let cache = &self.cache_ident;
        if is_flat {
            quote! { noises: &#noises, cache: &#cache, x: f64, z: f64 }
        } else {
            quote! { noises: &#noises, cache: &#cache, x: f64, y: f64, z: f64 }
        }
    }

    /// Generate the SIMD (4-Y batched) parameter list for a non-flat density
    /// function. Flat functions don't have a 4x form (callers splat from cache).
    pub(super) fn fn_params_4x(&self) -> TokenStream {
        let noises = &self.noises_ident;
        let cache = &self.cache_ident;
        quote! { noises: &#noises, cache: &#cache, x: f64, ys: f64x4, z: f64 }
    }

    /// Generate the function parameter list for a router entry point.
    /// Router functions read x/z from the cache, so flat variants omit explicit coords.
    pub(super) fn fn_params_router(&self, is_flat: bool) -> TokenStream {
        let noises = &self.noises_ident;
        let cache = &self.cache_ident;
        if is_flat {
            quote! { noises: &#noises, cache: &#cache }
        } else {
            quote! { noises: &#noises, cache: &#cache, x: f64, y: f64, z: f64 }
        }
    }
}
