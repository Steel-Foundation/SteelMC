use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, Debug)]
pub struct TimelineJson {
    clock: Option<String>,
    period_ticks: Option<i64>,
    #[serde(default)]
    tracks: serde_json::Map<String, Value>,
    #[serde(default)]
    time_markers: serde_json::Map<String, Value>,
}

#[derive(Deserialize)]
struct TrackJson {
    ease: Option<Value>,
    modifier: Option<String>,
    keyframes: Vec<KeyframeJson>,
}

#[derive(Deserialize)]
struct KeyframeJson {
    ticks: i64,
    value: Value,
}

fn quote_opt_identifier(s: &str) -> TokenStream {
    let (namespace, path) = s.split_once(':').expect("Identifier missing ':'");
    assert_eq!(
        namespace, "minecraft",
        "Expected minecraft namespace in: {}",
        s
    );
    quote! { Some(Identifier::vanilla_static(#path)) }
}

fn quote_ease(v: &Value) -> TokenStream {
    match v {
        Value::String(s) => {
            let s_str = s.as_str();
            quote! { Some(Ease::Named(#s_str)) }
        }
        Value::Object(obj) => {
            if let Some(Value::Array(vals)) = obj.get("cubic_bezier") {
                let a = vals[0].as_f64().unwrap() as f32;
                let b = vals[1].as_f64().unwrap() as f32;
                let c = vals[2].as_f64().unwrap() as f32;
                let d = vals[3].as_f64().unwrap() as f32;
                quote! { Some(Ease::CubicBezier([#a, #b, #c, #d])) }
            } else {
                quote! { None }
            }
        }
        _ => quote! { None },
    }
}

fn quote_keyframe_value(v: &Value) -> TokenStream {
    match v {
        Value::Bool(b) => {
            if *b {
                quote! { KeyframeValue::Bool(true) }
            } else {
                quote! { KeyframeValue::Bool(false) }
            }
        }
        Value::String(s) => {
            let s_str = s.as_str();
            quote! { KeyframeValue::String(#s_str) }
        }
        Value::Number(n) => {
            if n.is_f64() && !n.is_i64() && !n.is_u64() {
                let f = n.as_f64().unwrap() as f32;
                quote! { KeyframeValue::Float(#f) }
            } else {
                let i = n.as_i64().unwrap_or_else(|| n.as_u64().unwrap() as i64) as i32;
                if i < 0 {
                    let abs = i.unsigned_abs();
                    let abs_lit = Literal::u32_unsuffixed(abs);
                    quote! { KeyframeValue::Int(- #abs_lit as i32) }
                } else {
                    let lit = Literal::i32_suffixed(i);
                    quote! { KeyframeValue::Int(#lit) }
                }
            }
        }
        _ => panic!("Unexpected keyframe value: {:?}", v),
    }
}

fn quote_time_marker(name: &str, v: &Value) -> TokenStream {
    match v {
        Value::Number(n) => {
            let ticks = n.as_i64().unwrap_or_else(|| n.as_u64().unwrap() as i64);
            quote! {
                TimeMarker {
                    name: #name,
                    ticks: #ticks,
                    show_in_commands: None,
                }
            }
        }
        Value::Object(obj) => {
            let ticks = obj["ticks"]
                .as_i64()
                .unwrap_or_else(|| obj["ticks"].as_u64().unwrap() as i64);
            let show = obj["show_in_commands"].as_bool().unwrap();
            let show_ts = if show {
                quote! { Some(true) }
            } else {
                quote! { Some(false) }
            };
            quote! {
                TimeMarker {
                    name: #name,
                    ticks: #ticks,
                    show_in_commands: #show_ts,
                }
            }
        }
        _ => panic!("Unexpected time_marker value: {:?}", v),
    }
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/timeline/");

    let timeline_dir = "build_assets/builtin_datapacks/minecraft/timeline";
    let mut timelines: Vec<(String, TimelineJson)> = Vec::new();

    // Read all timeline JSON files
    for entry in fs::read_dir(timeline_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let timeline_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let timeline_data: TimelineJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", timeline_name, e));

            timelines.push((timeline_name, timeline_data));
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::timeline::{Timeline, TimelineRegistry, Track, Keyframe, KeyframeValue, Ease, TimeMarker};
        use steel_utils::Identifier;
    });

    // Generate static timeline definitions
    let mut register_stream = TokenStream::new();
    for (timeline_name, timeline_data) in &timelines {
        let timeline_ident = Ident::new(&timeline_name.to_shouty_snake_case(), Span::call_site());
        let timeline_name_str = timeline_name.as_str();

        let key = quote! { Identifier::vanilla_static(#timeline_name_str) };

        let clock_ts = match &timeline_data.clock {
            Some(s) => quote_opt_identifier(s),
            None => quote! { None },
        };

        let period_ticks_ts = match timeline_data.period_ticks {
            Some(pt) => quote! { Some(#pt) },
            None => quote! { None },
        };

        // Generate track literals
        let track_tokens: Vec<TokenStream> = timeline_data
            .tracks
            .iter()
            .map(|(track_name, track_val)| {
                let track: TrackJson = serde_json::from_value(track_val.clone())
                    .unwrap_or_else(|e| panic!("Failed to parse track {}: {}", track_name, e));
                let track_name_str = track_name.as_str();

                let ease_ts = match &track.ease {
                    Some(v) => quote_ease(v),
                    None => quote! { None },
                };

                let modifier_ts = match &track.modifier {
                    Some(m) => {
                        let m_str = m.as_str();
                        quote! { Some(#m_str) }
                    }
                    None => quote! { None },
                };

                let keyframe_tokens: Vec<TokenStream> = track
                    .keyframes
                    .iter()
                    .map(|kf| {
                        let ticks = kf.ticks;
                        let value_ts = quote_keyframe_value(&kf.value);
                        quote! {
                            Keyframe {
                                ticks: #ticks,
                                value: #value_ts,
                            }
                        }
                    })
                    .collect();

                quote! {
                    Track {
                        name: #track_name_str,
                        ease: #ease_ts,
                        modifier: #modifier_ts,
                        keyframes: &[
                            #(#keyframe_tokens),*
                        ],
                    }
                }
            })
            .collect();

        // Generate time_marker literals
        let marker_tokens: Vec<TokenStream> = timeline_data
            .time_markers
            .iter()
            .map(|(marker_name, marker_val)| quote_time_marker(marker_name.as_str(), marker_val))
            .collect();

        stream.extend(quote! {
            pub static #timeline_ident: &Timeline = &Timeline {
                key: #key,
                clock: #clock_ts,
                period_ticks: #period_ticks_ts,
                tracks: &[
                    #(#track_tokens),*
                ],
                time_markers: &[
                    #(#marker_tokens),*
                ],
            };
        });

        register_stream.extend(quote! {
            registry.register(#timeline_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_timelines(registry: &mut TimelineRegistry) {
            #register_stream
        }
    });

    stream
}
