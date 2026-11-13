//! Generates typed environment attribute definitions from the extracted
//! built-in registry, and the attribute maps and timeline tracks other build
//! scripts embed.
//!
//! Every vanilla attribute type is generated except `activity`; maps and
//! tracks skip entries for activity attributes.
// TODO: Generate `activity` attributes once Steel has the built-in activity
// registry.

use std::fs;
use std::str::FromStr as _;

use heck::{ToPascalCase, ToShoutySnakeCase};
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use rustc_hash::FxHashMap;
use serde::Deserialize;
use serde_json::Value;
use steel_math::easing::EasingType;
use steel_utils::Identifier;
use steel_utils::value_providers::IntProvider;

use crate::dimension_types::{BedRuleJson, generate_bed_rule};
use crate::features::generate_int_provider;
use crate::generator_functions::generate_sound_event_ref;

const ATTRIBUTES_FILE: &str = "build_assets/environment_attributes.json";

/// Mob categories in vanilla declaration order: serialized name and variant.
const MOB_CATEGORIES: [(&str, &str); 8] = [
    ("monster", "Monster"),
    ("creature", "Creature"),
    ("ambient", "Ambient"),
    ("axolotls", "Axolotls"),
    ("underground_water_creature", "UndergroundWaterCreature"),
    ("water_creature", "WaterCreature"),
    ("water_ambient", "WaterAmbient"),
    ("misc", "Misc"),
];

const MOON_PHASES: [&str; 8] = [
    "full_moon",
    "waning_gibbous",
    "third_quarter",
    "waning_crescent",
    "new_moon",
    "waxing_crescent",
    "first_quarter",
    "waxing_gibbous",
];

#[derive(Deserialize)]
struct EnvironmentAttributesFile {
    environment_attributes: Vec<EnvironmentAttributeJson>,
}

#[derive(Deserialize)]
struct EnvironmentAttributeJson {
    name: String,
    #[serde(rename = "type")]
    attribute_type: String,
    default: Value,
    syncable: bool,
    positional: bool,
    spatially_interpolated: bool,
    full_resolution_biomes: bool,
    min: Option<f32>,
    max: Option<f32>,
}

/// Generated code for a value, and whether it can be built in a `const`
/// context.
struct Quoted {
    tokens: TokenStream,
    is_const: bool,
}

impl Quoted {
    const fn constant(tokens: TokenStream) -> Self {
        Self {
            tokens,
            is_const: true,
        }
    }

    const fn lazy(tokens: TokenStream) -> Self {
        Self {
            tokens,
            is_const: false,
        }
    }

