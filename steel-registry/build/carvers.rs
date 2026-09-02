//! Build-time codegen for `ConfiguredCarver` statics.
//!
//! Reads `build_assets/builtin_datapacks/minecraft/worldgen/carver/*.json`
//! and emits a `pub static` per carver plus a `register_carvers` fn.
//!
//! Vanilla flattened the old config wrapper into `CaveWorldCarver`/
//! `CanyonWorldCarver` directly, dropped the separate `nether_cave` type
//! (now just a differently-configured `cave`), and moved `lava_level`/
//! `replaceable` out to a fixed Aquifer floor + the global
//! `#minecraft:uncarvable` tag (handled in steel-core).

use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::value_providers::{
    FloatProvider, HeightProvider, IntProvider, VerticalAnchor, WeightedIntProvider,
};

// ── JSON-facing structs ─────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct CarverJson {
    #[serde(rename = "type")]
    carver_type: String,
}

/// Fields common to `CaveWorldCarver`/`CanyonWorldCarver`. `carver_type` is
/// re-read here (flattened into the structs below) so the type-specific
/// parse pass doesn't reject `type` as unknown.
#[derive(Deserialize, Debug)]
struct CarverBaseJson {
    #[serde(rename = "type")]
    #[expect(
        dead_code,
        reason = "consumed only so deny_unknown_fields accepts `type`"
    )]
    carver_type: String,
    probability: f32,
    y: HeightProvider,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct CaveConfigJson {
    #[serde(flatten)]
    base: CarverBaseJson,
    count: IntProvider,
    thickness: FloatProvider,
    #[serde(default)]
    weird_thickness_bias: bool,
    room_vertical_radius_multiplier: FloatProvider,
    horizontal_radius_multiplier: FloatProvider,
    vertical_radius_multiplier: FloatProvider,
    #[serde(default = "default_start_vertical_radius_multiplier")]
    start_vertical_radius_multiplier: FloatProvider,
    floor_level: FloatProvider,
}

fn default_start_vertical_radius_multiplier() -> FloatProvider {
    FloatProvider::Constant(1.0)
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct CanyonShapeJson {
    distance_factor: FloatProvider,
    thickness: FloatProvider,
    width_smoothness: i32,
    horizontal_radius_factor: FloatProvider,
    vertical_radius_default_factor: f32,
    vertical_radius_center_factor: f32,
    y_scale: FloatProvider,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct CanyonConfigJson {
    #[serde(flatten)]
    base: CarverBaseJson,
    vertical_rotation: FloatProvider,
    shape: CanyonShapeJson,
}

// ── Codegen helpers ─────────────────────────────────────────────────────────

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

fn generate_int_provider(i: &IntProvider) -> TokenStream {
    match i {
        IntProvider::Constant(v) => quote! { IntProvider::Constant(#v) },
        IntProvider::Uniform {
            min_inclusive,
            max_inclusive,
        } => quote! {
            IntProvider::Uniform {
                min_inclusive: #min_inclusive,
                max_inclusive: #max_inclusive,
            }
        },
        IntProvider::BiasedToBottom {
            min_inclusive,
            max_inclusive,
        } => quote! {
            IntProvider::BiasedToBottom {
                min_inclusive: #min_inclusive,
                max_inclusive: #max_inclusive,
            }
        },
        IntProvider::VeryBiasedToBottom {
            min_inclusive,
            max_inclusive,
            inner,
        } => quote! {
            IntProvider::VeryBiasedToBottom {
                min_inclusive: #min_inclusive,
                max_inclusive: #max_inclusive,
                inner: #inner,
            }
        },
        IntProvider::Trapezoid { min, max, plateau } => quote! {
            IntProvider::Trapezoid {
                min: #min,
                max: #max,
                plateau: #plateau,
            }
        },
        IntProvider::ClampedNormal {
            mean,
            deviation,
            min_inclusive,
            max_inclusive,
        } => quote! {
            IntProvider::ClampedNormal {
                mean: #mean,
                deviation: #deviation,
                min_inclusive: #min_inclusive,
                max_inclusive: #max_inclusive,
            }
        },
        IntProvider::Clamped {
            source,
            min_inclusive,
            max_inclusive,
        } => {
            let source = generate_int_provider(source);
            quote! {
                IntProvider::Clamped {
                    source: Box::new(#source),
                    min_inclusive: #min_inclusive,
                    max_inclusive: #max_inclusive,
                }
            }
        }
        IntProvider::WeightedList { distribution } => {
            let entries: Vec<TokenStream> = distribution
                .iter()
                .map(generate_weighted_int_provider)
                .collect();
            quote! {
                IntProvider::WeightedList {
                    distribution: vec![#(#entries),*],
                }
            }
        }
    }
}

fn generate_weighted_int_provider(w: &WeightedIntProvider) -> TokenStream {
    let data = generate_int_provider(&w.data);
    let weight = w.weight;
    quote! { WeightedIntProvider { data: #data, weight: #weight } }
}

fn generate_base(base: &CarverBaseJson) -> TokenStream {
    let probability = base.probability;
    let y = generate_height_provider(base.y);

    quote! {
        CarverConfiguration {
            probability: #probability,
            y: #y,
        }
    }
}

fn generate_cave_kind(cfg: &CaveConfigJson) -> TokenStream {
    let base = generate_base(&cfg.base);
    let count = generate_int_provider(&cfg.count);
    let thickness = generate_float_provider(cfg.thickness);
    let weird_thickness_bias = cfg.weird_thickness_bias;
    let room_vrm = generate_float_provider(cfg.room_vertical_radius_multiplier);
    let hrm = generate_float_provider(cfg.horizontal_radius_multiplier);
    let vrm = generate_float_provider(cfg.vertical_radius_multiplier);
    let start_vrm = generate_float_provider(cfg.start_vertical_radius_multiplier);
    let floor = generate_float_provider(cfg.floor_level);

    quote! {
        ConfiguredCarverKind::Cave(CaveCarverConfiguration {
            base: #base,
            count: #count,
            thickness: #thickness,
            weird_thickness_bias: #weird_thickness_bias,
            room_vertical_radius_multiplier: #room_vrm,
            horizontal_radius_multiplier: #hrm,
            vertical_radius_multiplier: #vrm,
            start_vertical_radius_multiplier: #start_vrm,
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
    let y_scale = generate_float_provider(cfg.shape.y_scale);

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
                y_scale: #y_scale,
            },
        })
    }
}

// ── Build entry point ───────────────────────────────────────────────────────

pub(crate) fn build() -> TokenStream {
    let dir = "../steel-utils/build_assets/builtin_datapacks/minecraft/worldgen/carver";
    println!("cargo:rerun-if-changed={dir}");

    let mut entries: Vec<(String, TokenStream)> = Vec::new();

    let mut files: Vec<_> = fs::read_dir(dir)
        .expect("carver dir missing")
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    // Sort for deterministic output
    files.sort_by_key(std::fs::DirEntry::file_name);

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
                let cfg: CaveConfigJson = serde_json::from_str(&content)
                    .unwrap_or_else(|e| panic!("failed to parse {name} cave config: {e}"));
                generate_cave_kind(&cfg)
            }
            "minecraft:canyon" => {
                let cfg: CanyonConfigJson = serde_json::from_str(&content)
                    .unwrap_or_else(|e| panic!("failed to parse {name} canyon config: {e}"));
                generate_canyon_kind(&cfg)
            }
            other => panic!("unknown carver type `{other}` in {name}.json"),
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
        use steel_utils::value_providers::{
            FloatProvider, HeightProvider, IntProvider, VerticalAnchor, WeightedIntProvider,
        };
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
