//! Surface rule JSON parsing and transpilation.
//!
//! Parses surface rule trees from `noise_settings/{dimension}.json` and generates
//! a `try_apply_surface_rule()` function per dimension that inlines all conditions
//! and block outputs as Rust code.

use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

// ── JSON types ──────────────────────────────────────────────────────────────

/// Surface rule source (top-level rule node).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum SurfaceRuleJson {
    #[serde(rename = "minecraft:block")]
    Block { result_state: ResultStateJson },
    #[serde(rename = "minecraft:sequence")]
    Sequence { sequence: Vec<SurfaceRuleJson> },
    #[serde(rename = "minecraft:condition")]
    Condition {
        if_true: SurfaceConditionJson,
        then_run: Box<SurfaceRuleJson>,
    },
    #[serde(rename = "minecraft:bandlands")]
    Bandlands {},
}

/// Block state reference in a surface rule.
///
/// Currently only uses the block name (all vanilla surface rule blocks use
/// default state). If modded surface rules need non-default block states,
/// add a `Properties` field and wire it through the transpiler.
#[derive(Debug, Clone, Deserialize)]
pub struct ResultStateJson {
    #[serde(rename = "Name")]
    pub name: String,
}

/// Surface rule condition.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum SurfaceConditionJson {
    #[serde(rename = "minecraft:stone_depth")]
    StoneDepth {
        offset: i32,
        add_surface_depth: bool,
        secondary_depth_range: i32,
        surface_type: String,
    },
    #[serde(rename = "minecraft:above_preliminary_surface")]
    AbovePreliminarySurface {},
    #[serde(rename = "minecraft:biome")]
    BiomeIs { biome_is: Vec<BiomeIdJson> },
    #[serde(rename = "minecraft:noise_threshold")]
    NoiseThreshold {
        noise: String,
        min_threshold: f64,
        max_threshold: f64,
    },
    #[serde(rename = "minecraft:vertical_gradient")]
    VerticalGradient {
        random_name: String,
        true_at_and_below: VerticalAnchorJson,
        false_at_and_above: VerticalAnchorJson,
    },
    #[serde(rename = "minecraft:y_above")]
    YAbove {
        anchor: VerticalAnchorJson,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    #[serde(rename = "minecraft:water")]
    Water {
        offset: i32,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    #[serde(rename = "minecraft:temperature")]
    Temperature {},
    #[serde(rename = "minecraft:steep")]
    Steep {},
    #[serde(rename = "minecraft:hole")]
    Hole {},
    #[serde(rename = "minecraft:not")]
    Not { invert: Box<SurfaceConditionJson> },
}

/// Biome reference — plain string biome ID.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct BiomeIdJson(String);

impl BiomeIdJson {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Vertical anchor for Y-level resolution.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum VerticalAnchorJson {
    Absolute { absolute: i32 },
    AboveBottom { above_bottom: i32 },
    BelowTop { below_top: i32 },
}

// ── Transpiler ──────────────────────────────────────────────────────────────

/// Context for surface rule transpilation.
pub struct SurfaceRuleTranspiler {
    /// Collected noise IDs referenced by NoiseThreshold conditions.
    pub noise_ids: Vec<String>,
    /// Unique biome names for generating cached `OnceLock<u16>` statics.
    pub biome_names: Vec<String>,
    /// Min Y for this dimension.
    min_y: i32,
    /// Height for this dimension.
    height: i32,
}

impl SurfaceRuleTranspiler {
    pub fn new(min_y: i32, height: i32) -> Self {
        Self {
            noise_ids: Vec::new(),
            biome_names: Vec::new(),
            min_y,
            height,
        }
    }

    /// Transpile a surface rule tree into a Rust function body.
    ///
    /// Generated code references `ctx: &SurfaceRuleContext` from steel_utils.
    pub fn transpile_rule(&mut self, rule: &SurfaceRuleJson) -> TokenStream {
        match rule {
            SurfaceRuleJson::Block { result_state } => {
                let block_name = result_state
                    .name
                    .strip_prefix("minecraft:")
                    .unwrap_or(&result_state.name);
                let ident = Ident::new(&block_name.to_uppercase(), Span::call_site());
                quote! {
                    return Some(crate::vanilla_blocks::#ident.default_state());
                }
            }
            SurfaceRuleJson::Sequence { sequence } => {
                let stmts: Vec<_> = sequence.iter().map(|r| self.transpile_rule(r)).collect();
                quote! { #(#stmts)* }
            }
            SurfaceRuleJson::Condition { if_true, then_run } => {
                let cond = self.transpile_condition(if_true);
                let body = self.transpile_rule(then_run);
                quote! {
                    if #cond {
                        #body
                    }
                }
            }
            SurfaceRuleJson::Bandlands {} => {
                quote! {
                    return Some(ctx.system.get_band(ctx.block_x, ctx.block_y, ctx.block_z));
                }
            }
        }
    }

    /// Transpile a condition into a boolean expression.
    fn transpile_condition(&mut self, cond: &SurfaceConditionJson) -> TokenStream {
        match cond {
            SurfaceConditionJson::StoneDepth {
                offset,
                add_surface_depth,
                secondary_depth_range,
                surface_type,
            } => {
                let is_floor = surface_type == "floor";
                let depth_field = if is_floor {
                    quote! { ctx.stone_depth_above }
                } else {
                    quote! { ctx.stone_depth_below }
                };

                if *secondary_depth_range > 0 {
                    let range = *secondary_depth_range;
                    if *add_surface_depth {
                        quote! {
                            {
                                let extra = ((ctx.surface_secondary + 1.0) / 2.0 * #range as f64) as i32;
                                #depth_field <= 1 + #offset + ctx.surface_depth + extra
                            }
                        }
                    } else {
                        quote! {
                            {
                                let extra = ((ctx.surface_secondary + 1.0) / 2.0 * #range as f64) as i32;
                                #depth_field <= 1 + #offset + extra
                            }
                        }
                    }
                } else if *add_surface_depth {
                    quote! { #depth_field <= 1 + #offset + ctx.surface_depth }
                } else {
                    quote! { #depth_field <= 1 + #offset }
                }
            }
            SurfaceConditionJson::AbovePreliminarySurface {} => {
                quote! { ctx.block_y >= ctx.min_surface_level }
            }
            SurfaceConditionJson::BiomeIs { biome_is } => {
                let checks: Vec<_> = biome_is
                    .iter()
                    .map(|b| {
                        let biome_name = b
                            .as_str()
                            .strip_prefix("minecraft:")
                            .unwrap_or(b.as_str());
                        let upper = biome_name.to_uppercase();
                        if !self.biome_names.contains(&upper) {
                            self.biome_names.push(upper.clone());
                        }
                        let static_name = Ident::new(
                            &format!("BIOME_ID_{upper}"),
                            Span::call_site(),
                        );
                        let biome_ident = Ident::new(&upper, Span::call_site());
                        quote! { ctx.biome_id == *#static_name.get_or_init(|| crate::RegistryEntry::id(&*crate::vanilla_biomes::#biome_ident) as u16) }
                    })
                    .collect();
                if checks.len() == 1 {
                    checks.into_iter().next().unwrap()
                } else {
                    quote! { ( #(#checks)||* ) }
                }
            }
            SurfaceConditionJson::NoiseThreshold {
                noise,
                min_threshold,
                max_threshold,
            } => {
                let noise_key = noise.clone();
                let noise_index =
                    if let Some(idx) = self.noise_ids.iter().position(|k| k == &noise_key) {
                        idx
                    } else {
                        let idx = self.noise_ids.len();
                        self.noise_ids.push(noise_key);
                        idx
                    };
                let min_f = *min_threshold;
                let max_f = *max_threshold;
                quote! {
                    {
                        let v = ctx.system.get_noise(#noise_index, ctx.block_x, ctx.block_z);
                        v >= #min_f && v <= #max_f
                    }
                }
            }
            SurfaceConditionJson::VerticalGradient {
                random_name,
                true_at_and_below,
                false_at_and_above,
            } => {
                let true_y = self.resolve_anchor(true_at_and_below);
                let false_y = self.resolve_anchor(false_at_and_above);
                let name_lit = random_name.as_str();
                quote! {
                    {
                        const NAME_HASH: steel_utils::random::name_hash::NameHash = steel_utils::random::name_hash::NameHash::new(#name_lit);
                        ctx.system.vertical_gradient(&NAME_HASH, ctx.block_x, ctx.block_y, ctx.block_z, #true_y, #false_y)
                    }
                }
            }
            SurfaceConditionJson::YAbove {
                anchor,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                // Vanilla: blockY + (addStoneDepth ? stoneDepthAbove : 0)
                //            >= anchor + surfaceDepth * multiplier
                let anchor_y = self.resolve_anchor(anchor);
                let mul = *surface_depth_multiplier;
                if *add_stone_depth {
                    quote! {
                        ctx.block_y + ctx.stone_depth_above >= #anchor_y + ctx.surface_depth * #mul
                    }
                } else {
                    quote! {
                        ctx.block_y >= #anchor_y + ctx.surface_depth * #mul
                    }
                }
            }
            SurfaceConditionJson::Water {
                offset,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                // Vanilla: waterHeight == MIN_VALUE
                //   || blockY + (addStoneDepth ? stoneDepthAbove : 0)
                //        >= waterHeight + offset + surfaceDepth * multiplier
                let mul = *surface_depth_multiplier;
                if *add_stone_depth {
                    quote! {
                        ctx.water_height == i32::MIN
                            || ctx.block_y + ctx.stone_depth_above >= ctx.water_height + #offset + ctx.surface_depth * #mul
                    }
                } else {
                    quote! {
                        ctx.water_height == i32::MIN
                            || ctx.block_y >= ctx.water_height + #offset + ctx.surface_depth * #mul
                    }
                }
            }
            SurfaceConditionJson::Temperature {} => {
                quote! { ctx.cold_enough_to_snow }
            }
            SurfaceConditionJson::Steep {} => {
                quote! { ctx.steep }
            }
            SurfaceConditionJson::Hole {} => {
                quote! { ctx.surface_depth <= 0 }
            }
            SurfaceConditionJson::Not { invert } => {
                let inner = self.transpile_condition(invert);
                quote! { !(#inner) }
            }
        }
    }

    /// Resolve a vertical anchor to a constant Y value.
    fn resolve_anchor(&self, anchor: &VerticalAnchorJson) -> i32 {
        match anchor {
            VerticalAnchorJson::Absolute { absolute } => *absolute,
            VerticalAnchorJson::AboveBottom { above_bottom } => self.min_y + above_bottom,
            VerticalAnchorJson::BelowTop { below_top } => self.height - 1 + self.min_y - below_top,
        }
    }
}

/// Generate the complete `try_apply_surface_rule` function for a dimension.
///
/// Returns the function token stream and the list of noise IDs needed.
pub fn generate_surface_rule_function(
    rule: &SurfaceRuleJson,
    min_y: i32,
    height: i32,
) -> (TokenStream, Vec<String>) {
    let mut transpiler = SurfaceRuleTranspiler::new(min_y, height);
    let body = transpiler.transpile_rule(rule);
    let noise_ids = transpiler.noise_ids.clone();

    let biome_statics: Vec<_> = transpiler
        .biome_names
        .iter()
        .map(|name| {
            let static_name = Ident::new(&format!("BIOME_ID_{name}"), Span::call_site());
            quote! {
                static #static_name: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
            }
        })
        .collect();

    let func = quote! {
        /// Apply this dimension's surface rule at the current context position.
        #[allow(clippy::collapsible_if, clippy::needless_return, clippy::erasing_op, unused_comparisons)]
        fn apply_surface_rule_impl(
            ctx: &steel_utils::surface::SurfaceRuleContext<'_>,
        ) -> Option<steel_utils::BlockStateId> {
            #(#biome_statics)*
            #body
            None
        }
    };

    (func, noise_ids)
}
