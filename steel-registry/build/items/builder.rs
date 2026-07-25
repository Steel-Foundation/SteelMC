use super::{
    FromStr, Ident, Identifier, Item, Span, ToShoutySnakeCase, TokenStream, Value,
    banner_pattern_ref_token, block_state_component_token, blocks_attacks_component_token,
    component_i32, consumable_component_token, damage_type_ref_token,
    death_protection_component_token, dye_color_token, entity_type_ref_token,
    fireworks_component_token, food_component_token, generate_allowed_entities,
    generate_attack_range_component, generate_attribute_modifiers_component,
    generate_piercing_weapon_component, generate_tool_component, generate_weapon_component,
    get_component_ident, holder_set_component_field, holder_set_token, identifier_token,
    instrument_ref_token, item_name_component_token, item_ref_token, jukebox_song_ref_token,
    kinetic_weapon_component_token, optional_identifier_token, quote, rarity_component_token,
    sound_event_holder_token, sound_event_value_token, swing_animation_component_token,
    trim_material_ref_token, use_effects_component_token,
};

/// Returns the crafting remainder item key for a given item, if any.
/// Based on vanilla Minecraft's `Item.Properties.craftRemainder()` calls.
pub(super) fn get_craft_remainder(item_name: &str) -> Option<&'static str> {
    match item_name {
        // Buckets return empty bucket
        "water_bucket"
        | "lava_bucket"
        | "milk_bucket"
        | "powder_snow_bucket"
        | "pufferfish_bucket"
        | "salmon_bucket"
        | "cod_bucket"
        | "tropical_fish_bucket"
        | "axolotl_bucket"
        | "tadpole_bucket" => Some("bucket"),
        // Bottles return empty glass bottle
        "dragon_breath" | "honey_bottle" => Some("glass_bottle"),
        // Potions also return glass bottles when used in crafting
        "potion" => Some("glass_bottle"),
        _ => None,
    }
}

