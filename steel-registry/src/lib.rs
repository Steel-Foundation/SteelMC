#![feature(const_trait_impl, const_cmp, derive_const)]

use crate::world_clock::WorldClockRegistry;
use crate::{
    banner_pattern::BannerPatternRegistry,
    biome::BiomeRegistry,
    block_entity_type::BlockEntityTypeRegistry,
    blocks::BlockRegistry,
    cat_sound_variant::CatSoundVariantRegistry,
    cat_variant::CatVariantRegistry,
    chat_type::ChatTypeRegistry,
    chicken_sound_variant::ChickenSoundVariantRegistry,
    chicken_variant::ChickenVariantRegistry,
    cow_sound_variant::CowSoundVariantRegistry,
    cow_variant::CowVariantRegistry,
    damage_type::DamageTypeRegistry,
    data_components::{DataComponentRegistry, vanilla_components},
    dialog::DialogRegistry,
    dimension_type::DimensionTypeRegistry,
    entity_data::{EntityDataSerializerRegistry, register_vanilla_entity_data_serializers},
    entity_types::EntityTypeRegistry,
    fluid::FluidRegistry,
    frog_variant::FrogVariantRegistry,
    game_rules::GameRuleRegistry,
    instrument::InstrumentRegistry,
    items::ItemRegistry,
    jukebox_song::JukeboxSongRegistry,
    loot_table::LootTableRegistry,
    menu_type::MenuTypeRegistry,
    painting_variant::PaintingVariantRegistry,
    pig_sound_variant::PigSoundVariantRegistry,
    pig_variant::PigVariantRegistry,
    poi::PoiTypeRegistry,
    recipe::RecipeRegistry,
    timeline::TimelineRegistry,
    trim_material::TrimMaterialRegistry,
    trim_pattern::TrimPatternRegistry,
    wolf_sound_variant::WolfSoundVariantRegistry,
    wolf_variant::WolfVariantRegistry,
    zombie_nautilus_variant::ZombieNautilusVariantRegistry,
};
use std::{fmt::Debug, ops::Deref, sync::OnceLock};
use steel_utils::Identifier;

pub mod banner_pattern;
pub mod biome;
pub mod block_entity_type;
pub mod blocks;
pub mod cat_sound_variant;
pub mod cat_variant;
pub mod chat_type;
pub mod chicken_sound_variant;
pub mod chicken_variant;
pub mod cow_sound_variant;
pub mod cow_variant;
pub mod damage_type;
pub mod data_components;
pub mod dialog;
pub mod dimension_type;
pub mod entity_data;
pub mod entity_types;
pub mod fluid;
pub mod frog_variant;
pub mod game_rules;
pub mod instrument;
pub mod item_stack;
pub mod items;
pub mod jukebox_song;
pub mod loot_table;
pub mod menu_type;
pub mod painting_variant;
pub mod pig_sound_variant;
pub mod pig_variant;
pub mod poi;
pub mod recipe;
pub mod timeline;
pub mod trim_material;
pub mod trim_pattern;
pub mod wolf_sound_variant;
pub mod wolf_variant;
pub mod world_clock;
pub mod zombie_nautilus_variant;

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
#[path = "generated/vanilla_packets.rs"]
pub mod packets;

/// Multi-noise biome parameters for climate-based biome selection.
#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_multi_noise.rs"]
pub mod multi_noise;

/// Noise parameters for world generation.
#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_noise_parameters.rs"]
pub mod noise_parameters;

/// Density functions and noise router for terrain generation.
#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_density_functions/mod.rs"]
pub mod density_functions;

#[allow(warnings)]
#[rustfmt::skip]
#[path = "generated/vanilla_world_clocks.rs"]
pub mod vanilla_world_clocks;

pub struct RegistryLock(OnceLock<Registry>);

impl RegistryLock {
    #[expect(clippy::result_large_err)]
    pub fn init(&self, value: Registry) -> Result<(), Registry> {
        self.0.set(value)
    }

    #[cfg(test)]
    pub(crate) fn get_or_init(&self, f: impl FnOnce() -> Registry) -> &Registry {
        self.0.get_or_init(f)
    }
}

