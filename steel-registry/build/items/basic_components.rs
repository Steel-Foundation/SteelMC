use super::{TokenStream, Value, identifier_token, quote, sound_event_value_token};

pub(super) fn food_component_token(value: &Value) -> TokenStream {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("food component must be an object"));
    let nutrition = object
        .get("nutrition")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("food.nutrition must be an integer"));
    let nutrition =
        i32::try_from(nutrition).unwrap_or_else(|_| panic!("food.nutrition must fit an i32"));
    assert!(nutrition >= 0, "food.nutrition must be non-negative");
    let saturation = object
        .get("saturation")
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic!("food.saturation must be a number")) as f32;
    let can_always_eat = object.get("can_always_eat").is_some_and(|value| {
        value
            .as_bool()
            .unwrap_or_else(|| panic!("food.can_always_eat must be a boolean"))
    });
    quote! {
        vanilla_components::FoodProperties::from_extracted(
            #nutrition,
            #saturation,
            #can_always_eat,
        )
    }
}

pub(super) fn block_state_component_token(value: &Value) -> TokenStream {
    let properties = value
        .as_object()
        .unwrap_or_else(|| panic!("block_state component must be an object"))
        .iter()
        .map(|(name, value)| {
            let value = value
                .as_str()
                .unwrap_or_else(|| panic!("block_state.{name} must be a string"));
            quote! { (#name.to_owned(), #value.to_owned()) }
        });
    quote! {
        vanilla_components::BlockItemStateProperties::new(
            BTreeMap::from([#(#properties),*]),
        )
    }
}

pub(super) fn fireworks_component_token(value: &Value) -> TokenStream {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("fireworks component must be an object"));
    let flight_duration = object.get("flight_duration").map_or(0, |value| {
        let value = value
            .as_i64()
            .unwrap_or_else(|| panic!("fireworks.flight_duration must be an integer"));
        i32::try_from(value).unwrap_or_else(|_| panic!("fireworks.flight_duration must fit an i32"))
    });
    assert!(
        (0..=u8::MAX.into()).contains(&flight_duration),
        "fireworks.flight_duration must be in 0..=255"
    );
    assert!(
        object
            .get("explosions")
            .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty)),
        "vanilla item prototypes currently require empty firework explosions"
    );
    quote! { vanilla_components::Fireworks::from_extracted(#flight_duration) }
}

pub(super) fn blocks_attacks_component_token(value: &Value) -> TokenStream {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("blocks_attacks component must be an object"));
    assert!(
        object.keys().all(|key| matches!(
            key.as_str(),
            "block_delay_seconds"
                | "item_damage"
                | "bypassed_by"
                | "block_sound"
                | "disabled_sound"
        )),
        "shield blocks_attacks contains unsupported fields: {value}"
    );
    let block_delay_seconds = object
        .get("block_delay_seconds")
        .and_then(Value::as_f64)
        .unwrap_or(0.0) as f32;
    assert!(
        block_delay_seconds >= 0.0 && !block_delay_seconds.is_nan(),
        "shield block delay must be non-negative"
    );
    let item_damage = object
        .get("item_damage")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("shield blocks_attacks must define item_damage"));
    let item_damage_value = |field: &str| {
        item_damage
            .get(field)
            .and_then(Value::as_f64)
            .unwrap_or_else(|| panic!("blocks_attacks.item_damage.{field} must be a number"))
            as f32
    };
    let threshold = item_damage_value("threshold");
    let base = item_damage_value("base");
    let factor = item_damage_value("factor");
    assert!(
        threshold >= 0.0,
        "item damage threshold must be non-negative"
    );
    let bypassed_by = object
        .get("bypassed_by")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("shield blocks_attacks must define bypassed_by"));
    let bypassed_by = bypassed_by
        .strip_prefix('#')
        .unwrap_or_else(|| panic!("shield bypassed_by must be a damage-type tag"));
    let bypassed_by = identifier_token(bypassed_by);
    let block_sound = sound_event_value_token(
        object
            .get("block_sound")
            .unwrap_or_else(|| panic!("shield blocks_attacks must define block_sound")),
        "blocks_attacks.block_sound",
    );
    let disabled_sound = sound_event_value_token(
        object
            .get("disabled_sound")
            .unwrap_or_else(|| panic!("shield blocks_attacks must define disabled_sound")),
        "blocks_attacks.disabled_sound",
    );
    quote! {
        vanilla_components::BlocksAttacks::from_extracted_shield(
            #block_delay_seconds,
            vanilla_components::ItemDamageFunction::from_extracted(#threshold, #base, #factor),
            crate::RegistryHolderSet::Tag(#bypassed_by),
            #block_sound,
            #disabled_sound,
        )
    }
}
