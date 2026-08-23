#![feature(const_trait_impl, const_cmp, derive_const)]
#![expect(
    missing_docs,
    reason = "registry APIs mirror large generated vanilla data surfaces that are not individually documented yet"
)]
#![expect(
    clippy::absolute_paths,
    clippy::allow_attributes_without_reason,
    clippy::fn_params_excessive_bools,
    clippy::items_after_statements,
    clippy::match_same_arms,
    clippy::missing_fields_in_debug,
    clippy::missing_panics_doc,
    clippy::ref_option,
    clippy::return_self_not_must_use,
    clippy::too_many_lines,
    clippy::trivially_copy_pass_by_ref,
    clippy::unreadable_literal,
    clippy::unused_self,
    reason = "registry model code mirrors vanilla/generated data and keeps existing panic-heavy registry invariants"
)]
pub mod attribute;
pub mod banner_pattern;
pub mod biome;
pub mod block_entity_type;
pub mod blocks;
pub mod carver;
pub mod cat_sound_variant;
pub mod cat_variant;
pub mod chat_type;
pub mod chicken_sound_variant;
pub mod chicken_variant;
pub mod consume_effect;
pub mod cow_sound_variant;
pub mod cow_variant;
pub mod damage_type;
pub mod data_component_predicate;
pub mod data_components;
pub mod dialog;
pub mod dimension_type;
pub mod dye_color;
pub mod enchantment;
pub use enchantment::effect as enchantment_effect;
pub mod entity_data;
pub mod entity_type;
pub mod entity_variant;
pub mod equipment;
pub mod feature;
pub mod fluid;
pub mod frog_variant;
pub mod game_events;
pub mod game_rules;
pub mod instrument;
pub mod item_predicate;
pub mod item_stack;
pub mod item_stack_template;
pub mod items;
pub mod jukebox_song;
pub mod loot_table;
mod macros;
pub mod map_decoration_type;
pub mod menu_type;
pub mod mob_effect;
pub use mob_effect::instance as mob_effect_instance;
pub mod painting_variant;
pub mod particle_type;
pub mod pig_sound_variant;
pub mod pig_variant;
pub mod poi;
pub mod position_source;
pub mod potion;
pub mod recipe;
pub mod registry;
pub use registry::holder;
pub use registry::holder_set;
pub use registry::reference as registry_reference;
pub use registry::*;
pub mod resolvable_profile;
pub mod sound_event;
pub mod stat;
pub mod structure;
pub use structure::processor as structure_processor;
pub use structure::set as structure_set;
pub use structure::template_pool;
pub mod timeline;
pub mod trim_material;
pub mod trim_pattern;
pub mod villager_profession;
pub mod villager_type;
pub mod wolf_sound_variant;
pub mod wolf_variant;
pub mod world_clock;
pub mod zombie_nautilus_variant;

pub use consume_effect::{ConsumeEffectData, ConsumeEffectType, ConsumeEffectTypeRef};
pub use dye_color::DyeColor;
pub use entity_variant::{
    AxolotlVariant, FoxVariant, HorseVariant, LlamaVariant, MooshroomVariant, ParrotVariant,
    RabbitVariant, SalmonVariant, TropicalFishBase, TropicalFishPattern,
};
pub use item_stack_template::ItemStackTemplate;
pub use mob_effect_instance::{MobEffectInstance, MobEffectInstanceDetails};
pub use potion::{Potion, PotionEffect, PotionRef};
pub use resolvable_profile::{
    PartialProfile, PlayerModelType, PlayerSkinPatch, ProfileProperty, ResolvableProfile,
    ResolvableProfileContents, StoredGameProfile,
};

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_attributes.rs"]
pub mod vanilla_attributes;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_blocks.rs"]
pub mod vanilla_blocks;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_block_tags.rs"]
pub mod vanilla_block_tags;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_banner_patterns.rs"]
pub mod vanilla_banner_patterns;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_items.rs"]
pub mod vanilla_items;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_item_tags.rs"]
pub mod vanilla_item_tags;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_biomes.rs"]
pub mod vanilla_biomes;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_biome_tags.rs"]
pub mod vanilla_biome_tags;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_chat_types.rs"]
pub mod vanilla_chat_types;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_trim_patterns.rs"]
pub mod vanilla_trim_patterns;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_trim_materials.rs"]
pub mod vanilla_trim_materials;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_wolf_variants.rs"]
pub mod vanilla_wolf_variants;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_wolf_sound_variants.rs"]
pub mod vanilla_wolf_sound_variants;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_pig_variants.rs"]
pub mod vanilla_pig_variants;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_pig_sound_variants.rs"]
pub mod vanilla_pig_sound_variants;