impl Deref for RegistryLock {
    type Target = Registry;

    fn deref(&self) -> &Self::Target {
        self.0.get().expect("Registry not init")
    }
}

pub static REGISTRY: RegistryLock = RegistryLock(OnceLock::new());

/// Trait for types stored in a registry, allowing self-lookup of their numeric ID.
pub trait RegistryEntry: 'static {
    fn key(&self) -> &Identifier;
    fn try_id(&self) -> Option<usize>;

    /// # Panics
    /// Panics if the entry is not registered.
    fn id(&self) -> usize {
        self.try_id().expect("entry not found in registry")
    }
}

/// Generic trait for registries with a typed entry.
///
/// `Entry` is the concrete type (e.g. `Block`); all lookups return `&'static Entry`
/// to enforce cheap pointer copies and prevent expensive clones.
pub trait RegistryExt {
    type Entry: RegistryEntry;

    fn freeze(&mut self);
    fn by_id(&self, id: usize) -> Option<&'static Self::Entry>;
    fn by_key(&self, key: &Identifier) -> Option<&'static Self::Entry>;
    fn id_from_key(&self, key: &Identifier) -> Option<usize>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;
}

/// Trait for registries that support tagging entries.
pub trait TaggedRegistryExt: RegistryExt {
    fn register_tag(&mut self, tag: Identifier, keys: &[&'static str]);
    fn modify_tag(&mut self, tag: &Identifier, f: impl FnOnce(Vec<Identifier>) -> Vec<Identifier>);
    fn is_in_tag(&self, entry: &'static Self::Entry, tag: &Identifier) -> bool;
    fn get_tag(&self, tag: &Identifier) -> Option<Vec<&'static Self::Entry>>;
    fn iter_tag(&self, tag: &Identifier) -> impl Iterator<Item = &'static Self::Entry> + '_;
    fn tag_keys(&self) -> impl Iterator<Item = &Identifier> + '_;
}

/// Implements `RegistryExt` for a registry type.
///
/// Expects `$id_field` to be `Vec<&'static $Entry>`.
#[macro_export]
macro_rules! impl_registry_ext {
    ($Registry:ty, $Entry:ty, $id_field:ident, $key_field:ident) => {
        impl $crate::RegistryExt for $Registry {
            type Entry = $Entry;

            fn freeze(&mut self) {
                self.allows_registering = false;
            }

            fn by_id(&self, id: usize) -> Option<&'static $Entry> {
                self.$id_field.get(id).copied()
            }

            fn by_key(&self, key: &steel_utils::Identifier) -> Option<&'static $Entry> {
                self.$key_field
                    .get(key)
                    .and_then(|&id| self.$id_field.get(id).copied())
            }

            fn id_from_key(&self, key: &steel_utils::Identifier) -> Option<usize> {
                self.$key_field.get(key).copied()
            }

            fn len(&self) -> usize {
                self.$id_field.len()
            }

            fn is_empty(&self) -> bool {
                self.$id_field.is_empty()
            }
        }
    };
}

/// Implements `RegistryEntry` for an entry type via hash map lookup.
#[macro_export]
macro_rules! impl_registry_entry {
    ($Entry:ty, $global_field:ident) => {
        impl $crate::RegistryEntry for $Entry {
            fn key(&self) -> &steel_utils::Identifier {
                &self.key
            }

            fn try_id(&self) -> Option<usize> {
                use $crate::RegistryExt;
                $crate::REGISTRY.$global_field.id_from_key(&self.key)
            }
        }
    };
}

/// Implements both `RegistryExt` and `RegistryEntry` for a standard registry.
#[macro_export]
macro_rules! impl_registry {
    ($Registry:ty, $Entry:ty, $id_field:ident, $key_field:ident, $global_field:ident) => {
        $crate::impl_registry_ext!($Registry, $Entry, $id_field, $key_field);
        $crate::impl_registry_entry!($Entry, $global_field);
    };
}

/// Implements `TaggedRegistryExt` for a registry with tag support.
#[macro_export]
macro_rules! impl_tagged_registry {
    ($Registry:ty, $key_field:ident, $entity_name:literal) => {
        impl $crate::TaggedRegistryExt for $Registry {
            fn register_tag(&mut self, tag: steel_utils::Identifier, keys: &[&'static str]) {
                assert!(
                    self.allows_registering,
                    "Cannot register tags after registry has been frozen"
                );

                let identifiers: Vec<steel_utils::Identifier> = keys
                    .iter()
                    .filter_map(|key| {
                        let ident = steel_utils::registry::registry_vanilla_or_custom_tag(key);
                        if self.$key_field.contains_key(&ident) {
                            Some(ident)
                        } else {
                            None
                        }
                    })
                    .collect();

                self.tags.insert(tag, identifiers);
            }

            fn modify_tag(
                &mut self,
                tag: &steel_utils::Identifier,
                f: impl FnOnce(Vec<steel_utils::Identifier>) -> Vec<steel_utils::Identifier>,
            ) {
                let existing = self.tags.remove(tag).unwrap_or_default();
                let entries = f(existing)
                    .into_iter()
                    .filter(|key| {
                        let exists = self.$key_field.contains_key(key);
                        if !exists {
                            tracing::error!(
                                "{} {} not found in registry, skipping from tag {}",
                                $entity_name,
                                key,
                                tag,
                            );
                        }
                        exists
                    })
                    .collect();
                self.tags.insert(tag.clone(), entries);
            }

            fn is_in_tag(
                &self,
                entry: &'static Self::Entry,
                tag: &steel_utils::Identifier,
            ) -> bool {
                self.tags
                    .get(tag)
                    .is_some_and(|entries| entries.contains(&entry.key))
            }

            fn get_tag(&self, tag: &steel_utils::Identifier) -> Option<Vec<&'static Self::Entry>> {
                use $crate::RegistryExt;
                self.tags.get(tag).map(|idents| {
                    idents
                        .iter()
                        .filter_map(|ident| self.by_key(ident))
                        .collect()
                })
            }

            fn iter_tag(
                &self,
                tag: &steel_utils::Identifier,
            ) -> impl Iterator<Item = &'static Self::Entry> + '_ {
                use $crate::RegistryExt;
                self.tags
                    .get(tag)
                    .into_iter()
                    .flat_map(|v| v.iter().filter_map(|ident| self.by_key(ident)))
            }

            fn tag_keys(&self) -> impl Iterator<Item = &steel_utils::Identifier> + '_ {
                self.tags.keys()
            }
        }
    };
}

