use super::{Literal, TokenStream, quote};

#[derive(Clone, Copy)]
pub(super) enum NbtNumberHint {
    Infer,
    Float,
    Double,
}

#[derive(Clone, Copy)]
pub(super) enum NbtValueHint {
    Infer,
    Float,
    Double,
    LevelBasedValue,
    FloatProvider,
    DoubleBounds,
    MovementPredicate,
}

impl NbtValueHint {
    const fn number_hint(self) -> NbtNumberHint {
        match self {
            Self::Float | Self::LevelBasedValue | Self::FloatProvider => NbtNumberHint::Float,
            Self::Double | Self::DoubleBounds => NbtNumberHint::Double,
            Self::Infer | Self::MovementPredicate => NbtNumberHint::Infer,
        }
    }
}

pub(super) fn generate_nbt_number(number: &serde_json::Number, hint: NbtNumberHint) -> TokenStream {
    match hint {
        NbtNumberHint::Float => {
            let Some(value) = number.as_f64() else {
                panic!("unsupported enchantment effect NBT float: {number}");
            };
            let value = Literal::f32_unsuffixed(value as f32);
            return quote! { NbtTag::Float(#value) };
        }
        NbtNumberHint::Double => {
            let Some(value) = number.as_f64() else {
                panic!("unsupported enchantment effect NBT double: {number}");
            };
            let value = Literal::f64_unsuffixed(value);
            return quote! { NbtTag::Double(#value) };
        }
        NbtNumberHint::Infer => {}
    }

    if let Some(value) = number.as_i64() {
        if let Ok(value) = i32::try_from(value) {
            let value = Literal::i32_unsuffixed(value);
            return quote! { NbtTag::Int(#value) };
        }

        let value = Literal::i64_unsuffixed(value);
        return quote! { NbtTag::Long(#value) };
    }

    if let Some(value) = number.as_u64() {
        if let Ok(value) = i32::try_from(value) {
            let value = Literal::i32_unsuffixed(value);
            return quote! { NbtTag::Int(#value) };
        }
        if let Ok(value) = i64::try_from(value) {
            let value = Literal::i64_unsuffixed(value);
            return quote! { NbtTag::Long(#value) };
        }

        panic!("enchantment effect NBT integer out of i64 range: {value}");
    }

    let Some(value) = number.as_f64() else {
        panic!("unsupported enchantment effect NBT number: {number}");
    };
    let value = Literal::f32_unsuffixed(value as f32);
    quote! { NbtTag::Float(#value) }
}

pub(super) fn generate_nbt_compound(
    value: &serde_json::Value,
    context: &str,
    hint: NbtValueHint,
) -> TokenStream {
    let Some(object) = value.as_object() else {
        panic!("enchantment effect NBT {context} must be an object");
    };
    let object_type = object.get("type").and_then(serde_json::Value::as_str);
    let entries = object.iter().map(|(key, value)| {
        let value_hint = nbt_child_value_hint(hint, object_type, key);
        let value = generate_nbt_tag(value, value_hint);
        quote! {
            compound.insert(#key, #value);
        }
    });

    quote! {{
        let mut compound = NbtCompound::new();
        #(#entries)*
        compound
    }}
}

pub(super) fn nbt_child_value_hint(
    parent: NbtValueHint,
    object_type: Option<&str>,
    key: &str,
) -> NbtValueHint {
    match parent {
        NbtValueHint::LevelBasedValue => match key {
            "value" | "base" | "power" | "numerator" | "denominator" | "fallback" => {
                NbtValueHint::LevelBasedValue
            }
            "min" | "max" | "added" | "per_level_above_first" | "values" => NbtValueHint::Float,
            _ => NbtValueHint::Infer,
        },
        NbtValueHint::FloatProvider => match key {
            "value" | "min" | "max" | "min_inclusive" | "max_exclusive" | "mean" | "deviation"
            | "plateau" | "constant" | "scale" => NbtValueHint::Float,
            _ => NbtValueHint::Infer,
        },
        NbtValueHint::DoubleBounds => match key {
            "min" | "max" => NbtValueHint::Double,
            _ => NbtValueHint::Infer,
        },
        NbtValueHint::MovementPredicate => match key {
            "x" | "y" | "z" | "speed" | "horizontal_speed" | "vertical_speed" | "fall_distance" => {
                NbtValueHint::DoubleBounds
            }
            _ => NbtValueHint::Infer,
        },
        NbtValueHint::Infer | NbtValueHint::Float | NbtValueHint::Double => {
            nbt_object_child_hint(object_type, key)
        }
    }
}

pub(super) fn nbt_object_child_hint(object_type: Option<&str>, key: &str) -> NbtValueHint {
    match object_type {
        Some("minecraft:apply_impulse") => match key {
            "direction" | "coordinate_scale" => NbtValueHint::Double,
            "magnitude" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:explode") => match key {
            "offset" => NbtValueHint::Double,
            "radius" | "knockback_multiplier" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:change_item_damage" | "minecraft:apply_exhaustion") => match key {
            "amount" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:damage_entity") => match key {
            "min_damage" | "max_damage" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:ignite") => match key {
            "duration" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:apply_mob_effect") => match key {
            "min_duration" | "max_duration" | "min_amplifier" | "max_amplifier" => {
                NbtValueHint::LevelBasedValue
            }
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:add" | "minecraft:set") => match key {
            "value" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:multiply") => match key {
            "factor" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:remove_binomial") => match key {
            "chance" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:play_sound") => match key {
            "volume" | "pitch" => NbtValueHint::FloatProvider,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:spawn_particles") => match key {
            "speed" => NbtValueHint::FloatProvider,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:replace_disk") => match key {
            "radius" | "height" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:clamped") => match key {
            "value" => NbtValueHint::LevelBasedValue,
            "min" | "max" => NbtValueHint::Float,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:exponent") => match key {
            "base" | "power" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:fraction") => match key {
            "numerator" | "denominator" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:levels_squared") => match key {
            "added" => NbtValueHint::Float,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:linear") => match key {
            "base" | "per_level_above_first" => NbtValueHint::Float,
            _ => NbtValueHint::Infer,
        },
        Some("minecraft:lookup") => match key {
            "values" => NbtValueHint::Float,
            "fallback" => NbtValueHint::LevelBasedValue,
            _ => NbtValueHint::Infer,
        },
        _ => match key {
            "minecraft:movement" | "movement" => NbtValueHint::MovementPredicate,
            "offset" | "scale" | "movement_scale" if is_float_provider_object(object_type) => {
                NbtValueHint::FloatProvider
            }
            _ => NbtValueHint::Infer,
        },
    }
}

pub(super) fn is_float_provider_object(object_type: Option<&str>) -> bool {
    matches!(
        object_type,
        Some(
            "minecraft:constant"
                | "minecraft:uniform"
                | "minecraft:clamped_normal"
                | "minecraft:trapezoid"
                | "minecraft:in_bounding_box"
                | "minecraft:entity_position"
        )
    )
}

pub(super) fn generate_nbt_tag(value: &serde_json::Value, hint: NbtValueHint) -> TokenStream {
    match value {
        serde_json::Value::Null => {
            panic!("enchantment effect NBT cannot contain null values");
        }
        serde_json::Value::Bool(value) => {
            let value = i8::from(*value);
            quote! { NbtTag::Byte(#value) }
        }
        serde_json::Value::Number(number) => generate_nbt_number(number, hint.number_hint()),
        serde_json::Value::String(value) => quote! { NbtTag::String(#value.into()) },
        serde_json::Value::Array(values) => {
            if values.is_empty() {
                return quote! { NbtTag::List(NbtList::Empty) };
            }

            let values = values.iter().map(|value| generate_nbt_tag(value, hint));
            quote! { NbtTag::List(NbtList::from(vec![#(#values),*])) }
        }
        serde_json::Value::Object(_) => {
            let value = generate_nbt_compound(value, "compound", hint);
            quote! { NbtTag::Compound(#value) }
        }
    }
}
