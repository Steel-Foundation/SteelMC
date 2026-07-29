use super::{TokenStream, Value, quote, sound_event_value_token};

pub(super) fn kinetic_condition_token(value: Option<&Value>, field: &str) -> TokenStream {
    let Some(value) = value else {
        return quote! { None };
    };
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("kinetic_weapon.{field} must be an object"));
    let max_duration_ticks = object
        .get("max_duration_ticks")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("kinetic_weapon.{field}.max_duration_ticks must be an integer"));
    let max_duration_ticks = i32::try_from(max_duration_ticks)
        .unwrap_or_else(|_| panic!("kinetic_weapon.{field}.max_duration_ticks must fit an i32"));
    assert!(
        max_duration_ticks >= 0,
        "kinetic_weapon.{field}.max_duration_ticks must be non-negative"
    );
    let min_speed = object.get("min_speed").map_or(0.0, |value| {
        value
            .as_f64()
            .unwrap_or_else(|| panic!("kinetic_weapon.{field}.min_speed must be a number"))
            as f32
    });
    let min_relative_speed = object.get("min_relative_speed").map_or(0.0, |value| {
        value
            .as_f64()
            .unwrap_or_else(|| panic!("kinetic_weapon.{field}.min_relative_speed must be a number"))
            as f32
    });
    quote! {
        Some(vanilla_components::KineticWeaponCondition::from_extracted(
            #max_duration_ticks,
            #min_speed,
            #min_relative_speed,
        ))
    }
}

pub(super) fn kinetic_weapon_component_token(value: &Value) -> TokenStream {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("kinetic_weapon component must be an object"));
    let non_negative_i32 = |field: &str, default: i32| {
        let value = object.get(field).map_or(i64::from(default), |value| {
            value
                .as_i64()
                .unwrap_or_else(|| panic!("kinetic_weapon.{field} must be an integer"))
        });
        let value = i32::try_from(value)
            .unwrap_or_else(|_| panic!("kinetic_weapon.{field} must fit an i32"));
        assert!(value >= 0, "kinetic_weapon.{field} must be non-negative");
        value
    };
    let float = |field: &str, default: f32| {
        object.get(field).map_or(default, |value| {
            value
                .as_f64()
                .unwrap_or_else(|| panic!("kinetic_weapon.{field} must be a number"))
                as f32
        })
    };
    let contact_cooldown_ticks = non_negative_i32("contact_cooldown_ticks", 10);
    let delay_ticks = non_negative_i32("delay_ticks", 0);
    let dismount_conditions =
        kinetic_condition_token(object.get("dismount_conditions"), "dismount_conditions");
    let knockback_conditions =
        kinetic_condition_token(object.get("knockback_conditions"), "knockback_conditions");
    let damage_conditions =
        kinetic_condition_token(object.get("damage_conditions"), "damage_conditions");
    let forward_movement = float("forward_movement", 0.0);
    let damage_multiplier = float("damage_multiplier", 1.0);
    let sound = object.get("sound").map_or_else(
        || quote! { None },
        |sound| {
            let sound = sound_event_value_token(sound, "kinetic_weapon.sound");
            quote! { Some(#sound) }
        },
    );
    let hit_sound = object.get("hit_sound").map_or_else(
        || quote! { None },
        |sound| {
            let sound = sound_event_value_token(sound, "kinetic_weapon.hit_sound");
            quote! { Some(#sound) }
        },
    );
    quote! {
        vanilla_components::KineticWeapon::from_extracted(
            #contact_cooldown_ticks,
            #delay_ticks,
            #dismount_conditions,
            #knockback_conditions,
            #damage_conditions,
            #forward_movement,
            #damage_multiplier,
            #sound,
            #hit_sound,
        )
    }
}