pub const BLOCKS_REGISTRY: Identifier = Identifier::vanilla_static("block");
pub const ITEMS_REGISTRY: Identifier = Identifier::vanilla_static("item");
pub const BIOMES_REGISTRY: Identifier = Identifier::vanilla_static("worldgen/biome");
pub const CHAT_TYPE_REGISTRY: Identifier = Identifier::vanilla_static("chat_type");
pub const TRIM_PATTERN_REGISTRY: Identifier = Identifier::vanilla_static("trim_pattern");
pub const TRIM_MATERIAL_REGISTRY: Identifier = Identifier::vanilla_static("trim_material");
pub const WOLF_VARIANT_REGISTRY: Identifier = Identifier::vanilla_static("wolf_variant");
pub const WOLF_SOUND_VARIANT_REGISTRY: Identifier =
    Identifier::vanilla_static("wolf_sound_variant");
pub const PIG_VARIANT_REGISTRY: Identifier = Identifier::vanilla_static("pig_variant");
pub const PIG_SOUND_VARIANT_REGISTRY: Identifier = Identifier::vanilla_static("pig_sound_variant");
pub const CHICKEN_SOUND_VARIANT_REGISTRY: Identifier =
    Identifier::vanilla_static("chicken_sound_variant");
pub const CAT_SOUND_VARIANT_REGISTRY: Identifier = Identifier::vanilla_static("cat_sound_variant");
pub const COW_SOUND_VARIANT_REGISTRY: Identifier = Identifier::vanilla_static("cow_sound_variant");
pub const FROG_VARIANT_REGISTRY: Identifier = Identifier::vanilla_static("frog_variant");
pub const CAT_VARIANT_REGISTRY: Identifier = Identifier::vanilla_static("cat_variant");
pub const COW_VARIANT_REGISTRY: Identifier = Identifier::vanilla_static("cow_variant");
pub const CHICKEN_VARIANT_REGISTRY: Identifier = Identifier::vanilla_static("chicken_variant");
pub const PAINTING_VARIANT_REGISTRY: Identifier = Identifier::vanilla_static("painting_variant");
pub const DIMENSION_TYPE_REGISTRY: Identifier = Identifier::vanilla_static("dimension_type");
pub const DAMAGE_TYPE_REGISTRY: Identifier = Identifier::vanilla_static("damage_type");
pub const BANNER_PATTERN_REGISTRY: Identifier = Identifier::vanilla_static("banner_pattern");
//TODO: Add enchantments
//pub const ENCHANTMENT_REGISTRY: Identifier = Identifier::vanilla_static("enchantment");
pub const JUKEBOX_SONG_REGISTRY: Identifier = Identifier::vanilla_static("jukebox_song");
pub const INSTRUMENT_REGISTRY: Identifier = Identifier::vanilla_static("instrument");
pub const DIALOG_REGISTRY: Identifier = Identifier::vanilla_static("dialog");
pub const MENU_TYPE_REGISTRY: Identifier = Identifier::vanilla_static("menu");
pub const ZOMBIE_NAUTILUS_VARIANT_REGISTRY: Identifier =
    Identifier::vanilla_static("zombie_nautilus_variant");
