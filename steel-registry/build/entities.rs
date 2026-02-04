use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Deserialize)]
struct EntityTypeEntry {
    name: String,
    width: f32,
    height: f32,
    eye_height: f32,
    fixed: bool,
    mob_category: String,
    client_tracking_range: i32,
    update_interval: i32,
    fire_immune: bool,
    summonable: bool,
    can_spawn_far_from_player: bool,
    #[serde(default = "default_can_serialize")]
    can_serialize: bool,
    #[serde(default)]
    flags: Option<FlagsEntry>,
}

fn default_can_serialize() -> bool {
    true
}

#[derive(Deserialize)]
struct FlagsEntry {
    is_pushable: bool,
    is_attackable: bool,
    is_pickable: bool,
    can_be_collided_with: bool,
    is_pushed_by_fluid: bool,
    can_freeze: bool,
    can_be_hit_by_projectile: bool,
    #[serde(default)]
    is_sensitive_to_water: bool,
    #[serde(default)]
    can_breathe_underwater: bool,
    #[serde(default)]
    can_be_seen_as_enemy: bool,
}

fn mob_category_variant(category: &str) -> TokenStream {
    match category {
        "MONSTER" => quote! { MobCategory::Monster },
        "CREATURE" => quote! { MobCategory::Creature },
        "AMBIENT" => quote! { MobCategory::Ambient },
        "AXOLOTLS" => quote! { MobCategory::Axolotls },
        "UNDERGROUND_WATER_CREATURE" => quote! { MobCategory::UndergroundWaterCreature },
        "WATER_CREATURE" => quote! { MobCategory::WaterCreature },
        "WATER_AMBIENT" => quote! { MobCategory::WaterAmbient },
        "MISC" => quote! { MobCategory::Misc },
        _ => panic!("Unknown mob category: {}", category),
    }
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/entities.json");

    let entities_file = "build_assets/entities.json";
    let content = fs::read_to_string(entities_file).unwrap();
    let entity_types: Vec<EntityTypeEntry> = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse entities.json: {}", e));

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::entity_types::{EntityDimensions, EntityFlags, EntityType, EntityTypeRegistry, MobCategory};
        use steel_utils::Identifier;
    });

    for entity_type in &entity_types {
        let entity_type_ident =
            Ident::new(&entity_type.name.to_shouty_snake_case(), Span::call_site());
        let entity_type_key = &entity_type.name;
        let client_tracking_range = entity_type.client_tracking_range;
        let update_interval = entity_type.update_interval;

        // Dimensions
        let width = Literal::f32_suffixed(entity_type.width);
        let height = Literal::f32_suffixed(entity_type.height);
        let eye_height = Literal::f32_suffixed(entity_type.eye_height);
        let fixed = entity_type.fixed;

        // Classification
        let mob_category = mob_category_variant(&entity_type.mob_category);
        let fire_immune = entity_type.fire_immune;
        let summonable = entity_type.summonable;
        let can_spawn_far = entity_type.can_spawn_far_from_player;
        let can_serialize = entity_type.can_serialize;

        // Flags (with defaults for entities that don't have them, like fishing_bobber)
        let flags = entity_type.flags.as_ref();
        let is_pushable = flags.is_some_and(|f| f.is_pushable);
        let is_attackable = flags.is_some_and(|f| f.is_attackable);
        let is_pickable = flags.is_some_and(|f| f.is_pickable);
        let can_be_collided_with = flags.is_some_and(|f| f.can_be_collided_with);
        let is_pushed_by_fluid = flags.is_none_or(|f| f.is_pushed_by_fluid);
        let can_freeze = flags.is_none_or(|f| f.can_freeze);
        let can_be_hit_by_projectile = flags.is_some_and(|f| f.can_be_hit_by_projectile);
        let is_sensitive_to_water = flags.is_some_and(|f| f.is_sensitive_to_water);
        let can_breathe_underwater = flags.is_some_and(|f| f.can_breathe_underwater);
        let can_be_seen_as_enemy = flags.is_some_and(|f| f.can_be_seen_as_enemy);

        stream.extend(quote! {
            pub static #entity_type_ident: &EntityType = &EntityType {
                key: Identifier::vanilla_static(#entity_type_key),
                client_tracking_range: #client_tracking_range,
                update_interval: #update_interval,
                dimensions: EntityDimensions::new(#width, #height, #eye_height),
                fixed: #fixed,
                mob_category: #mob_category,
                fire_immune: #fire_immune,
                summonable: #summonable,
                can_spawn_far_from_player: #can_spawn_far,
                can_serialize: #can_serialize,
                flags: EntityFlags {
                    is_pushable: #is_pushable,
                    is_attackable: #is_attackable,
                    is_pickable: #is_pickable,
                    can_be_collided_with: #can_be_collided_with,
                    is_pushed_by_fluid: #is_pushed_by_fluid,
                    can_freeze: #can_freeze,
                    can_be_hit_by_projectile: #can_be_hit_by_projectile,
                    is_sensitive_to_water: #is_sensitive_to_water,
                    can_breathe_underwater: #can_breathe_underwater,
                    can_be_seen_as_enemy: #can_be_seen_as_enemy,
                },
            };
        });
    }

    let mut register_stream = TokenStream::new();
    for entity_type in &entity_types {
        let entity_type_ident =
            Ident::new(&entity_type.name.to_shouty_snake_case(), Span::call_site());
        register_stream.extend(quote! {
            registry.register(#entity_type_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_entity_types(registry: &mut EntityTypeRegistry) {
            #register_stream
        }
    });

    stream
}
