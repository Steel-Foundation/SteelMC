//! Surface rule JSON parsing and transpilation.
//!
//! Parses surface rule trees from `noise_settings/{dimension}.json` and generates
//! a `try_apply_surface_rule()` function per dimension that inlines all conditions
//! and block outputs as Rust code.

use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use std::{mem, slice};

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
    BiomeIs { biome_is: SingleOrList<BiomeIdJson> },
    #[serde(rename = "minecraft:noise_threshold")]
    NoiseThreshold {
        noise: String,
        #[serde(default)]
        is_3d: bool,
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

/// Vanilla holder-set JSON accepts either a single ID or a list of IDs.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum SingleOrList<T> {
    Single(T),
    List(Vec<T>),
}

impl<T> SingleOrList<T> {
    fn as_slice(&self) -> &[T] {
        match self {
            Self::Single(value) => slice::from_ref(value),
            Self::List(values) => values,
        }
    }
}

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
    /// Collected noise IDs referenced by `NoiseThreshold` conditions.
    pub noise_ids: Vec<String>,
    /// Collected random IDs referenced by `VerticalGradient` conditions.
    pub gradient_ids: Vec<String>,
    /// Collected block state names returned by block-result rules.
    pub block_state_names: Vec<String>,
    /// Whether generated conditions read `ctx.biome_id` directly or indirectly.
    pub uses_biome: bool,
    /// Whether generated conditions use `ctx.min_surface_level`.
    pub uses_preliminary_surface: bool,
    /// Whether generated conditions use `ctx.surface_secondary`.
    pub uses_surface_secondary: bool,
    /// Whether generated conditions use `ctx.steep`.
    pub uses_steep: bool,
    /// Min Y for this dimension.
    min_y: i32,
    /// Height for this dimension.
    height: i32,
}

impl SurfaceRuleTranspiler {
    pub const fn new(min_y: i32, height: i32, uses_preliminary_surface: bool) -> Self {
        Self {
            noise_ids: Vec::new(),
            gradient_ids: Vec::new(),
            block_state_names: Vec::new(),
            uses_biome: false,
            uses_preliminary_surface,
            uses_surface_secondary: false,
            uses_steep: false,
            min_y,
            height,
        }
    }