pub const TIMELINE_REGISTRY: Identifier = Identifier::vanilla_static("timeline");
pub const LOOT_TABLE_REGISTRY: Identifier = Identifier::vanilla_static("loot_table");
pub const BLOCK_ENTITY_TYPE_REGISTRY: Identifier = Identifier::vanilla_static("block_entity_type");
pub const FLUID_REGISTRY: Identifier = Identifier::vanilla_static("fluid");
pub const ENTITY_TYPE_REGISTRY: Identifier = Identifier::vanilla_static("entity_type");
pub const POI_TYPE_REGISTRY: Identifier = Identifier::vanilla_static("point_of_interest_type");
pub const WORLD_CLOCK_REGISTRY: Identifier = Identifier::vanilla_static("world_clock");

pub struct Registry {
    pub blocks: BlockRegistry,
    pub items: ItemRegistry,
    pub data_components: DataComponentRegistry,
    pub entity_data_serializers: EntityDataSerializerRegistry,
    pub biomes: BiomeRegistry,
    pub chat_types: ChatTypeRegistry,
    pub trim_patterns: TrimPatternRegistry,
    pub trim_materials: TrimMaterialRegistry,
    pub wolf_variants: WolfVariantRegistry,
    pub wolf_sound_variants: WolfSoundVariantRegistry,
    pub pig_sound_variants: PigSoundVariantRegistry,
    pub chicken_sound_variants: ChickenSoundVariantRegistry,
    pub cat_sound_variants: CatSoundVariantRegistry,
    pub cow_sound_variants: CowSoundVariantRegistry,
    pub pig_variants: PigVariantRegistry,
    pub frog_variants: FrogVariantRegistry,
    pub cat_variants: CatVariantRegistry,
    pub cow_variants: CowVariantRegistry,
    pub chicken_variants: ChickenVariantRegistry,
    pub painting_variants: PaintingVariantRegistry,
    pub dimension_types: DimensionTypeRegistry,
    pub damage_types: DamageTypeRegistry,
    pub banner_patterns: BannerPatternRegistry,
    pub jukebox_songs: JukeboxSongRegistry,
    pub instruments: InstrumentRegistry,
    pub dialogs: DialogRegistry,
    pub menu_types: MenuTypeRegistry,
    pub zombie_nautilus_variants: ZombieNautilusVariantRegistry,
    pub timelines: TimelineRegistry,
    pub recipes: RecipeRegistry,
    pub entity_types: EntityTypeRegistry,
    pub loot_tables: LootTableRegistry,
    pub block_entity_types: BlockEntityTypeRegistry,
    pub game_rules: GameRuleRegistry,
    pub fluids: FluidRegistry,
    pub poi_types: PoiTypeRegistry,
    pub world_clocks: WorldClockRegistry,
}

