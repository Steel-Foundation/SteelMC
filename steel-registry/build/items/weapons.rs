use super::{
    FromStr, Identifier, TokenStream, Value, block_ref_token, generate_sound_event_ref,
    identifier_token, quote,
};

pub(super) fn generate_weapon_component(value: &Value) -> TokenStream {
    let item_damage_per_attack = value
        .get("item_damage_per_attack")
        .and_then(Value::as_i64)
        .unwrap_or(1) as i32;
    let disable_blocking_for_seconds = value
        .get("disable_blocking_for_seconds")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;

    quote! {
        vanilla_components::Weapon {
            item_damage_per_attack: #item_damage_per_attack,
            disable_blocking_for_seconds: #disable_blocking_for_seconds,
        }
    }
}

pub(super) fn generate_attack_range_component(value: &Value) -> TokenStream {
    let min_reach = value
        .get("min_reach")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    let max_reach = value
        .get("max_reach")
        .and_then(Value::as_f64)
        .unwrap_or(3.0) as f32;
    let min_creative_reach = value
        .get("min_creative_reach")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    let max_creative_reach = value
        .get("max_creative_reach")
        .and_then(Value::as_f64)
        .unwrap_or(5.0) as f32;
    let hitbox_margin = value
        .get("hitbox_margin")
        .and_then(Value::as_f64)
        .unwrap_or(0.3) as f32;
    let mob_factor = value
        .get("mob_factor")
        .and_then(Value::as_f64)
        .unwrap_or(1.0) as f32;

    quote! {
        vanilla_components::AttackRange {
            min_reach: #min_reach,
            max_reach: #max_reach,
            min_creative_reach: #min_creative_reach,
            max_creative_reach: #max_creative_reach,
            hitbox_margin: #hitbox_margin,
            mob_factor: #mob_factor,
        }
    }
}

pub(super) fn optional_sound_event_holder_token(value: &Value, field: &str) -> TokenStream {
    let Some(value) = value.get(field) else {
        return quote! { None };
    };

    if let Some(sound) = value.as_str() {
        let id = Identifier::from_str(sound).unwrap_or_else(|error| {
            panic!("invalid sound event id {sound:?} in piercing weapon field {field}: {error}")
        });
        let sound = generate_sound_event_ref(&id);
        return quote! { Some(crate::sound_event::SoundEventHolder::registry(#sound)) };
    }

    let Some(sound) = value.as_object() else {
        panic!("piercing weapon field {field} must be a sound id string or direct sound object");
    };
    let sound_id_value = sound
        .get("sound_id")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("direct piercing weapon sound field {field} missing sound_id"));
    Identifier::from_str(sound_id_value).unwrap_or_else(|error| {
        panic!(
            "invalid direct piercing weapon sound id {sound_id_value:?} in field {field}: {error}"
        )
    });
    let sound_id = identifier_token(sound_id_value);
    let fixed_range = sound.get("range").map_or_else(
        || quote! { None },
        |range| {
            let range = range.as_f64().unwrap_or_else(|| {
                panic!("direct piercing weapon sound field {field} range must be a number")
            }) as f32;
            quote! { Some(#range) }
        },
    );
    quote! {
        Some(crate::sound_event::SoundEventHolder::Direct {
            sound_id: #sound_id,
            fixed_range: #fixed_range,
        })
    }
}

pub(super) fn generate_piercing_weapon_component(value: &Value) -> TokenStream {
    let deals_knockback = value
        .get("deals_knockback")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let dismounts = value
        .get("dismounts")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let sound = optional_sound_event_holder_token(value, "sound");
    let hit_sound = optional_sound_event_holder_token(value, "hit_sound");

    quote! {
        vanilla_components::PiercingWeapon {
            deals_knockback: #deals_knockback,
            dismounts: #dismounts,
            sound: #sound,
            hit_sound: #hit_sound,
        }
    }
}

/// Generates the `TokenStream` for a single `ToolRule` from JSON data.
pub(super) fn generate_tool_rule(rule: &Value) -> TokenStream {
    let blocks_token = match rule.get("blocks") {
        Some(Value::String(value)) if value.starts_with('#') => {
            let tag_value = value.trim_start_matches('#');
            Identifier::from_str(tag_value)
                .unwrap_or_else(|error| panic!("invalid tool block tag {value:?}: {error}"));
            let tag = identifier_token(tag_value);
            quote! { vanilla_components::ToolRuleBlocks::Tag(#tag) }
        }
        Some(Value::String(value)) => {
            let block = block_ref_token(value);
            quote! { vanilla_components::ToolRuleBlocks::Direct(vec![#block]) }
        }
        Some(Value::Array(values)) => {
            let blocks = values
                .iter()
                .map(|value| {
                    let value = value
                        .as_str()
                        .unwrap_or_else(|| panic!("tool rule block list entries must be strings"));
                    assert!(
                        !value.starts_with('#'),
                        "tool rule direct block lists cannot contain tags: {value}"
                    );
                    block_ref_token(value)
                })
                .collect::<Vec<_>>();
            quote! { vanilla_components::ToolRuleBlocks::Direct(vec![#(#blocks),*]) }
        }
        _ => panic!("tool rule must contain blocks as a string or string array"),
    };

    let speed_token = if let Some(value) = rule.get("speed") {
        let speed = value
            .as_f64()
            .unwrap_or_else(|| panic!("tool rule speed must be a number"))
            as f32;
        assert!(
            speed.is_finite() && speed > 0.0,
            "tool rule speed must be a positive finite float"
        );
        quote! { Some(#speed) }
    } else {
        quote! { None }
    };

    let correct_for_drops_token = if let Some(value) = rule.get("correct_for_drops") {
        let correct = value
            .as_bool()
            .unwrap_or_else(|| panic!("tool rule correct_for_drops must be a boolean"));
        quote! { Some(#correct) }
    } else {
        quote! { None }
    };

    quote! {
        vanilla_components::ToolRule {
            blocks: #blocks_token,
            speed: #speed_token,
            correct_for_drops: #correct_for_drops_token,
        }
    }
}