    /// Wraps the value in a `LazyValue`.
    fn into_lazy_value(self) -> TokenStream {
        let tokens = self.tokens;
        if self.is_const {
            quote! { crate::environment_attribute::LazyValue::new(#tokens) }
        } else {
            quote! { crate::environment_attribute::LazyValue::lazy(|| #tokens) }
        }
    }
}

/// Value types Steel implements.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AttributeKind {
    Boolean,
    Float,
    AngleDegrees,
    Integer,
    TriState,
    RgbColor,
    ArgbColor,
    MoonPhase,
    BedRule,
    Particle,
    AmbientParticles,
    BackgroundMusic,
    AmbientSounds,
    MobSpawnSettings,
}

impl AttributeKind {
    /// Returns `None` for `activity`, which Steel does not implement yet.
    fn from_type(attribute_type: &str) -> Option<Self> {
        Some(match attribute_type {
            "minecraft:boolean" => Self::Boolean,
            "minecraft:float" => Self::Float,
            "minecraft:angle_degrees" => Self::AngleDegrees,
            "minecraft:integer" => Self::Integer,
            "minecraft:tri_state" => Self::TriState,
            "minecraft:rgb_color" => Self::RgbColor,
            "minecraft:argb_color" => Self::ArgbColor,
            "minecraft:moon_phase" => Self::MoonPhase,
            "minecraft:bed_rule" => Self::BedRule,
            "minecraft:particle" => Self::Particle,
            "minecraft:ambient_particles" => Self::AmbientParticles,
            "minecraft:background_music" => Self::BackgroundMusic,
            "minecraft:ambient_sounds" => Self::AmbientSounds,
            "minecraft:mob_spawn_settings" => Self::MobSpawnSettings,
            "minecraft:activity" => return None,
            _ => panic!("Unknown environment attribute type {attribute_type}"),
        })
    }

    fn value_type(self) -> TokenStream {
        match self {
            Self::Boolean => quote! { bool },
            Self::Float | Self::AngleDegrees => quote! { f32 },
            Self::Integer => quote! { i32 },
            Self::TriState => quote! { crate::environment_attribute::TriState },
            Self::RgbColor => quote! { glam::Vec3 },
            Self::ArgbColor => quote! { glam::Vec4 },
            Self::MoonPhase => quote! { crate::environment_attribute::MoonPhase },
            Self::BedRule => quote! { crate::environment_attribute::BedRule },
            Self::Particle => quote! { crate::particle_type::ParticleData },
            Self::AmbientParticles => quote! {
                crate::environment_attribute::AttributeList<
                    crate::environment_attribute::AmbientParticle
                >
            },
            Self::BackgroundMusic => quote! { crate::environment_attribute::BackgroundMusic },
            Self::AmbientSounds => quote! { crate::environment_attribute::AmbientSounds },
            Self::MobSpawnSettings => quote! { crate::environment_attribute::MobSpawnSettings },
        }
    }

    fn type_static(self) -> TokenStream {
        let ident = match self {
            Self::Boolean => "BOOLEAN",
            Self::Float => "FLOAT",
            Self::AngleDegrees => "ANGLE_DEGREES",
            Self::Integer => "INTEGER",
            Self::TriState => "TRI_STATE",
            Self::RgbColor => "RGB_COLOR",
            Self::ArgbColor => "ARGB_COLOR",
            Self::MoonPhase => "MOON_PHASE",
            Self::BedRule => "BED_RULE",
            Self::Particle => "PARTICLE",
            Self::AmbientParticles => "AMBIENT_PARTICLES",
            Self::BackgroundMusic => "BACKGROUND_MUSIC",
            Self::AmbientSounds => "AMBIENT_SOUNDS",
            Self::MobSpawnSettings => "MOB_SPAWN_SETTINGS",
        };
        let ident = Ident::new(ident, Span::call_site());
        quote! { crate::environment_attribute::#ident }
    }

    /// Generates a value of this type, as decoded by vanilla's value codec.
    fn quote_value(self, context: &str, value: &Value) -> Quoted {
        match self {
            Self::Boolean => {
                let Some(value) = value.as_bool() else {
                    panic!("{context} expects a boolean, got {value}");
                };
                Quoted::constant(quote! { #value })
            }
            Self::Float | Self::AngleDegrees => Quoted::constant(quote_f32(context, value)),
            Self::Integer => Quoted::constant(quote_i32(context, value)),
            Self::TriState => Quoted::constant(quote_tri_state(context, value)),
            Self::RgbColor => Quoted::constant(quote_rgb(context, value)),
            Self::ArgbColor => Quoted::constant(quote_argb(context, value)),
            Self::MoonPhase => Quoted::constant(quote_moon_phase(context, value)),
            Self::BedRule => {
                let bed_rule: BedRuleJson = serde_json::from_value(value.clone())
                    .unwrap_or_else(|error| panic!("{context} has an invalid bed rule: {error}"));
                Quoted::constant(generate_bed_rule(&bed_rule))
            }
            Self::Particle => Quoted::lazy(quote_particle(context, value)),
            Self::AmbientParticles => quote_ambient_particles(context, value),
            Self::BackgroundMusic => Quoted::constant(quote_background_music(context, value)),
            Self::AmbientSounds => Quoted::constant(quote_ambient_sounds(context, value)),
            Self::MobSpawnSettings => Quoted::constant(quote_mob_spawn_settings(context, value)),
        }
    }

    /// Generates the modifier for `operation` and its decoded argument, as
    /// vanilla's per-type modifier libraries allow them.
    fn quote_modifier(self, context: &str, operation: &str, argument: &Value) -> QuotedModifier {
        let simple = |modifier_type: &str, argument: Quoted| {
            let modifier_type = Ident::new(modifier_type, Span::call_site());
            (
                quote! { #modifier_type },
                quote! { #modifier_type },
                argument,
            )
        };
        let variant = |modifier_type: &str, argument: Quoted| {
            let modifier_type = Ident::new(modifier_type, Span::call_site());
            let variant = pascal_ident(operation);
            (
                quote! { #modifier_type },
                quote! { #modifier_type::#variant },
                argument,
            )
        };
        let (modifier_type, modifier, argument) = match (self, operation) {
            (_, "override") => simple("OverrideModifier", self.quote_value(context, argument)),
            (Self::Boolean, "and" | "nand" | "or" | "nor" | "xor" | "xnor") => {
                variant("BooleanModifier", self.quote_value(context, argument))
            }
            (
                Self::Float | Self::AngleDegrees,
                "add" | "subtract" | "multiply" | "minimum" | "maximum",
            ) => variant(
                "FloatModifier",
                Quoted::constant(quote_f32(context, argument)),
            ),
            (Self::Float | Self::AngleDegrees, "alpha_blend") => simple(
                "FloatAlphaBlendModifier",
                Quoted::constant(quote_float_with_alpha(context, argument)),
            ),
            (Self::Integer, "add" | "subtract" | "multiply" | "minimum" | "maximum") => variant(
                "IntegerModifier",
                Quoted::constant(quote_i32(context, argument)),
            ),
            (Self::RgbColor | Self::ArgbColor, "alpha_blend") => simple(
                "ColorAlphaBlendModifier",
                Quoted::constant(quote_argb(context, argument)),
            ),
            (Self::RgbColor | Self::ArgbColor, "add" | "subtract") => variant(
                "RgbModifier",
                Quoted::constant(quote_rgb(context, argument)),
            ),
            (Self::RgbColor | Self::ArgbColor, "multiply") => {
                simple("ColorMultiplyModifier", self.quote_value(context, argument))
            }
            (Self::RgbColor | Self::ArgbColor, "blend_to_gray") => simple(
                "BlendToGrayModifier",
                Quoted::constant(quote_blend_to_gray(context, argument)),
            ),
            (Self::AmbientParticles, "append") => {
                simple("ListAppendModifier", self.quote_value(context, argument))
            }
            (Self::MobSpawnSettings, "overlay") => simple(
                "MobSpawnSettingsOverlayModifier",
                self.quote_value(context, argument),
            ),
            _ => panic!(
                "{context} uses modifier {operation}, which its attribute type does not allow"
            ),
        };
        QuotedModifier {
            modifier_type: quote! { crate::environment_attribute::#modifier_type },
            modifier: quote! { crate::environment_attribute::#modifier },
            argument,
        }
    }
}

/// Generated modifier type and value, with its argument.
struct QuotedModifier {
    modifier_type: TokenStream,
    modifier: TokenStream,
    argument: Quoted,
}

/// A generated attribute that maps and tracks can reference.
#[derive(Clone)]
pub struct SupportedAttribute {
    kind: AttributeKind,
    const_ident: Ident,
}

impl SupportedAttribute {
    fn path(&self) -> TokenStream {
        let ident = &self.const_ident;
        quote! { crate::vanilla_environment_attributes::#ident }
    }
}

/// Attributes Steel generates, keyed by their full identifier.
pub struct SupportedAttributes {
    by_name: FxHashMap<String, SupportedAttribute>,
}

impl SupportedAttributes {
    /// Loads the extracted registry, keeping attributes of implemented types.
    pub fn load() -> Self {
        let mut by_name = FxHashMap::default();
        let mut const_names = FxHashMap::default();
        for attribute in load_attributes() {
            let Some(kind) = AttributeKind::from_type(&attribute.attribute_type) else {
                continue;
            };
            let const_name = const_name(&attribute.name);
            if let Some(previous) = const_names.insert(const_name.clone(), attribute.name.clone()) {
                panic!(
                    "Environment attributes {previous} and {} share constant {const_name}",
                    attribute.name
                );
            }
            by_name.insert(
                attribute.name,
                SupportedAttribute {
                    kind,
                    const_ident: Ident::new(&const_name, Span::call_site()),
                },
            );
        }
        Self { by_name }
    }

    fn get(&self, name: &str) -> Option<&SupportedAttribute> {
        self.by_name.get(name)
    }

    /// Generates an `EnvironmentAttributeMap` from a data pack `attributes`
    /// object, skipping attributes Steel does not implement.
    pub fn quote_map<'a>(
        &self,
        attributes: impl IntoIterator<Item = (&'a String, &'a Value)>,
    ) -> TokenStream {
        let mut attributes: Vec<_> = attributes.into_iter().collect();
        attributes.sort_by_key(|(name, _)| *name);
        let entries: Vec<(TokenStream, (TokenStream, TokenStream))> = attributes
            .into_iter()
            .filter_map(|(name, value)| {
                let attribute = self.get(name)?;
                Some((
                    attribute.path(),
                    quote_modifier_entry(name, attribute.kind, value),
                ))
            })
            .collect();
        if entries.is_empty() {
            return quote! { crate::environment_attribute::EnvironmentAttributeMap::EMPTY };
        }

        // Entries own lazily built arguments, so they have destructors and
        // cannot be `'static` temporaries; each gets a block-local static.
        let mut statics = TokenStream::new();
        let mut map_entries = Vec::new();
        for (index, (attribute_path, (entry_type, entry))) in entries.into_iter().enumerate() {
            let entry_ident = Ident::new(&format!("ENTRY_{index}"), Span::call_site());
            statics.extend(quote! { static #entry_ident: #entry_type = #entry; });
            map_entries.push(quote! {
                crate::environment_attribute::EnvironmentAttributeMapEntry::new(
                    &#attribute_path,
                    &#entry_ident,
                )
            });
        }
        quote! {
            {
                #statics
                static ENTRIES: &[crate::environment_attribute::EnvironmentAttributeMapEntry] =
                    &[#(#map_entries),*];
                crate::environment_attribute::EnvironmentAttributeMap::new(ENTRIES)
            }
        }
    }

    /// Generates the typed attribute tracks of a timeline's `tracks` object,
    /// skipping attributes Steel does not implement.
    pub fn quote_tracks(&self, tracks: &serde_json::Map<String, Value>) -> TokenStream {
        let tracks: Vec<TokenStream> = tracks
            .iter()
            .filter_map(|(name, track)| {
                let attribute = self.get(name)?;
                Some(quote_track(name, attribute, track))
            })
            .collect();
        quote! {
            {
                static TRACKS: &[&dyn crate::timeline::ErasedAttributeTrack] = &[#(#tracks),*];
                TRACKS
            }
        }
    }
}

fn load_attributes() -> Vec<EnvironmentAttributeJson> {
    println!("cargo:rerun-if-changed={ATTRIBUTES_FILE}");
    let content = fs::read_to_string(ATTRIBUTES_FILE)
        .unwrap_or_else(|error| panic!("Failed to read {ATTRIBUTES_FILE}: {error}"));
    let file: EnvironmentAttributesFile = serde_json::from_str(&content)
        .unwrap_or_else(|error| panic!("Failed to parse {ATTRIBUTES_FILE}: {error}"));
    file.environment_attributes
}

fn const_name(name: &str) -> String {
    let path = name.rsplit('/').next().unwrap_or(name);
    path.to_shouty_snake_case()
}

fn identifier_path(name: &str) -> &str {
    name.strip_prefix("minecraft:")
        .unwrap_or_else(|| panic!("Environment attribute {name} is not in the vanilla namespace"))
}

fn pascal_ident(name: &str) -> Ident {
    Ident::new(&name.to_pascal_case(), Span::call_site())
}

/// Constant name of the generated static for a vanilla registry entry.
fn vanilla_ref_ident(context: &str, id: &str) -> Ident {
    let identifier = Identifier::from_str(id)
        .unwrap_or_else(|_| panic!("{context} has an invalid identifier {id}"));
    assert_eq!(
        identifier.namespace.as_ref(),
        "minecraft",
        "{context} references non-vanilla {id}"
    );
    Ident::new(&identifier.path.to_shouty_snake_case(), Span::call_site())
}

pub fn build() -> TokenStream {
    let mut statics = TokenStream::new();
    let mut registrations = TokenStream::new();

    for attribute in load_attributes() {
        let Some(kind) = AttributeKind::from_type(&attribute.attribute_type) else {
            continue;
        };
        let const_ident = Ident::new(&const_name(&attribute.name), Span::call_site());
        let path = identifier_path(&attribute.name);
        let value_type = kind.value_type();
        let type_static = kind.type_static();
        let default = kind
            .quote_value(&attribute.name, &attribute.default)
            .into_lazy_value();
        let range = quote_range(attribute.min, attribute.max);
        let syncable = attribute.syncable;
        let positional = attribute.positional;
        let spatially_interpolated = attribute.spatially_interpolated;
        let full_resolution_biomes = attribute.full_resolution_biomes;

        statics.extend(quote! {
            pub static #const_ident: EnvironmentAttribute<#value_type> = EnvironmentAttribute::new(
                Identifier::vanilla_static(#path),
                &#type_static,
                #default,
                #range,
                EnvironmentAttributeFlags {
                    syncable: #syncable,
                    positional: #positional,
                    spatially_interpolated: #spatially_interpolated,
                    full_resolution_biomes: #full_resolution_biomes,
                },
            );
        });
        registrations.extend(quote! {
            registry.register(&#const_ident);
        });
    }

    quote! {
        use crate::environment_attribute::{
            AttributeRange, EnvironmentAttribute, EnvironmentAttributeFlags,
            EnvironmentAttributeRegistry,
        };
        use steel_utils::Identifier;

        #statics

        pub fn register_environment_attributes(registry: &mut EnvironmentAttributeRegistry) {
            #registrations
        }
    }
}

fn quote_range(min: Option<f32>, max: Option<f32>) -> TokenStream {
    if min.is_none() && max.is_none() {
        return quote! { AttributeRange::Any };
    }
    let min = min.map_or_else(|| quote! { f32::NEG_INFINITY }, |min| quote! { #min });
    let max = max.map_or_else(|| quote! { f32::INFINITY }, |max| quote! { #max });
    quote! { AttributeRange::of_float(#min, #max) }
}

/// Generates the type and constructor of an `AttributeModifierEntry` for a
/// map value: either a plain value (override) or
/// `{"modifier": .., "argument": ..}`.
fn quote_modifier_entry(
    attribute: &str,
    kind: AttributeKind,
    value: &Value,
) -> (TokenStream, TokenStream) {
    let (operation, argument) = match value {
        Value::Object(object) if object.contains_key("modifier") => {
            let Some(operation) = object["modifier"].as_str() else {
                panic!("{attribute} has a non-string modifier");
            };
            let Some(argument) = object.get("argument") else {
                panic!("{attribute} modifier {operation} has no argument");
            };
            (operation, argument)
        }
        _ => ("override", value),
    };
    let value_type = kind.value_type();
    let QuotedModifier {
        modifier_type,
        modifier,
        argument,
    } = kind.quote_modifier(attribute, operation, argument);
    let argument_tokens = argument.tokens;
    let constructor = if argument.is_const {
        quote! { new(#modifier, #argument_tokens) }
    } else {
        quote! { new_lazy(#modifier, || #argument_tokens) }
    };
    let entry_type = quote! {
        crate::environment_attribute::AttributeModifierEntry<#value_type, #modifier_type>
    };
    (entry_type.clone(), quote! { <#entry_type>::#constructor })
}

fn quote_track(name: &str, attribute: &SupportedAttribute, track: &Value) -> TokenStream {
    let Value::Object(track) = track else {
        panic!("Timeline track {name} is not an object");
    };
    let operation = track.get("modifier").map_or("override", |modifier| {
        modifier
            .as_str()
            .unwrap_or_else(|| panic!("Timeline track {name} has a non-string modifier"))
    });
    let Some(Value::Array(keyframes)) = track.get("keyframes") else {
        panic!("Timeline track {name} has no keyframes");
    };
    let Some(first_keyframe) = keyframes.first() else {
        panic!("Timeline track {name} has no keyframes");
    };

    let modifier = attribute
        .kind
        .quote_modifier(name, operation, field(name, first_keyframe, "value"))
        .modifier;
    let keyframes: Vec<TokenStream> = keyframes
        .iter()
        .map(|keyframe| {
            let Some(ticks) = keyframe.get("ticks").and_then(Value::as_i64) else {
                panic!("Timeline track {name} has a keyframe without integer ticks");
            };
            let Ok(ticks) = i32::try_from(ticks) else {
                panic!("Timeline track {name} keyframe ticks {ticks} exceed i32");
            };
            let value = attribute
                .kind
                .quote_modifier(name, operation, field(name, keyframe, "value"))
                .argument;
            assert!(
                value.is_const,
                "Timeline track {name} needs keyframe values buildable in a const context"
            );
            let value = value.tokens;
            quote! { steel_math::keyframe_track::Keyframe { ticks: #ticks, value: #value } }
        })
        .collect();

    let attribute_path = attribute.path();
    let value_type = attribute.kind.value_type();
    let easing = quote_easing(name, track.get("ease"));
    quote! {
        &crate::timeline::AttributeTrack::<#value_type, _>::new(
            &#attribute_path,
            #modifier,
            steel_math::keyframe_track::KeyframeTrack::new(&[#(#keyframes),*], #easing),
        )
    }
}

fn quote_easing(track: &str, ease: Option<&Value>) -> TokenStream {
    match ease {
        None => quote! { steel_math::easing::EasingType::Linear },
        Some(Value::String(name)) => {
            assert!(
                EasingType::from_name(name).is_some(),
                "Timeline track {track} uses unknown easing {name}"
            );
            let variant = pascal_ident(name);
            quote! { steel_math::easing::EasingType::#variant }
        }
        Some(Value::Object(object)) => {
            let Some(Value::Array(controls)) = object.get("cubic_bezier") else {
                panic!("Timeline track {track} has an unknown easing object");
            };
            let [x1, y1, x2, y2] = controls.as_slice() else {
                panic!("Timeline track {track} cubic_bezier needs 4 control values");
            };
            let [x1, y1, x2, y2] = [x1, y1, x2, y2].map(|value| quote_f32(track, value));
            quote! {
                steel_math::easing::EasingType::CubicBezier(
                    steel_math::easing::CubicBezierControls { x1: #x1, y1: #y1, x2: #x2, y2: #y2 }
                )
            }
        }
        Some(other) => panic!("Timeline track {track} has an invalid easing {other}"),
    }
}

fn field<'a>(context: &str, object: &'a Value, name: &str) -> &'a Value {
    object
        .get(name)
        .unwrap_or_else(|| panic!("{context} is missing field {name}"))
}

fn quote_f32(context: &str, value: &Value) -> TokenStream {
    let Some(value) = value.as_f64() else {
        panic!("{context} expects a float, got {value}");
    };
    let literal = Literal::f32_suffixed(value as f32);
    quote! { #literal }
}

fn quote_f64(context: &str, value: &Value) -> TokenStream {
    let Some(value) = value.as_f64() else {
        panic!("{context} expects a double, got {value}");
    };
    let literal = Literal::f64_suffixed(value);
    quote! { #literal }
}

fn parse_i32(context: &str, value: &Value) -> i32 {
    value
        .as_i64()
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or_else(|| panic!("{context} expects an int, got {value}"))
}

fn quote_i32(context: &str, value: &Value) -> TokenStream {
    let value = parse_i32(context, value);
    quote! { #value }
}

/// Vanilla `TriState.CODEC`: a boolean or `"true"`/`"false"`/`"default"`.
fn quote_tri_state(context: &str, value: &Value) -> TokenStream {
    let variant = match value {
        Value::Bool(true) => "True",
        Value::Bool(false) => "False",
        Value::String(name) => match name.as_str() {
            "true" => "True",
            "false" => "False",
            "default" => "Default",
            _ => panic!("{context} has an invalid tri-state {name}"),
        },
        _ => panic!("{context} has an invalid tri-state {value}"),
    };
    let variant = Ident::new(variant, Span::call_site());
    quote! { crate::environment_attribute::TriState::#variant }
}

/// Parses a packed color: vanilla's `hexColor(digits)` string or an int.
fn parse_packed_color(context: &str, value: &Value, digits: usize) -> Option<u32> {
    match value {
        Value::String(text) => {
            let Some(hex) = text.strip_prefix('#').filter(|hex| hex.len() == digits) else {
                panic!("{context} color {text} needs '#' and {digits} hex digits");
            };
            Some(
                u32::from_str_radix(hex, 16)
                    .unwrap_or_else(|_| panic!("{context} has an invalid hex color {text}")),
            )
        }
        Value::Number(_) => Some(parse_i32(context, value).cast_unsigned()),
        _ => None,
    }
}

fn float_components<const N: usize>(context: &str, value: &Value) -> [TokenStream; N] {
    let components = value
        .as_array()
        .filter(|components| components.len() == N)
        .unwrap_or_else(|| panic!("{context} has an invalid color {value}"));
    std::array::from_fn(|index| quote_f32(context, &components[index]))
}

/// Vanilla `ExtraCodecs.STRING_RGB_VEC3_COLOR`: `"#rrggbb"`, a packed int, or
/// `[r, g, b]` floats.
fn quote_rgb(context: &str, value: &Value) -> TokenStream {
    if let Some(packed) = parse_packed_color(context, value, 6) {
        let packed = Literal::u32_suffixed(packed);
        return quote! { crate::environment_attribute::argb::vector3f_from_rgb24(#packed) };
    }
    let [red, green, blue] = float_components(context, value);
    quote! { glam::Vec3::new(#red, #green, #blue) }
}

/// Vanilla `ExtraCodecs.STRING_ARGB_VEC4_COLOR`: `"#aarrggbb"`, a packed int,
/// or `[r, g, b, a]` floats.
fn quote_argb(context: &str, value: &Value) -> TokenStream {
    if let Some(packed) = parse_packed_color(context, value, 8) {
        let packed = Literal::u32_suffixed(packed);
        return quote! { crate::environment_attribute::argb::vector4f_from_argb32(#packed) };
    }
    let [red, green, blue, alpha] = float_components(context, value);
    quote! { glam::Vec4::new(#red, #green, #blue, #alpha) }
}

/// Vanilla `FloatWithAlpha.CODEC`: a plain float (alpha 1) or `{value, alpha}`.
fn quote_float_with_alpha(context: &str, argument: &Value) -> TokenStream {
    let (value, alpha) = match argument {
        Value::Object(object) => {
            let value = quote_f32(context, field(context, argument, "value"));
            let alpha = object
                .get("alpha")
                .map_or_else(|| quote! { 1.0f32 }, |alpha| quote_f32(context, alpha));
            (value, alpha)
        }
        _ => (quote_f32(context, argument), quote! { 1.0f32 }),
    };
    quote! {
        crate::environment_attribute::FloatWithAlpha { value: #value, alpha: #alpha }
    }
}

fn quote_blend_to_gray(context: &str, argument: &Value) -> TokenStream {
    let brightness = quote_f32(context, field(context, argument, "brightness"));
    let factor = quote_f32(context, field(context, argument, "factor"));
    quote! {
        crate::environment_attribute::BlendToGray { brightness: #brightness, factor: #factor }
    }
}

fn quote_moon_phase(context: &str, value: &Value) -> TokenStream {
    let Some(name) = value.as_str().filter(|name| MOON_PHASES.contains(name)) else {
        panic!("{context} has an invalid moon phase {value}");
    };
    let variant = pascal_ident(name);
    quote! { crate::environment_attribute::MoonPhase::#variant }
}

/// Vanilla `ParticleTypes.CODEC` for particles without options.
///
/// Vanilla data only uses option-less particles in attributes; option-bearing
/// particles fail the build so they get implemented instead of dropped.
fn quote_particle(context: &str, value: &Value) -> TokenStream {
    let Some(object) = value.as_object() else {
        panic!("{context} has an invalid particle {value}");
    };
    let Some(particle_type) = object.get("type").and_then(Value::as_str) else {
        panic!("{context} particle has no type");
    };
    assert!(
        object.len() == 1,
        "{context} particle {particle_type} has options, which attribute generation does not support"
    );
    let ident = vanilla_ref_ident(context, particle_type);
    quote! {
        crate::particle_type::ParticleData::simple(&crate::vanilla_particle_types::#ident)
    }
}

fn quote_ambient_particles(context: &str, value: &Value) -> Quoted {
    let Some(particles) = value.as_array() else {
        panic!("{context} expects a list of ambient particles, got {value}");
    };
    if particles.is_empty() {
        return Quoted::constant(quote! { crate::environment_attribute::AttributeList::EMPTY });
    }
    let particles = particles.iter().map(|particle| {
        let particle_data = quote_particle(context, field(context, particle, "particle"));
        let probability = quote_f32(context, field(context, particle, "probability"));
        quote! {
            crate::environment_attribute::AmbientParticle {
                particle: #particle_data,
                probability: #probability,
            }
        }
    });
    Quoted::lazy(quote! {
        crate::environment_attribute::AttributeList::owned(vec![#(#particles),*])
    })
}

/// Vanilla `SoundEvent.CODEC` for registry references.
fn quote_sound_event(context: &str, value: &Value) -> TokenStream {
    let Some(id) = value.as_str() else {
        panic!(
            "{context} uses an inline sound event {value}, which attribute generation does not support"
        );
    };
    let identifier = Identifier::from_str(id)
        .unwrap_or_else(|_| panic!("{context} has an invalid sound event {id}"));
    generate_sound_event_ref(&identifier)
}

fn quote_option(
    value: Option<&Value>,
    quote_some: impl FnOnce(&Value) -> TokenStream,
) -> TokenStream {
    value.map_or_else(
        || quote! { None },
        |value| {
            let value = quote_some(value);
            quote! { Some(#value) }
        },
    )
}

/// Vanilla `sounds.Music.CODEC`.
fn quote_music(context: &str, music: &Value) -> TokenStream {
    let sound = quote_sound_event(context, field(context, music, "sound"));
    let min_delay = quote_i32(context, field(context, music, "min_delay"));
    let max_delay = quote_i32(context, field(context, music, "max_delay"));
    let replace_current_music = music.get("replace_current_music").is_some_and(|replace| {
        replace
            .as_bool()
            .unwrap_or_else(|| panic!("{context} has a non-boolean replace_current_music"))
    });
    quote! {
        crate::environment_attribute::Music {
            sound: #sound,
            min_delay: #min_delay,
            max_delay: #max_delay,
            replace_current_music: #replace_current_music,
        }
    }
}

fn quote_background_music(context: &str, value: &Value) -> TokenStream {
    let default_music = quote_option(value.get("default"), |music| quote_music(context, music));
    let creative_music = quote_option(value.get("creative"), |music| quote_music(context, music));
    let underwater_music =
        quote_option(value.get("underwater"), |music| quote_music(context, music));
    quote! {
        crate::environment_attribute::BackgroundMusic {
            default_music: #default_music,
            creative_music: #creative_music,
            underwater_music: #underwater_music,
        }
    }
}

fn quote_ambient_sounds(context: &str, value: &Value) -> TokenStream {
    let loop_sound = quote_option(value.get("loop"), |sound| quote_sound_event(context, sound));
    let mood = quote_option(value.get("mood"), |mood| {
        let sound = quote_sound_event(context, field(context, mood, "sound"));
        let tick_delay = quote_i32(context, field(context, mood, "tick_delay"));
        let block_search_extent = quote_i32(context, field(context, mood, "block_search_extent"));
        let offset = quote_f64(context, field(context, mood, "offset"));
        quote! {
            crate::environment_attribute::AmbientMoodSettings {
                sound: #sound,
                tick_delay: #tick_delay,
                block_search_extent: #block_search_extent,
                sound_position_offset: #offset,
            }
        }
    });
    // Vanilla `ExtraCodecs.compactListCodec`: one element or a list.
    let additions: Vec<&Value> = match value.get("additions") {
        None => Vec::new(),
        Some(Value::Array(additions)) => additions.iter().collect(),
        Some(addition) => vec![addition],
    };
    let additions = additions.into_iter().map(|addition| {
        let sound = quote_sound_event(context, field(context, addition, "sound"));
        let tick_chance = quote_f64(context, field(context, addition, "tick_chance"));
        quote! {
            crate::environment_attribute::AmbientAdditionsSettings {
                sound: #sound,
                tick_chance: #tick_chance,
            }
        }
    });
    quote! {
        crate::environment_attribute::AmbientSounds {
            loop_sound: #loop_sound,
            mood: #mood,
            additions: std::borrow::Cow::Borrowed(&[#(#additions),*]),
        }
    }
}

/// Vanilla `MobSpawnSettings.CODEC`, with categories in declaration order.
fn quote_mob_spawn_settings(context: &str, value: &Value) -> TokenStream {
    let Some(spawns_by_category) = field(context, value, "spawns_by_category").as_object() else {
        panic!("{context} spawns_by_category is not an object");
    };
    for category in spawns_by_category.keys() {
        assert!(
            MOB_CATEGORIES.iter().any(|(name, _)| name == category),
            "{context} has an unknown mob category {category}"
        );
    }
    // Spawn lists hold `Cow`s and int providers, which have destructors, so
    // each array is a block-local static rather than a `'static` temporary.
    let mut statics = TokenStream::new();
    let mut categories = Vec::new();
    for (name, variant) in MOB_CATEGORIES {
        let Some(spawns) = spawns_by_category.get(name) else {
            continue;
        };
        let Some(spawns) = spawns.as_array() else {
            panic!("{context} {name} spawns are not a list");
        };
        let spawns_ident = Ident::new(
            &format!("{}_SPAWNS", name.to_shouty_snake_case()),
            Span::call_site(),
        );
        let spawn_count = spawns.len();
        let spawns = spawns
            .iter()
            .map(|spawn| quote_weighted_spawner(context, spawn));
        statics.extend(quote! {
            static #spawns_ident: [
                crate::environment_attribute::Weighted<crate::environment_attribute::SpawnerData>;
                #spawn_count
            ] = [#(#spawns),*];
        });
        let variant = Ident::new(variant, Span::call_site());
        categories.push(quote! {
            (
                crate::entity_type::MobCategory::#variant,
                std::borrow::Cow::Borrowed(&#spawns_ident),
            )
        });
    }
    let category_count = categories.len();

    let Some(spawn_costs) = field(context, value, "spawn_costs").as_object() else {
        panic!("{context} spawn_costs is not an object");
    };
    let cost_count = spawn_costs.len();
    let costs = spawn_costs.iter().map(|(entity_type, cost)| {
        let entity_type = vanilla_ref_ident(context, entity_type);
        let energy_budget = quote_f64(context, field(context, cost, "energy_budget"));
        let charge = quote_f64(context, field(context, cost, "charge"));
        quote! {
            (
                &crate::vanilla_entities::#entity_type,
                crate::environment_attribute::MobSpawnCost {
                    energy_budget: #energy_budget,
                    charge: #charge,
                },
            )
        }
    });
    quote! {
        {
            #statics
            static CATEGORIES: [crate::environment_attribute::CategorySpawns; #category_count] =
                [#(#categories),*];
            static COSTS: [
                (crate::entity_type::EntityTypeRef, crate::environment_attribute::MobSpawnCost);
                #cost_count
            ] = [#(#costs),*];
            crate::environment_attribute::MobSpawnSettings::new(&CATEGORIES, &COSTS)
        }
    }
}

/// A `Weighted<SpawnerData>` entry: the spawner fields plus `weight`.
fn quote_weighted_spawner(context: &str, spawn: &Value) -> TokenStream {
    let Some(entity_type) = field(context, spawn, "type").as_str() else {
        panic!("{context} spawner type is not a string");
    };
    let entity_type = vanilla_ref_ident(context, entity_type);
    let count: IntProvider = serde_json::from_value(field(context, spawn, "count").clone())
        .unwrap_or_else(|error| panic!("{context} has an invalid spawn count: {error}"));
    let count = generate_int_provider(&count);
    let weight = quote_i32(context, field(context, spawn, "weight"));
    quote! {
        crate::environment_attribute::Weighted {
            value: crate::environment_attribute::SpawnerData::new(
                &crate::vanilla_entities::#entity_type,
                {
                    use steel_utils::value_providers::IntProvider;
                    #count
                },
            ),
            weight: #weight,
        }
    }
}