impl Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Registry {")
            .and_then(|_| f.write_fmt(format_args!("Blocks Loaded: {}", self.blocks.len())))
            .and_then(|_| f.write_str("}"))
    }
}

impl Registry {
    #[must_use]
    pub fn new_vanilla() -> Self {
        let mut registry = Self::new_empty();

        vanilla_blocks::register_blocks(&mut registry.blocks);
        vanilla_block_tags::register_block_tags(&mut registry.blocks);

        vanilla_components::register_vanilla_data_components(&mut registry.data_components);

        register_vanilla_entity_data_serializers(&mut registry.entity_data_serializers);

        vanilla_items::register_items(&mut registry.items);
        vanilla_item_tags::register_item_tags(&mut registry.items);

        vanilla_biomes::register_biomes(&mut registry.biomes);
        vanilla_chat_types::register_chat_types(&mut registry.chat_types);
        vanilla_trim_patterns::register_trim_patterns(&mut registry.trim_patterns);
        vanilla_trim_materials::register_trim_materials(&mut registry.trim_materials);
        vanilla_wolf_variants::register_wolf_variants(&mut registry.wolf_variants);
        vanilla_wolf_sound_variants::register_wolf_sound_variants(
            &mut registry.wolf_sound_variants,
        );
        vanilla_pig_variants::register_pig_variants(&mut registry.pig_variants);
        vanilla_pig_sound_variants::register_pig_sound_variants(&mut registry.pig_sound_variants);
        vanilla_chicken_sound_variants::register_chicken_sound_variants(
            &mut registry.chicken_sound_variants,
        );
        vanilla_cat_sound_variants::register_cat_sound_variants(&mut registry.cat_sound_variants);
        vanilla_cow_sound_variants::register_cow_sound_variants(&mut registry.cow_sound_variants);
        vanilla_frog_variants::register_frog_variants(&mut registry.frog_variants);
        vanilla_cat_variants::register_cat_variants(&mut registry.cat_variants);
        vanilla_cow_variants::register_cow_variants(&mut registry.cow_variants);
        vanilla_chicken_variants::register_chicken_variants(&mut registry.chicken_variants);
        vanilla_painting_variants::register_painting_variants(&mut registry.painting_variants);
        vanilla_painting_variant_tags::register_painting_variant_tags(
            &mut registry.painting_variants,
        );
        vanilla_dimension_types::register_dimension_types(&mut registry.dimension_types);
        vanilla_damage_types::register_damage_types(&mut registry.damage_types);
        vanilla_damage_type_tags::register_damage_type_tags(&mut registry.damage_types);
        vanilla_banner_patterns::register_banner_patterns(&mut registry.banner_patterns);
        vanilla_banner_pattern_tags::register_banner_pattern_tags(&mut registry.banner_patterns);
        vanilla_jukebox_songs::register_jukebox_songs(&mut registry.jukebox_songs);
        vanilla_instruments::register_instruments(&mut registry.instruments);
        vanilla_instrument_tags::register_instrument_tags(&mut registry.instruments);
        vanilla_dialogs::register_dialogs(&mut registry.dialogs);
        vanilla_dialog_tags::register_dialog_tags(&mut registry.dialogs);
        vanilla_menu_types::register_menu_types(&mut registry.menu_types);
        vanilla_zombie_nautilus_variants::register_zombie_nautilus_variants(
            &mut registry.zombie_nautilus_variants,
        );
        vanilla_timelines::register_timelines(&mut registry.timelines);
        vanilla_timeline_tags::register_timeline_tags(&mut registry.timelines);
        vanilla_recipes::register_recipes(&mut registry.recipes);
        vanilla_entities::register_entity_types(&mut registry.entity_types);
        vanilla_entity_type_tags::register_entity_type_tags(&mut registry.entity_types);
        vanilla_loot_tables::register_loot_tables(&mut registry.loot_tables);
        vanilla_block_entity_types::register_block_entity_types(&mut registry.block_entity_types);
        vanilla_game_rules::register_game_rules(&mut registry.game_rules);

        vanilla_fluids::register_fluids(&mut registry.fluids);
        vanilla_fluid_tags::register_fluid_tags(&mut registry.fluids);

        vanilla_poi_types::register_poi_types(&mut registry.poi_types);
        vanilla_poi_type_tags::register_poi_type_tags(&mut registry.poi_types);

        vanilla_world_clocks::register_world_clocks(&mut registry.world_clocks);
        registry
    }

