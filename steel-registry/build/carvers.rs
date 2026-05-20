//! Build-time codegen for `ConfiguredCarver` statics.
//!
//! Reads `build_assets/builtin_datapacks/minecraft/worldgen/configured_carver/*.json`,
//! deserialises each via `steel_utils::value_providers` types, and emits
//! Rust source with a `pub static` per carver plus a `register_carvers` fn.

use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use serde_json::Value;
use steel_utils::Identifier;
use steel_utils::value_providers::{FloatProvider, HeightProvider, VerticalAnchor};

// ── JSON-facing structs ─────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct CarverJson {
    #[serde(rename = "type")]
    carver_type: String,
    config: Value,
}

#[derive(Deserialize, Debug)]
struct CarverConfigBaseJson {
    probability: f32,
    y: HeightProvider,
    #[serde(rename = "yScale")]
    y_scale: FloatProvider,
    lava_level: VerticalAnchor,
    replaceable: String,
    #[serde(default)]
    #[expect(dead_code, reason = "debug_settings parsed but ignored (see TODO)")]
    debug_settings: Option<Value>,
}

#[derive(Deserialize, Debug)]
struct CaveConfigJson {
    #[serde(flatten)]
    base: CarverConfigBaseJson,
    horizontal_radius_multiplier: FloatProvider,
    vertical_radius_multiplier: FloatProvider,
    floor_level: FloatProvider,
}

#[derive(Deserialize, Debug)]
struct CanyonShapeJson {
    distance_factor: FloatProvider,
    thickness: FloatProvider,
    width_smoothness: i32,
    horizontal_radius_factor: FloatProvider,
    vertical_radius_default_factor: f32,
    vertical_radius_center_factor: f32,
}

#[derive(Deserialize, Debug)]
struct CanyonConfigJson {
    #[serde(flatten)]
    base: CarverConfigBaseJson,
    vertical_rotation: FloatProvider,
    shape: CanyonShapeJson,
}

// ── Codegen helpers ─────────────────────────────────────────────────────────

