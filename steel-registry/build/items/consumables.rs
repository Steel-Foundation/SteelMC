use super::{
    FromStr, Ident, Identifier, Span, ToShoutySnakeCase, TokenStream, Value, holder_set_token,
    quote, registry_sound_event_holder_token, sound_event_value_token,
};

pub(super) fn mob_effect_ref_token(value: &str) -> TokenStream {
    let id = Identifier::from_str(value)
        .unwrap_or_else(|error| panic!("invalid mob effect id {value:?}: {error}"));
    assert_eq!(
        id.namespace.as_ref(),
        "minecraft",
        "vanilla item prototypes must reference vanilla mob effects: {id}"
    );
    let ident = Ident::new(&id.path.to_shouty_snake_case(), Span::call_site());
    quote! { &crate::vanilla_mob_effects::#ident }
}

pub(super) fn mob_effect_instance_token(value: &Value) -> TokenStream {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("mob effect instance must be an object"));
    assert!(
        object.keys().all(|key| matches!(
            key.as_str(),
            "id" | "duration" | "amplifier" | "ambient" | "show_particles" | "show_icon"
        )),
        "extracted mob effect instance contains unsupported fields: {value}"
    );
    let effect = object.get("id").and_then(Value::as_str).map_or_else(
        || panic!("mob effect instance must define id"),
        mob_effect_ref_token,
    );
    let integer = |field: &str, default: i32| {
        object.get(field).map_or(default, |value| {
            let value = value
                .as_i64()
                .unwrap_or_else(|| panic!("mob effect {field} must be an integer"));
            i32::try_from(value).unwrap_or_else(|_| panic!("mob effect {field} must fit an i32"))
        })
    };
    let boolean = |field: &str, default: bool| {
        object.get(field).map_or(default, |value| {
            value
                .as_bool()
                .unwrap_or_else(|| panic!("mob effect {field} must be a boolean"))
        })
    };
    let duration = integer("duration", 0);
    let amplifier = integer("amplifier", 0);
    let ambient = boolean("ambient", false);
    let show_particles = boolean("show_particles", true);
    let show_icon = boolean("show_icon", show_particles);
    quote! {
        crate::MobEffectInstance::new(
            #effect,
            #duration,
            #amplifier,
            #ambient,
            #show_particles,
            #show_icon,
            None,
        )
    }
}

