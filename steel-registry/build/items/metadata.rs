use super::{
    FromStr, Ident, Identifier, Span, ToShoutySnakeCase, TokenStream, Value,
    generate_sound_event_ref, identifier_token, quote, split_identifier,
};

pub(super) fn item_name_component_token(value: &Value) -> TokenStream {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("item_name component must be an object"));
    assert_eq!(
        object.len(),
        1,
        "vanilla item_name component contains unsupported fields: {value}"
    );
    let translation = object
        .get("translate")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("item_name component must contain a translate string"));
    quote! {
        TextComponent::translated(TranslatedMessage::new(#translation, None))
    }
}

pub(super) fn entity_type_ref_token(s: &str) -> Option<TokenStream> {
    let (namespace, path) = split_identifier(s);
    if namespace != "minecraft" {
        return None;
    }

    let ident = Ident::new(&path.to_shouty_snake_case(), Span::call_site());
    Some(quote! { &vanilla_entities::#ident })
}

pub(super) fn registry_sound_event_holder_token(sound: &str, field: &str) -> TokenStream {
    let id = Identifier::from_str(sound).unwrap_or_else(|error| {
        panic!("invalid sound event id {sound:?} in item component field {field}: {error}")
    });
    let sound = generate_sound_event_ref(&id);
    quote! { crate::sound_event::SoundEventHolder::registry(#sound) }
}

pub(super) fn sound_event_value_token(value: &Value, field: &str) -> TokenStream {
    if let Some(sound) = value.as_str() {
        return registry_sound_event_holder_token(sound, field);
    }

    let Some(sound) = value.as_object() else {
        panic!("equippable field {field} must be a sound id string or direct sound object");
    };
    let sound_id_value = sound
        .get("sound_id")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("direct equippable sound field {field} missing sound_id"));
    Identifier::from_str(sound_id_value).unwrap_or_else(|error| {
        panic!("invalid direct equippable sound id {sound_id_value:?} in field {field}: {error}")
    });
    let sound_id = identifier_token(sound_id_value);
    let fixed_range = sound.get("range").map_or_else(
        || quote! { None },
        |range| {
            let range = range.as_f64().unwrap_or_else(|| {
                panic!("direct equippable sound field {field} range must be a number")
            }) as f32;
            quote! { Some(#range) }
        },
    );

    quote! {
        crate::sound_event::SoundEventHolder::Direct {
            sound_id: #sound_id,
            fixed_range: #fixed_range,
        }
    }
}

pub(super) fn sound_event_holder_token(value: &Value, field: &str, default: &str) -> TokenStream {
    value.get(field).map_or_else(
        || registry_sound_event_holder_token(default, field),
        |value| sound_event_value_token(value, field),
    )
}

pub(super) fn rarity_component_token(value: &Value) -> Option<TokenStream> {
    match value
        .as_str()
        .unwrap_or_else(|| panic!("rarity component must be a string"))
    {
        "common" => None,
        "uncommon" => Some(quote! { vanilla_components::Rarity::Uncommon }),
        "rare" => Some(quote! { vanilla_components::Rarity::Rare }),
        "epic" => Some(quote! { vanilla_components::Rarity::Epic }),
        rarity => panic!("unknown rarity component value: {rarity}"),
    }
}

pub(super) fn use_effects_component_token(value: &Value) -> Option<TokenStream> {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("use_effects component must be an object"));
    assert!(
        object.keys().all(|key| matches!(
            key.as_str(),
            "can_sprint" | "interact_vibrations" | "speed_multiplier"
        )),
        "use_effects component contains an unknown field: {value}"
    );
    let can_sprint = object.get("can_sprint").is_some_and(|value| {
        value
            .as_bool()
            .expect("use_effects.can_sprint must be a boolean")
    });
    let interact_vibrations = object.get("interact_vibrations").is_none_or(|value| {
        value
            .as_bool()
            .expect("use_effects.interact_vibrations must be a boolean")
    });
    let speed_multiplier = object.get("speed_multiplier").map_or(0.2_f32, |value| {
        value
            .as_f64()
            .expect("use_effects.speed_multiplier must be a number") as f32
    });
    assert!(
        speed_multiplier.is_finite() && (0.0..=1.0).contains(&speed_multiplier),
        "use_effects.speed_multiplier must be between 0 and 1"
    );
    if !can_sprint && interact_vibrations && speed_multiplier.to_bits() == 0.2_f32.to_bits() {
        return None;
    }
    Some(quote! {
        vanilla_components::UseEffects::new(
            #can_sprint,
            #interact_vibrations,
            #speed_multiplier,
        )
    })
}

pub(super) fn swing_animation_component_token(value: &Value) -> Option<TokenStream> {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("swing_animation component must be an object"));
    assert!(
        object
            .keys()
            .all(|key| matches!(key.as_str(), "type" | "duration")),
        "swing_animation component contains an unknown field: {value}"
    );
    let animation_type = match object.get("type").map_or("whack", |value| {
        value
            .as_str()
            .expect("swing_animation.type must be a string")
    }) {
        "none" => quote! { vanilla_components::SwingAnimationType::None },
        "whack" => quote! { vanilla_components::SwingAnimationType::Whack },
        "stab" => quote! { vanilla_components::SwingAnimationType::Stab },
        animation_type => panic!("unknown swing_animation type: {animation_type}"),
    };
    let duration = object.get("duration").map_or(6_i32, |value| {
        let duration = value
            .as_i64()
            .expect("swing_animation.duration must be an integer");
        i32::try_from(duration).expect("swing_animation.duration is outside the i32 range")
    });
    assert!(duration > 0, "swing_animation.duration must be positive");
    if object.is_empty() {
        return None;
    }
    Some(quote! {
        vanilla_components::SwingAnimation::new(#animation_type, #duration)
    })
}

pub(super) fn damage_type_ref_token(value: &str) -> TokenStream {
    let id = Identifier::from_str(value)
        .unwrap_or_else(|error| panic!("invalid damage_type component id {value:?}: {error}"));
    assert_eq!(
        id.namespace.as_ref(),
        "minecraft",
        "vanilla item damage_type references must use the minecraft namespace: {id}"
    );

    let ident = Ident::new(&id.path.to_shouty_snake_case(), Span::call_site());
    quote! { &crate::vanilla_damage_types::#ident }
}

pub(super) fn banner_pattern_ref_token(value: &str) -> TokenStream {
    let id = Identifier::from_str(value)
        .unwrap_or_else(|error| panic!("invalid banner pattern id {value:?}: {error}"));
    assert_eq!(
        id.namespace.as_ref(),
        "minecraft",
        "vanilla item banner patterns must use the minecraft namespace: {id}"
    );

    let ident = Ident::new(&id.path.to_shouty_snake_case(), Span::call_site());
    quote! { &crate::vanilla_banner_patterns::#ident }
}

pub(super) fn item_ref_token(value: &str, component: &str) -> TokenStream {
    let id = Identifier::from_str(value)
        .unwrap_or_else(|error| panic!("invalid {component} item id {value:?}: {error}"));
    assert_eq!(
        id.namespace.as_ref(),
        "minecraft",
        "vanilla {component} references must use the minecraft namespace: {id}"
    );
    let ident = Ident::new(&id.path.to_shouty_snake_case(), Span::call_site());
    quote! { &*#ident }
}

pub(super) fn holder_set_token(
    value: &Value,
    component: &str,
    direct_ref: impl Fn(&str) -> TokenStream,
) -> TokenStream {
    match value {
        Value::String(value) if value.starts_with('#') => {
            let tag = value.trim_start_matches('#');
            Identifier::from_str(tag)
                .unwrap_or_else(|error| panic!("invalid {component} tag {value:?}: {error}"));
            let tag = identifier_token(tag);
            quote! { crate::RegistryHolderSet::Tag(#tag) }
        }
        Value::String(value) => {
            let entry = direct_ref(value);
            quote! { crate::RegistryHolderSet::Direct(vec![#entry]) }
        }
        Value::Array(values) => {
            let entries = values
                .iter()
                .map(|value| {
                    let value = value.as_str().unwrap_or_else(|| {
                        panic!("{component} direct holder list entries must be strings")
                    });
                    assert!(
                        !value.starts_with('#'),
                        "{component} direct holder lists cannot contain tags: {value}"
                    );
                    direct_ref(value)
                })
                .collect::<Vec<_>>();
            quote! { crate::RegistryHolderSet::Direct(vec![#(#entries),*]) }
        }
        _ => panic!("{component} holder set must be a string or string array"),
    }
}

pub(super) fn holder_set_component_field<'a>(
    value: &'a Value,
    component: &str,
    field: &str,
) -> &'a Value {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("{component} component must be an object"));
    assert_eq!(
        object.len(),
        1,
        "{component} component must contain only {field}"
    );
    object
        .get(field)
        .unwrap_or_else(|| panic!("{component} component must contain {field}"))
}

pub(super) fn optional_identifier_token(value: &Value, field: &str) -> TokenStream {
    value
        .get(field)
        .and_then(|value| value.as_str())
        .map_or_else(
            || quote! { None },
            |id| {
                let id = identifier_token(id);
                quote! { Some(#id) }
            },
        )
}