    pub fn freeze(&mut self) {
        self.blocks.freeze();
        self.data_components.freeze();
        self.entity_data_serializers.freeze();
        self.items.freeze();
        self.biomes.freeze();
        self.chat_types.freeze();
        self.trim_patterns.freeze();
        self.trim_materials.freeze();
        self.wolf_variants.freeze();
        self.wolf_sound_variants.freeze();
        self.pig_variants.freeze();
        self.pig_sound_variants.freeze();
        self.chicken_sound_variants.freeze();
        self.cat_sound_variants.freeze();
        self.cow_sound_variants.freeze();
        self.frog_variants.freeze();
        self.cat_variants.freeze();
        self.cow_variants.freeze();
        self.chicken_variants.freeze();
        self.painting_variants.freeze();
        self.dimension_types.freeze();
        self.damage_types.freeze();
        self.banner_patterns.freeze();
        self.jukebox_songs.freeze();
        self.instruments.freeze();
        self.dialogs.freeze();
        self.menu_types.freeze();
        self.zombie_nautilus_variants.freeze();
        self.timelines.freeze();
        self.recipes.freeze();
        self.entity_types.freeze();
        self.loot_tables.freeze();
        self.block_entity_types.freeze();
        self.game_rules.freeze();
        self.fluids.freeze();
        self.poi_types.freeze();
        self.world_clocks.freeze();
    }

    #[must_use]
    pub fn new_empty() -> Self {
        Self {
            blocks: BlockRegistry::new(),
            data_components: DataComponentRegistry::new(),
            entity_data_serializers: EntityDataSerializerRegistry::new(),
            items: ItemRegistry::new(),
            biomes: BiomeRegistry::new(),
            chat_types: ChatTypeRegistry::new(),
            trim_patterns: TrimPatternRegistry::new(),
            trim_materials: TrimMaterialRegistry::new(),
            wolf_variants: WolfVariantRegistry::new(),
            wolf_sound_variants: WolfSoundVariantRegistry::new(),
            pig_variants: PigVariantRegistry::new(),
            pig_sound_variants: PigSoundVariantRegistry::new(),
            chicken_sound_variants: ChickenSoundVariantRegistry::new(),
            cat_sound_variants: CatSoundVariantRegistry::new(),
            cow_sound_variants: CowSoundVariantRegistry::new(),
            frog_variants: FrogVariantRegistry::new(),
            cat_variants: CatVariantRegistry::new(),
            cow_variants: CowVariantRegistry::new(),
            chicken_variants: ChickenVariantRegistry::new(),
            painting_variants: PaintingVariantRegistry::new(),
            dimension_types: DimensionTypeRegistry::new(),
            damage_types: DamageTypeRegistry::new(),
            banner_patterns: BannerPatternRegistry::new(),
            jukebox_songs: JukeboxSongRegistry::new(),
            instruments: InstrumentRegistry::new(),
            dialogs: DialogRegistry::new(),
            menu_types: MenuTypeRegistry::new(),
            zombie_nautilus_variants: ZombieNautilusVariantRegistry::new(),
            timelines: TimelineRegistry::new(),
            recipes: RecipeRegistry::new(),
            entity_types: EntityTypeRegistry::new(),
            loot_tables: LootTableRegistry::new(),
            block_entity_types: BlockEntityTypeRegistry::new(),
            game_rules: GameRuleRegistry::new(),
            fluids: FluidRegistry::new(),
            world_clocks: WorldClockRegistry::new(),
            poi_types: PoiTypeRegistry::new(),
        }
    }
}