fn generate_identifier(resource: &Identifier) -> TokenStream {
    let namespace = resource.namespace.as_ref();
    let path = resource.path.as_ref();
    quote! { Identifier { namespace: Cow::Borrowed(#namespace), path: Cow::Borrowed(#path) } }
}

fn generate_vertical_anchor(v: VerticalAnchor) -> TokenStream {
    match v {
        VerticalAnchor::Absolute(y) => quote! { VerticalAnchor::Absolute(#y) },
        VerticalAnchor::AboveBottom(o) => quote! { VerticalAnchor::AboveBottom(#o) },
        VerticalAnchor::BelowTop(o) => quote! { VerticalAnchor::BelowTop(#o) },
    }
}

fn generate_height_provider(h: HeightProvider) -> TokenStream {
    match h {
        HeightProvider::Constant(a) => {
            let anchor = generate_vertical_anchor(a);
            quote! { HeightProvider::Constant(#anchor) }
        }
        HeightProvider::Uniform {
            min_inclusive,
            max_inclusive,
        } => {
            let min = generate_vertical_anchor(min_inclusive);
            let max = generate_vertical_anchor(max_inclusive);
            quote! {
                HeightProvider::Uniform {
                    min_inclusive: #min,
                    max_inclusive: #max,
                }
            }
        }
        HeightProvider::Trapezoid {
            min_inclusive,
            max_inclusive,
            plateau,
        } => {
            let min = generate_vertical_anchor(min_inclusive);
            let max = generate_vertical_anchor(max_inclusive);
            quote! {
                HeightProvider::Trapezoid {
                    min_inclusive: #min,
                    max_inclusive: #max,
                    plateau: #plateau,
                }
            }
        }
        HeightProvider::BiasedToBottom {
            min_inclusive,
            max_inclusive,
            inner,
        } => {
            let min = generate_vertical_anchor(min_inclusive);
            let max = generate_vertical_anchor(max_inclusive);
            quote! {
                HeightProvider::BiasedToBottom {
                    min_inclusive: #min,
                    max_inclusive: #max,
                    inner: #inner,
                }
            }
        }
        HeightProvider::VeryBiasedToBottom {
            min_inclusive,
            max_inclusive,
            inner,
        } => {
            let min = generate_vertical_anchor(min_inclusive);
            let max = generate_vertical_anchor(max_inclusive);
            quote! {
                HeightProvider::VeryBiasedToBottom {
                    min_inclusive: #min,
                    max_inclusive: #max,
                    inner: #inner,
                }
            }
        }
    }
}

fn generate_float_provider(f: FloatProvider) -> TokenStream {
    match f {
        FloatProvider::Constant(v) => quote! { FloatProvider::Constant(#v) },
        FloatProvider::Uniform {
            min_inclusive,
            max_exclusive,
        } => quote! {
            FloatProvider::Uniform {
                min_inclusive: #min_inclusive,
                max_exclusive: #max_exclusive,
            }
        },
        FloatProvider::Trapezoid { min, max, plateau } => quote! {
            FloatProvider::Trapezoid {
                min: #min,
                max: #max,
                plateau: #plateau,
            }
        },
        FloatProvider::ClampedNormal {
            mean,
            deviation,
            min,
            max,
        } => quote! {
            FloatProvider::ClampedNormal {
                mean: #mean,
                deviation: #deviation,
                min: #min,
                max: #max,
            }
        },
    }
}

/// Parses a tag reference string like `#minecraft:overworld_carver_replaceables`
/// into the underlying tag [`Identifier`]. Non-tag (inline list) forms are
/// rejected — all vanilla carvers use tags.
fn parse_replaceable_tag(s: &str) -> Identifier {
    let stripped = s
        .strip_prefix('#')
        .unwrap_or_else(|| panic!("carver `replaceable` must be a `#tag` reference, got `{s}`"));
    let (ns, path) = stripped.split_once(':').unwrap_or(("minecraft", stripped));
    Identifier::new(ns.to_owned(), path.to_owned())
}

fn generate_base(base: &CarverConfigBaseJson) -> TokenStream {
    let probability = base.probability;
    let y = generate_height_provider(base.y);
    let y_scale = generate_float_provider(base.y_scale);
    let lava_level = generate_vertical_anchor(base.lava_level);
    let tag = generate_identifier(&parse_replaceable_tag(&base.replaceable));

    quote! {
        CarverConfiguration {
            probability: #probability,
            y: #y,
            y_scale: #y_scale,
            lava_level: #lava_level,
            replaceable_tag: #tag,
        }
    }
}

fn generate_cave_kind(kind_name: &str, cfg: &CaveConfigJson) -> TokenStream {
    let base = generate_base(&cfg.base);
    let hrm = generate_float_provider(cfg.horizontal_radius_multiplier);
    let vrm = generate_float_provider(cfg.vertical_radius_multiplier);
    let floor = generate_float_provider(cfg.floor_level);
    let kind_ident = Ident::new(kind_name, Span::call_site());

    quote! {
        ConfiguredCarverKind::#kind_ident(CaveCarverConfiguration {
            base: #base,
            horizontal_radius_multiplier: #hrm,
            vertical_radius_multiplier: #vrm,
            floor_level: #floor,
        })
    }
}

fn generate_canyon_kind(cfg: &CanyonConfigJson) -> TokenStream {
    let base = generate_base(&cfg.base);
    let vrot = generate_float_provider(cfg.vertical_rotation);
    let df = generate_float_provider(cfg.shape.distance_factor);
    let thick = generate_float_provider(cfg.shape.thickness);
    let ws = cfg.shape.width_smoothness;
    let hrf = generate_float_provider(cfg.shape.horizontal_radius_factor);
    let vrdf = cfg.shape.vertical_radius_default_factor;
    let vrcf = cfg.shape.vertical_radius_center_factor;

    quote! {
        ConfiguredCarverKind::Canyon(CanyonCarverConfiguration {
            base: #base,
            vertical_rotation: #vrot,
            shape: CanyonShapeConfiguration {
                distance_factor: #df,
                thickness: #thick,
                width_smoothness: #ws,
                horizontal_radius_factor: #hrf,
                vertical_radius_default_factor: #vrdf,
                vertical_radius_center_factor: #vrcf,
            },
        })
    }
}

// ── Build entry point ───────────────────────────────────────────────────────

pub(crate) fn build() -> TokenStream {
    let dir = "build_assets/builtin_datapacks/minecraft/worldgen/configured_carver";
    println!("cargo:rerun-if-changed={dir}");

    let mut entries: Vec<(String, TokenStream)> = Vec::new();

    let mut files: Vec<_> = fs::read_dir(dir)
        .expect("configured_carver dir missing")
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    // Sort for deterministic output
    files.sort_by_key(|e| e.file_name());

    for entry in files {
        let path = entry.path();
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("invalid carver file name")
            .to_string();
        let content =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {name}.json: {e}"));
        let raw: CarverJson = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("failed to parse {name}.json: {e}"));

        let kind = match raw.carver_type.as_str() {
            "minecraft:cave" => {
                let cfg: CaveConfigJson = serde_json::from_value(raw.config)
                    .unwrap_or_else(|e| panic!("failed to parse {name} cave config: {e}"));
                generate_cave_kind("Cave", &cfg)
            }
            "minecraft:nether_cave" => {
                let cfg: CaveConfigJson = serde_json::from_value(raw.config)
                    .unwrap_or_else(|e| panic!("failed to parse {name} nether_cave config: {e}"));
                generate_cave_kind("NetherCave", &cfg)
            }
            "minecraft:canyon" => {
                let cfg: CanyonConfigJson = serde_json::from_value(raw.config)
                    .unwrap_or_else(|e| panic!("failed to parse {name} canyon config: {e}"));
                generate_canyon_kind(&cfg)
            }
            other => panic!("unknown configured_carver type `{other}` in {name}.json"),
        };

        entries.push((name, kind));
    }

    let mut stream = TokenStream::new();
    stream.extend(quote! {
        use crate::carver::{
            CanyonCarverConfiguration, CanyonShapeConfiguration, CarverConfiguration,
            CaveCarverConfiguration, ConfiguredCarver, ConfiguredCarverKind,
            ConfiguredCarverRegistry,
        };
        use steel_utils::Identifier;
        use steel_utils::value_providers::{FloatProvider, HeightProvider, VerticalAnchor};
        use std::borrow::Cow;
        use std::sync::{LazyLock, OnceLock};
    });

    let mut register = TokenStream::new();
    for (name, kind) in &entries {
        let ident = Ident::new(&name.to_shouty_snake_case(), Span::call_site());
        let key = quote! { Identifier::vanilla_static(#name) };
        stream.extend(quote! {
            pub static #ident: LazyLock<ConfiguredCarver> = LazyLock::new(|| ConfiguredCarver {
                key: #key,
                kind: #kind,
                id: OnceLock::new(),
            });
        });
        register.extend(quote! {
            registry.register(&#ident);
        });
    }

    stream.extend(quote! {
        pub fn register_configured_carvers(registry: &mut ConfiguredCarverRegistry) {
            #register
        }
    });

    stream
}