#[allow(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_chicken_sound_variants.rs"]
pub mod vanilla_chicken_sound_variants;

#[allow(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_cat_sound_variants.rs"]
pub mod vanilla_cat_sound_variants;

#[allow(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_cow_sound_variants.rs"]
pub mod vanilla_cow_sound_variants;

#[allow(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_frog_variants.rs"]
pub mod vanilla_frog_variants;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_cat_variants.rs"]
pub mod vanilla_cat_variants;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_cow_variants.rs"]
pub mod vanilla_cow_variants;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_chicken_variants.rs"]
pub mod vanilla_chicken_variants;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_painting_variants.rs"]
pub mod vanilla_painting_variants;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_particle_types.rs"]
pub mod vanilla_particle_types;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_position_source_types.rs"]
pub mod vanilla_position_source_types;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_villager_types.rs"]
pub mod vanilla_villager_types;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_villager_professions.rs"]
pub mod vanilla_villager_professions;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_dimension_types.rs"]
pub mod vanilla_dimension_types;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_damage_types.rs"]
pub mod vanilla_damage_types;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_damage_type_tags.rs"]
pub mod vanilla_damage_type_tags;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_jukebox_songs.rs"]
pub mod vanilla_jukebox_songs;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_instruments.rs"]
pub mod vanilla_instruments;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_dialogs.rs"]
pub mod vanilla_dialogs;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_dialog_tags.rs"]
pub mod vanilla_dialog_tags;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_menu_types.rs"]
pub mod vanilla_menu_types;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_mob_effects.rs"]
pub mod vanilla_mob_effects;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_map_decoration_types.rs"]
pub mod vanilla_map_decoration_types;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_potions.rs"]
pub mod vanilla_potions;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_zombie_nautilus_variants.rs"]
pub mod vanilla_zombie_nautilus_variants;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_timelines.rs"]
pub mod vanilla_timelines;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_timeline_tags.rs"]
pub mod vanilla_timeline_tags;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_recipes.rs"]
pub mod vanilla_recipes;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_entities.rs"]
pub mod vanilla_entities;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_entity_data.rs"]
pub mod vanilla_entity_data;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_fluids.rs"]
pub mod vanilla_fluids;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_poi_types.rs"]
pub mod vanilla_poi_types;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_banner_pattern_tags.rs"]
pub mod vanilla_banner_pattern_tags;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_entity_type_tags.rs"]
pub mod vanilla_entity_type_tags;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_enchantment_tags.rs"]
pub mod vanilla_enchantment_tags;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_potion_tags.rs"]
pub mod vanilla_potion_tags;
#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_enchantments.rs"]
pub mod vanilla_enchantments;

#[allow(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_instrument_tags.rs"]
pub mod vanilla_instrument_tags;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_painting_variant_tags.rs"]
pub mod vanilla_painting_variant_tags;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_poi_type_tags.rs"]
pub mod vanilla_poi_type_tags;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_fluid_tags.rs"]
pub mod vanilla_fluid_tags;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_loot_tables.rs"]
pub mod vanilla_loot_tables;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_block_entity_types.rs"]
pub mod vanilla_block_entity_types;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_game_rules.rs"]
pub mod vanilla_game_rules;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_game_events.rs"]
pub mod vanilla_game_events;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_level_events.rs"]
pub mod level_events;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_sound_events.rs"]
pub mod sound_events;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_sound_types.rs"]
pub mod sound_types;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_structures.rs"]
pub mod vanilla_structures;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_structure_tags.rs"]
pub mod vanilla_structure_tags;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_structure_sets.rs"]
pub mod vanilla_structure_sets;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_structure_processors.rs"]
pub mod vanilla_structure_processors;

#[expect(
    clippy::doc_markdown,
    clippy::must_use_candidate,
    reason = "generated vanilla template pool data is emitted by the registry build script"
)]
#[rustfmt::skip]
#[path = "generated/vanilla_template_pools.rs"]
pub mod vanilla_template_pools;

#[allow(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_packets.rs"]
pub mod packets;

#[allow(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_world_clocks.rs"]
pub mod vanilla_world_clocks;
pub mod shared_structs;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_configured_carvers.rs"]
pub mod vanilla_configured_carvers;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_configured_features.rs"]
pub mod vanilla_configured_features;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_placed_features.rs"]
pub mod vanilla_placed_features;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_custom_stats.rs"]
pub mod vanilla_custom_stats;