pub(super) fn consume_effect_token(value: &Value) -> TokenStream {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("consume effect must be an object"));
    let effect_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("consume effect must define type"));
    match effect_type {
        "minecraft:apply_effects" => {
            let effects = object
                .get("effects")
                .and_then(Value::as_array)
                .unwrap_or_else(|| panic!("apply_effects must define an effects array"))
                .iter()
                .map(mob_effect_instance_token);
            let probability = object.get("probability").map_or(1.0, |value| {
                value
                    .as_f64()
                    .unwrap_or_else(|| panic!("apply_effects probability must be a number"))
                    as f32
            });
            assert!(
                !probability.is_nan() && (0.0..=1.0).contains(&probability),
                "apply_effects probability must be in 0..=1"
            );
            quote! {
                crate::ConsumeEffectData::new(
                    &crate::consume_effect::vanilla_consume_effect_types::APPLY_EFFECTS,
                    crate::consume_effect::ApplyStatusEffectsConsumeEffect::from_extracted(
                        vec![#(#effects),*],
                        #probability,
                    ),
                )
            }
        }
        "minecraft:remove_effects" => {
            let effects = object
                .get("effects")
                .unwrap_or_else(|| panic!("remove_effects must define effects"));
            let effects = holder_set_token(effects, "remove_effects", mob_effect_ref_token);
            quote! {
                crate::ConsumeEffectData::new(
                    &crate::consume_effect::vanilla_consume_effect_types::REMOVE_EFFECTS,
                    crate::consume_effect::RemoveStatusEffectsConsumeEffect::new(#effects),
                )
            }
        }
        "minecraft:clear_all_effects" => quote! {
            crate::ConsumeEffectData::new(
                &crate::consume_effect::vanilla_consume_effect_types::CLEAR_ALL_EFFECTS,
                crate::consume_effect::ClearAllStatusEffectsConsumeEffect,
            )
        },
        "minecraft:teleport_randomly" => {
            let diameter = object.get("diameter").map_or(16.0, |value| {
                value
                    .as_f64()
                    .unwrap_or_else(|| panic!("teleport_randomly diameter must be a number"))
                    as f32
            });
            assert!(
                diameter > 0.0,
                "teleport_randomly diameter must be positive"
            );
            quote! {
                crate::ConsumeEffectData::new(
                    &crate::consume_effect::vanilla_consume_effect_types::TELEPORT_RANDOMLY,
                    crate::consume_effect::TeleportRandomlyConsumeEffect::from_extracted(#diameter),
                )
            }
        }
        "minecraft:play_sound" => {
            let sound = object
                .get("sound")
                .unwrap_or_else(|| panic!("play_sound must define sound"));
            let sound = sound_event_value_token(sound, "consume_effect.play_sound");
            quote! {
                crate::ConsumeEffectData::new(
                    &crate::consume_effect::vanilla_consume_effect_types::PLAY_SOUND,
                    crate::consume_effect::PlaySoundConsumeEffect::new(#sound),
                )
            }
        }
        _ => panic!("unknown extracted consume effect type {effect_type:?}"),
    }
}

pub(super) fn consume_effects_token(value: Option<&Value>, field: &str) -> Vec<TokenStream> {
    value.map_or_else(Vec::new, |value| {
        value
            .as_array()
            .unwrap_or_else(|| panic!("{field} must be an array"))
            .iter()
            .map(consume_effect_token)
            .collect()
    })
}

pub(super) fn item_use_animation_token(value: Option<&Value>) -> TokenStream {
    let name = value.map_or("eat", |value| {
        value
            .as_str()
            .unwrap_or_else(|| panic!("consumable animation must be a string"))
    });
    let variant = match name {
        "none" => quote! { None },
        "eat" => quote! { Eat },
        "drink" => quote! { Drink },
        "block" => quote! { Block },
        "bow" => quote! { Bow },
        "trident" => quote! { Trident },
        "crossbow" => quote! { Crossbow },
        "spyglass" => quote! { Spyglass },
        "toot_horn" => quote! { TootHorn },
        "brush" => quote! { Brush },
        "bundle" => quote! { Bundle },
        "spear" => quote! { Spear },
        _ => panic!("unknown consumable animation {name:?}"),
    };
    quote! { vanilla_components::ItemUseAnimation::#variant }
}

pub(super) fn consumable_component_token(value: &Value) -> TokenStream {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("consumable component must be an object"));
    assert!(
        object.keys().all(|key| matches!(
            key.as_str(),
            "consume_seconds"
                | "animation"
                | "sound"
                | "has_consume_particles"
                | "on_consume_effects"
        )),
        "consumable component contains unsupported fields: {value}"
    );
    let consume_seconds = object.get("consume_seconds").map_or(1.6, |value| {
        value
            .as_f64()
            .unwrap_or_else(|| panic!("consume_seconds must be a number")) as f32
    });
    assert!(
        consume_seconds >= 0.0 && !consume_seconds.is_nan(),
        "consume_seconds must be non-negative"
    );
    let animation = item_use_animation_token(object.get("animation"));
    let sound = object.get("sound").map_or_else(
        || registry_sound_event_holder_token("minecraft:entity.generic.eat", "consumable.sound"),
        |value| sound_event_value_token(value, "consumable.sound"),
    );
    let has_consume_particles = object.get("has_consume_particles").is_none_or(|value| {
        value
            .as_bool()
            .unwrap_or_else(|| panic!("has_consume_particles must be a boolean"))
    });
    let effects = consume_effects_token(object.get("on_consume_effects"), "on_consume_effects");
    quote! {
        vanilla_components::Consumable::from_extracted(
            #consume_seconds,
            #animation,
            #sound,
            #has_consume_particles,
            vec![#(#effects),*],
        )
    }
}

pub(super) fn death_protection_component_token(value: &Value) -> TokenStream {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("death_protection component must be an object"));
    assert!(
        object.keys().all(|key| key == "death_effects"),
        "death_protection component contains unsupported fields: {value}"
    );
    let effects = consume_effects_token(object.get("death_effects"), "death_effects");
    quote! { vanilla_components::DeathProtection::new(vec![#(#effects),*]) }
}