    /// Transpile a surface rule tree into a Rust function body.
    ///
    /// Generated code references `ctx: &mut SurfaceRuleContext` from `steel_utils`.
    pub fn transpile_rule(&mut self, rule: &SurfaceRuleJson) -> TokenStream {
        match rule {
            SurfaceRuleJson::Block { result_state } => {
                let block_name = result_state.name.as_str();
                let block_state_index = if let Some(idx) = self
                    .block_state_names
                    .iter()
                    .position(|name| name == block_name)
                {
                    idx
                } else {
                    let idx = self.block_state_names.len();
                    self.block_state_names.push(block_name.to_owned());
                    idx
                };
                quote! {
                    return Some(ctx.block_state(#block_state_index));
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
    #[expect(
        clippy::too_many_lines,
        reason = "surface condition variants are best kept in one dispatch function"
    )]
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
                    self.uses_surface_secondary = true;
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
                self.uses_preliminary_surface = true;
                quote! { ctx.block_y >= ctx.min_surface_level }
            }
            SurfaceConditionJson::BiomeIs { biome_is } => {
                self.uses_biome = true;
                let checks: Vec<_> = biome_is
                    .as_slice()
                    .iter()
                    .map(|b| {
                        let biome_name = b
                            .as_str()
                            .strip_prefix("minecraft:")
                            .unwrap_or(b.as_str());
                        let upper = biome_name.to_uppercase();
                        let biome_ident = Ident::new(&upper, Span::call_site());
                        quote! { biome_id == steel_registry::RegistryEntry::id(&*steel_registry::vanilla_biomes::#biome_ident) as u16 }
                    })
                    .collect();
                let check = if checks.is_empty() {
                    quote! { false }
                } else if checks.len() == 1 {
                    let mut checks = checks;
                    checks.remove(0)
                } else {
                    quote! { ( #(#checks)||* ) }
                };
                let biome_id = if self.uses_preliminary_surface {
                    quote! { ctx.biome_id() }
                } else {
                    quote! { ctx.known_biome_id() }
                };
                quote! { #biome_id.is_some_and(|biome_id| #check) }
            }
            SurfaceConditionJson::NoiseThreshold {
                noise,
                is_3d,
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
                let sample = if *is_3d {
                    quote! { ctx.condition_noise_3d(#noise_index) }
                } else {
                    quote! { ctx.condition_noise(#noise_index) }
                };
                quote! {
                    {
                        let v = #sample;
                        v >= #min_f && v <= #max_f
                    }
                }
            }
            SurfaceConditionJson::VerticalGradient {
                random_name,
                true_at_and_below,
                false_at_and_above,
            } => {
                let gradient_index =
                    if let Some(idx) = self.gradient_ids.iter().position(|id| id == random_name) {
                        idx
                    } else {
                        let idx = self.gradient_ids.len();
                        self.gradient_ids.push(random_name.to_owned());
                        idx
                    };
                let true_y = self.resolve_anchor(true_at_and_below);
                let false_y = self.resolve_anchor(false_at_and_above);
                quote! { ctx.system.vertical_gradient(#gradient_index, ctx.block_x, ctx.block_y, ctx.block_z, #true_y, #false_y) }
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
                self.uses_biome = true;
                quote! { ctx.cold_enough_to_snow() }
            }
            SurfaceConditionJson::Steep {} => {
                self.uses_steep = true;
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

fn rule_uses_preliminary_surface(rule: &SurfaceRuleJson) -> bool {
    match rule {
        SurfaceRuleJson::Block { .. } | SurfaceRuleJson::Bandlands {} => false,
        SurfaceRuleJson::Sequence { sequence } => {
            sequence.iter().any(rule_uses_preliminary_surface)
        }
        SurfaceRuleJson::Condition { if_true, then_run } => {
            condition_uses_preliminary_surface(if_true) || rule_uses_preliminary_surface(then_run)
        }
    }
}

fn condition_uses_preliminary_surface(condition: &SurfaceConditionJson) -> bool {
    match condition {
        SurfaceConditionJson::AbovePreliminarySurface {} => true,
        SurfaceConditionJson::Not { invert } => condition_uses_preliminary_surface(invert),
        SurfaceConditionJson::StoneDepth { .. }
        | SurfaceConditionJson::BiomeIs { .. }
        | SurfaceConditionJson::NoiseThreshold { .. }
        | SurfaceConditionJson::VerticalGradient { .. }
        | SurfaceConditionJson::YAbove { .. }
        | SurfaceConditionJson::Water { .. }
        | SurfaceConditionJson::Temperature {}
        | SurfaceConditionJson::Steep {}
        | SurfaceConditionJson::Hole {} => false,
    }
}

/// Generate the complete `try_apply_surface_rule` function for a dimension.
///
/// Returns the function token stream, condition noise IDs, and returned block states.
type SurfaceRuleFunctionArtifacts = (
    TokenStream,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    bool,
    bool,
    bool,
    bool,
);

pub fn generate_surface_rule_function(
    rule: &SurfaceRuleJson,
    min_y: i32,
    height: i32,
) -> SurfaceRuleFunctionArtifacts {
    let uses_preliminary_surface = rule_uses_preliminary_surface(rule);
    let mut transpiler = SurfaceRuleTranspiler::new(min_y, height, uses_preliminary_surface);
    let body = transpiler.transpile_rule(rule);
    let noise_ids = mem::take(&mut transpiler.noise_ids);
    let gradient_ids = mem::take(&mut transpiler.gradient_ids);
    let block_state_names = mem::take(&mut transpiler.block_state_names);
    let uses_biome = transpiler.uses_biome;
    let uses_preliminary_surface = transpiler.uses_preliminary_surface;
    let uses_surface_secondary = transpiler.uses_surface_secondary;
    let uses_steep = transpiler.uses_steep;

    let func = quote! {
        /// Apply this dimension's surface rule at the current context position.
        #[allow(clippy::collapsible_if, clippy::needless_return, clippy::erasing_op, unused_comparisons)]
        fn apply_surface_rule_impl(
            ctx: &mut steel_worldgen::surface::SurfaceRuleContext<'_>,
        ) -> Option<steel_utils::BlockStateId> {
            #body
            None
        }
    };

    (
        func,
        noise_ids,
        gradient_ids,
        block_state_names,
        uses_biome,
        uses_preliminary_surface,
        uses_surface_secondary,
        uses_steep,
    )
}