pub(super) fn generate_builder_calls(item: &Item) -> Vec<TokenStream> {
    let mut builder_calls = Vec::new();

    for (key, value) in &item.components {
        let component_ident = get_component_ident(key);

        match key.as_str() {
            "minecraft:item_name" => {
                item_name_component_token(value);
            }
            "minecraft:item_model" => {
                let model = value
                    .as_str()
                    .unwrap_or_else(|| panic!("item_model component must be an identifier"));
                let model = Identifier::from_str(model)
                    .unwrap_or_else(|error| panic!("invalid item_model {model:?}: {error}"));
                assert_eq!(
                    model,
                    Identifier::vanilla(item.name.clone()),
                    "vanilla 26.2 item model must default to its item key"
                );
            }
            "minecraft:bucket_entity_data" => {
                assert!(
                    value.as_object().is_some_and(serde_json::Map::is_empty),
                    "vanilla bucket_entity_data item prototype must be empty, got {value}"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::BUCKET_ENTITY_DATA,
                        Some(vanilla_components::CustomData::default()),
                    )
                });
            }
            "minecraft:entity_data" => {
                let object = value
                    .as_object()
                    .unwrap_or_else(|| panic!("entity_data component must be an object"));
                assert_eq!(
                    object.len(),
                    1,
                    "extracted entity_data prototypes currently require an id-only value"
                );
                let entity_type = object
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("entity_data.id must be an entity type identifier"));
                let entity_type = entity_type_ref_token(entity_type).unwrap_or_else(|| {
                    panic!("vanilla entity_data references non-vanilla type {entity_type}")
                });
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::ENTITY_DATA,
                        Some(vanilla_components::EntityData::new(
                            #entity_type,
                            vanilla_components::CustomData::default(),
                        )),
                    )
                });
            }
            "minecraft:debug_stick_state" => {
                assert!(
                    value.as_object().is_some_and(serde_json::Map::is_empty),
                    "vanilla debug stick prototype must have an empty state"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::DEBUG_STICK_STATE,
                        Some(vanilla_components::DebugStickState::empty()),
                    )
                });
            }
            "minecraft:writable_book_content" => {
                assert!(
                    value.as_object().is_some_and(serde_json::Map::is_empty),
                    "vanilla writable book prototype must have empty content"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::WRITABLE_BOOK_CONTENT,
                        Some(vanilla_components::WritableBookContent::empty()),
                    )
                });
            }
            "minecraft:suspicious_stew_effects" => {
                assert!(
                    value.as_array().is_some_and(Vec::is_empty),
                    "vanilla suspicious stew prototype must have empty effects"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::SUSPICIOUS_STEW_EFFECTS,
                        Some(vanilla_components::SuspiciousStewEffects::empty()),
                    )
                });
            }
            "minecraft:potion_contents" => {
                assert!(
                    value.as_object().is_some_and(serde_json::Map::is_empty),
                    "vanilla potion item prototypes must have empty potion contents"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::POTION_CONTENTS,
                        Some(vanilla_components::PotionContents::empty()),
                    )
                });
            }
            "minecraft:potion_duration_scale" => {
                let scale = value
                    .as_f64()
                    .unwrap_or_else(|| panic!("potion_duration_scale component must be a number"))
                    as f32;
                assert!(
                    scale.is_finite() && !scale.is_sign_negative(),
                    "potion_duration_scale must be non-negative and finite"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::POTION_DURATION_SCALE,
                        Some(#scale),
                    )
                });
            }
            "minecraft:food" => {
                let food = food_component_token(value);
                builder_calls.push(quote! { .builder_set(vanilla_components::FOOD, Some(#food)) });
            }
            "minecraft:fireworks" => {
                let fireworks = fireworks_component_token(value);
                builder_calls
                    .push(quote! { .builder_set(vanilla_components::FIREWORKS, Some(#fireworks)) });
            }
            "minecraft:block_state" => {
                let block_state = block_state_component_token(value);
                builder_calls.push(
                    quote! { .builder_set(vanilla_components::BLOCK_STATE, Some(#block_state)) },
                );
            }
            "minecraft:blocks_attacks" => {
                let blocks_attacks = blocks_attacks_component_token(value);
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::BLOCKS_ATTACKS,
                        Some(#blocks_attacks),
                    )
                });
            }
            "minecraft:consumable" => {
                let consumable = consumable_component_token(value);
                builder_calls.push(
                    quote! { .builder_set(vanilla_components::CONSUMABLE, Some(#consumable)) },
                );
            }
            "minecraft:death_protection" => {
                let death_protection = death_protection_component_token(value);
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::DEATH_PROTECTION,
                        Some(#death_protection),
                    )
                });
            }
            "minecraft:bees" => {
                assert!(
                    value.as_array().is_some_and(Vec::is_empty),
                    "vanilla beehive item prototypes currently require empty bee occupants"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::BEES,
                        Some(vanilla_components::Bees::empty()),
                    )
                });
            }
            "minecraft:chicken/variant" => {
                let variant = value.as_str().unwrap_or_else(|| {
                    panic!("chicken/variant component must be an identifier string")
                });
                let variant = Identifier::from_str(variant)
                    .unwrap_or_else(|error| panic!("invalid chicken variant {variant:?}: {error}"));
                assert_eq!(
                    variant.namespace.as_ref(),
                    "minecraft",
                    "vanilla item prototype references non-vanilla chicken variant {variant}"
                );
                let variant = Ident::new(&variant.path.to_shouty_snake_case(), Span::call_site());
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::CHICKEN_VARIANT,
                        Some(vanilla_components::RegistryReference::new(
                            &crate::vanilla_chicken_variants::#variant,
                        )),
                    )
                });
            }
            "minecraft:kinetic_weapon" => {
                let kinetic_weapon = kinetic_weapon_component_token(value);
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::KINETIC_WEAPON,
                        Some(#kinetic_weapon),
                    )
                });
            }
            "minecraft:map_decorations" => {
                assert!(
                    value.as_object().is_some_and(serde_json::Map::is_empty),
                    "vanilla filled map prototype must have empty map decorations"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::MAP_DECORATIONS,
                        Some(vanilla_components::MapDecorations::EMPTY),
                    )
                });
            }
            "minecraft:enchantable" => {
                let object = value
                    .as_object()
                    .unwrap_or_else(|| panic!("enchantable component must be an object"));
                assert_eq!(
                    object.len(),
                    1,
                    "enchantable component must contain only its value"
                );
                let value = object
                    .get("value")
                    .and_then(Value::as_i64)
                    .unwrap_or_else(|| panic!("enchantable.value must be an integer"));
                let value = i32::try_from(value)
                    .unwrap_or_else(|_| panic!("enchantable.value must fit an i32"));
                assert!(value > 0, "enchantable.value must be positive");
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::ENCHANTABLE,
                        Some(vanilla_components::Enchantable::from_extracted_value(#value)),
                    )
                });
            }
            "minecraft:damage_resistant" => {
                let types = holder_set_component_field(value, "damage_resistant", "types");
                let types = holder_set_token(types, "damage_resistant", damage_type_ref_token);
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::DAMAGE_RESISTANT,
                        Some(vanilla_components::DamageResistant::new(#types)),
                    )
                });
            }
            "minecraft:repairable" => {
                let items = holder_set_component_field(value, "repairable", "items");
                let items = holder_set_token(items, "repairable", |item| {
                    item_ref_token(item, "repairable")
                });
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::REPAIRABLE,
                        Some(vanilla_components::Repairable::new(#items)),
                    )
                });
            }
            "minecraft:dye" => {
                let color = dye_color_token(value);
                builder_calls.push(quote! {
                    .builder_set(vanilla_components::DYE, Some(#color))
                });
            }
            "minecraft:map_color" => {
                let rgb = component_i32(value, "map_color");
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::MAP_COLOR,
                        Some(vanilla_components::MapItemColor::new(#rgb)),
                    )
                });
            }
            "minecraft:ominous_bottle_amplifier" => {
                let amplifier = component_i32(value, "ominous_bottle_amplifier");
                assert!(
                    (0..=4).contains(&amplifier),
                    "ominous_bottle_amplifier must be in 0..=4"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::OMINOUS_BOTTLE_AMPLIFIER,
                        Some(vanilla_components::OminousBottleAmplifier::new(#amplifier)),
                    )
                });
            }
            "minecraft:instrument" => {
                let instrument = instrument_ref_token(value);
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::INSTRUMENT,
                        Some(vanilla_components::InstrumentComponent::new(
                            crate::RegistryHolder::reference(#instrument),
                        )),
                    )
                });
            }
            "minecraft:provides_trim_material" => {
                let material = trim_material_ref_token(value);
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::PROVIDES_TRIM_MATERIAL,
                        Some(vanilla_components::ProvidesTrimMaterial::new(
                            crate::RegistryHolder::reference(#material),
                        )),
                    )
                });
            }
            "minecraft:jukebox_playable" => {
                let song = jukebox_song_ref_token(value);
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::JUKEBOX_PLAYABLE,
                        Some(vanilla_components::JukeboxPlayable::new(
                            #song,
                        )),
                    )
                });
            }
            "minecraft:provides_banner_patterns" => {
                let patterns =
                    holder_set_token(value, "provides_banner_patterns", banner_pattern_ref_token);
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::PROVIDES_BANNER_PATTERNS,
                        Some(#patterns),
                    )
                });
            }
            "minecraft:recipes" => {
                assert!(
                    value.as_array().is_some_and(Vec::is_empty),
                    "vanilla item prototypes currently require empty recipes, got {value}"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::RECIPES,
                        Some(vanilla_components::Recipes::empty()),
                    )
                });
            }
            "minecraft:banner_patterns" => {
                assert!(
                    value.as_array().is_some_and(Vec::is_empty),
                    "vanilla item prototypes currently require empty banner patterns, got {value}"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::BANNER_PATTERNS,
                        Some(vanilla_components::BannerPatternLayers::empty()),
                    )
                });
            }
            "minecraft:pot_decorations" => {
                let decorations = value
                    .as_array()
                    .unwrap_or_else(|| panic!("pot_decorations must be an item list, got {value}"));
                assert!(
                    decorations.len() == 4
                        && decorations
                            .iter()
                            .all(|decoration| decoration.as_str() == Some("minecraft:brick")),
                    "extracted decorated pot must use four brick placeholders"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::POT_DECORATIONS,
                        Some(vanilla_components::PotDecorations::EMPTY),
                    )
                });
            }
            "minecraft:use_remainder" => {
                let remainder = value.as_object().unwrap_or_else(|| {
                    panic!("use_remainder must be an item template, got {value}")
                });
                assert_eq!(
                    remainder.len(),
                    1,
                    "extracted use remainders currently require an item-only template"
                );
                let item = remainder
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("use_remainder.id must be an item identifier"));
                let item = item_ref_token(item, "use_remainder");
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::USE_REMAINDER,
                        Some(vanilla_components::UseRemainder::new(
                            vanilla_components::ItemStackTemplate::new(#item),
                        )),
                    )
                });
            }
            "minecraft:charged_projectiles" => {
                assert!(
                    value.as_array().is_some_and(Vec::is_empty),
                    "vanilla item prototypes currently require empty charged projectiles, got {value}"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::CHARGED_PROJECTILES,
                        Some(vanilla_components::ChargedProjectiles::empty()),
                    )
                });
            }
            "minecraft:bundle_contents" => {
                assert!(
                    value.as_array().is_some_and(Vec::is_empty),
                    "vanilla item prototypes currently require empty bundle contents, got {value}"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::BUNDLE_CONTENTS,
                        Some(vanilla_components::BundleContents::empty()),
                    )
                });
            }
            "minecraft:container" => {
                assert!(
                    value.as_array().is_some_and(Vec::is_empty),
                    "vanilla item prototypes currently require empty container contents, got {value}"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::CONTAINER,
                        Some(vanilla_components::ItemContainerContents::empty()),
                    )
                });
            }
            "minecraft:tooltip_style" | "minecraft:note_block_sound" => {
                let identifier = value
                    .as_str()
                    .unwrap_or_else(|| panic!("{key} component must be an identifier string"));
                let identifier = identifier_token(identifier);
                builder_calls.push(quote! {
                    .builder_set(vanilla_components::#component_ident, Some(#identifier))
                });
            }
            "minecraft:max_stack_size" => {
                let val = value.as_i64().unwrap() as i32;
                if val != 64 {
                    builder_calls.push(
                        quote! { .builder_set(vanilla_components::#component_ident, Some(#val)) },
                    );
                }
            }
            "minecraft:max_damage" => {
                let val = value.as_i64().unwrap() as i32;
                builder_calls.push(
                    quote! { .builder_set(vanilla_components::#component_ident, Some(#val)) },
                );
            }
            "minecraft:damage" => {
                let val = value.as_i64().unwrap() as i32;
                builder_calls.push(
                    quote! { .builder_set(vanilla_components::#component_ident, Some(#val)) },
                );
            }
            "minecraft:repair_cost" => {
                let val = value.as_i64().unwrap() as i32;
                if val != 0 {
                    builder_calls.push(
                        quote! { .builder_set(vanilla_components::#component_ident, Some(#val)) },
                    );
                }
            }
            "minecraft:use_effects" => {
                if let Some(use_effects) = use_effects_component_token(value) {
                    builder_calls.push(quote! {
                        .builder_set(vanilla_components::USE_EFFECTS, Some(#use_effects))
                    });
                }
            }
            "minecraft:lore" => {
                assert!(
                    value.as_array().is_some_and(Vec::is_empty),
                    "vanilla item prototypes currently require empty lore, got {value}"
                );
            }
            "minecraft:enchantments" => {
                assert!(
                    value.as_object().is_some_and(serde_json::Map::is_empty),
                    "vanilla item prototypes currently require default empty enchantments, got {value}"
                );
            }
            "minecraft:stored_enchantments" => {
                assert!(
                    value.as_object().is_some_and(serde_json::Map::is_empty),
                    "vanilla item prototypes currently require empty stored enchantments, got {value}"
                );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::STORED_ENCHANTMENTS,
                        Some(vanilla_components::ItemEnchantments::empty()),
                    )
                });
            }
            "minecraft:rarity" => {
                if let Some(rarity) = rarity_component_token(value) {
                    builder_calls
                        .push(quote! { .builder_set(vanilla_components::RARITY, Some(#rarity)) });
                }
            }
            "minecraft:tooltip_display" => {
                assert!(
                    value.as_object().is_some_and(serde_json::Map::is_empty),
                    "vanilla item prototypes currently require the default tooltip display, got {value}"
                );
            }
            "minecraft:swing_animation" => {
                if let Some(swing_animation) = swing_animation_component_token(value) {
                    builder_calls.push(quote! {
                        .builder_set(
                            vanilla_components::SWING_ANIMATION,
                            Some(#swing_animation),
                        )
                    });
                }
            }
            "minecraft:break_sound" => {
                if value.as_str() != Some("minecraft:entity.item.break") {
                    let break_sound = sound_event_value_token(value, "break_sound");
                    builder_calls.push(quote! {
                        .builder_set(vanilla_components::BREAK_SOUND, Some(#break_sound))
                    });
                }
            }
            "minecraft:unbreakable" => {
                builder_calls
                    .push(quote! { .builder_set(vanilla_components::#component_ident, Some(())) });
            }
            "minecraft:glider" => {
                builder_calls
                    .push(quote! { .builder_set(vanilla_components::#component_ident, Some(())) });
            }
            "minecraft:enchantment_glint_override" => {
                let val = value.as_bool().unwrap();
                builder_calls.push(
                    quote! { .builder_set(vanilla_components::#component_ident, Some(#val)) },
                );
            }
            "minecraft:equippable" => {
                // Parse the equippable component to get the slot
                if let Some(slot_str) = value.get("slot").and_then(|s| s.as_str()) {
                    let slot_variant = match slot_str {
                        "head" => quote! { vanilla_components::EquipmentSlot::Head },
                        "chest" => quote! { vanilla_components::EquipmentSlot::Chest },
                        "legs" => quote! { vanilla_components::EquipmentSlot::Legs },
                        "feet" => quote! { vanilla_components::EquipmentSlot::Feet },
                        "body" => quote! { vanilla_components::EquipmentSlot::Body },
                        "mainhand" => quote! { vanilla_components::EquipmentSlot::MainHand },
                        "offhand" => quote! { vanilla_components::EquipmentSlot::OffHand },
                        "saddle" => quote! { vanilla_components::EquipmentSlot::Saddle },
                        _ => panic!("unknown equippable slot {slot_str:?}"),
                    };
                    let allowed_entities = generate_allowed_entities(value);
                    let equip_sound = sound_event_holder_token(
                        value,
                        "equip_sound",
                        "minecraft:item.armor.equip_generic",
                    );
                    let asset_id = optional_identifier_token(value, "asset_id");
                    let camera_overlay = optional_identifier_token(value, "camera_overlay");
                    let dispensable = value
                        .get("dispensable")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true);
                    let swappable = value
                        .get("swappable")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true);
                    let damage_on_hurt = value
                        .get("damage_on_hurt")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true);
                    let equip_on_interact = value
                        .get("equip_on_interact")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let can_be_sheared = value
                        .get("can_be_sheared")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let shearing_sound = sound_event_holder_token(
                        value,
                        "shearing_sound",
                        "minecraft:item.shears.snip",
                    );
                    builder_calls.push(quote! {
                        .builder_set(
                            vanilla_components::EQUIPPABLE,
                            Some(vanilla_components::Equippable {
                                slot: #slot_variant,
                                equip_sound: #equip_sound,
                                asset_id: #asset_id,
                                camera_overlay: #camera_overlay,
                                allowed_entities: #allowed_entities,
                                dispensable: #dispensable,
                                swappable: #swappable,
                                damage_on_hurt: #damage_on_hurt,
                                equip_on_interact: #equip_on_interact,
                                can_be_sheared: #can_be_sheared,
                                shearing_sound: #shearing_sound,
                            }),
                        )
                    });
                }
            }
            "minecraft:tool" => {
                let tool_token = generate_tool_component(value);
                builder_calls
                    .push(quote! { .builder_set(vanilla_components::TOOL, Some(#tool_token)) });
            }
            "minecraft:attribute_modifiers" => {
                if let Some(modifiers) = generate_attribute_modifiers_component(value) {
                    builder_calls.push(quote! {
                        .builder_set(vanilla_components::ATTRIBUTE_MODIFIERS, Some(#modifiers))
                    });
                }
            }
            "minecraft:minimum_attack_charge" => {
                let val = value
                    .as_f64()
                    .expect("minimum_attack_charge component must be a number")
                    as f32;
                builder_calls.push(
                    quote! { .builder_set(vanilla_components::MINIMUM_ATTACK_CHARGE, Some(#val)) },
                );
            }
            "minecraft:damage_type" => {
                let damage_type = value
                    .as_str()
                    .expect("damage_type component must be an identifier string");
                let damage_type = damage_type_ref_token(damage_type);
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::DAMAGE_TYPE,
                        Some(vanilla_components::DamageTypeComponent::new(#damage_type)),
                    )
                });
            }
            "minecraft:use_cooldown" => {
                let seconds = value
                    .get("seconds")
                    .and_then(Value::as_f64)
                    .expect("use_cooldown.seconds must be a number")
                    as f32;
                let cooldown_group = value
                    .get("cooldown_group")
                    .and_then(Value::as_str)
                    .map_or_else(
                        || quote! { None },
                        |group| {
                            let id = Identifier::from_str(group)
                                .expect("use_cooldown.cooldown_group must be an identifier");
                            let namespace = id.namespace.as_ref();
                            let path = id.path.as_ref();
                            quote! { Some(Identifier::new_static(#namespace, #path)) }
                        },
                    );
                builder_calls.push(quote! {
                    .builder_set(
                        vanilla_components::USE_COOLDOWN,
                        Some(vanilla_components::UseCooldown::new(#seconds, #cooldown_group)),
                    )
                });
            }
            "minecraft:weapon" => {
                let weapon = generate_weapon_component(value);
                builder_calls
                    .push(quote! { .builder_set(vanilla_components::WEAPON, Some(#weapon)) });
            }
            "minecraft:attack_range" => {
                let attack_range = generate_attack_range_component(value);
                builder_calls.push(
                    quote! { .builder_set(vanilla_components::ATTACK_RANGE, Some(#attack_range)) },
                );
            }
            "minecraft:piercing_weapon" => {
                let piercing_weapon = generate_piercing_weapon_component(value);
                builder_calls.push(
                    quote! { .builder_set(vanilla_components::PIERCING_WEAPON, Some(#piercing_weapon)) },
                );
            }
            _ => panic!(
                "unsupported extracted component {key} on item {}",
                item.name
            ),
        }
    }

    builder_calls
}
