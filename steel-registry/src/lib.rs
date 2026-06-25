#![feature(const_trait_impl, const_cmp, derive_const)]

use crate::game_events::GameEventRegistry;
use crate::world_clock::WorldClockRegistry;
use crate::{
    attribute::AttributeRegistry,
    banner_pattern::BannerPatternRegistry,
    biome::BiomeRegistry,
    block_entity_type::BlockEntityTypeRegistry,
    blocks::BlockRegistry,
    carver::ConfiguredCarverRegistry,
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
    enchantment::EnchantmentRegistry,
    entity_data::{EntityDataSerializerRegistry, register_vanilla_entity_data_serializers},
    entity_type::EntityTypeRegistry,
    feature::{
        ConfiguredFeatureKind, ConfiguredFeatureRef, ConfiguredFeatureRegistry, PlacedFeatureData,
        PlacedFeatureRef, PlacedFeatureRegistry,
    },
    fluid::FluidRegistry,
    frog_variant::FrogVariantRegistry,
    game_rules::GameRuleRegistry,
    instrument::InstrumentRegistry,
    items::ItemRegistry,
    jukebox_song::JukeboxSongRegistry,
    loot_table::LootTableRegistry,
    menu_type::MenuTypeRegistry,
    mob_effect::MobEffectRegistry,
    painting_variant::PaintingVariantRegistry,
    particle_type::ParticleTypeRegistry,
    pig_sound_variant::PigSoundVariantRegistry,
    pig_variant::PigVariantRegistry,
    poi::PoiTypeRegistry,
    recipe::RecipeRegistry,
    sound_event::SoundEventRegistry,
    structure::StructureRegistry,
    structure_processor::StructureProcessorListRegistry,
    timeline::TimelineRegistry,
    trim_material::TrimMaterialRegistry,
    trim_pattern::TrimPatternRegistry,
    villager_profession::VillagerProfessionRegistry,
    villager_type::VillagerTypeRegistry,
    wolf_sound_variant::WolfSoundVariantRegistry,
    wolf_variant::WolfVariantRegistry,
    zombie_nautilus_variant::ZombieNautilusVariantRegistry,
};
use std::{fmt::Debug, ops::Deref, sync::OnceLock};
use steel_utils::Identifier;
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
pub mod cow_sound_variant;
pub mod cow_variant;
pub mod damage_type;
pub mod data_components;
pub mod dialog;
pub mod dimension_type;
pub mod enchantment;
pub mod enchantment_effect;
pub mod entity_data;
pub mod entity_type;
pub mod equipment;
pub mod feature;
pub mod fluid;
pub mod frog_variant;
pub mod game_events;
pub mod game_rules;
pub mod instrument;
pub mod item_stack;
pub mod items;
pub mod jukebox_song;
pub mod loot_table;
mod macros;
pub mod menu_type;
pub mod mob_effect;
pub mod painting_variant;
pub mod particle_type;
pub mod pig_sound_variant;
pub mod pig_variant;
pub mod poi;
pub mod recipe;
pub mod sound_event;
pub mod structure;
pub mod structure_processor;
pub mod structure_set;
pub mod template_pool;
pub mod timeline;
pub mod trim_material;
pub mod trim_pattern;
pub mod villager_profession;
pub mod villager_type;
pub mod wolf_sound_variant;
pub mod wolf_variant;
pub mod world_clock;
pub mod zombie_nautilus_variant;

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

pub struct RegistryLock(OnceLock<Registry>);

impl RegistryLock {
    #[expect(clippy::result_large_err)]
    pub fn init(&self, value: Registry) -> Result<(), Registry> {
        self.0.set(value)
    }
}

impl Deref for RegistryLock {
    type Target = Registry;

    fn deref(&self) -> &Self::Target {
        self.0.get().expect("Registry not init")
    }
}

pub static REGISTRY: RegistryLock = RegistryLock(OnceLock::new());

#[cfg(any(test, feature = "test-utils"))]
pub mod test_support {
    use std::sync::Once;

    use crate::{REGISTRY, Registry};

    static INIT_REGISTRY: Once = Once::new();

    /// Initializes the global registry with frozen vanilla data for tests.
    pub fn init_test_registry() {
        INIT_REGISTRY.call_once(|| {
            let mut registry = Registry::new_vanilla();
            registry.freeze();
            let _ = REGISTRY.init(registry);
        });
    }
}

/// Trait for types stored in a registry, allowing self-lookup of their numeric ID.
pub trait RegistryEntry: PartialEq + 'static {
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
pub const PARTICLE_TYPE_REGISTRY: Identifier = Identifier::vanilla_static("particle_type");
pub const VILLAGER_TYPE_REGISTRY: Identifier = Identifier::vanilla_static("villager_type");
pub const VILLAGER_PROFESSION_REGISTRY: Identifier =
    Identifier::vanilla_static("villager_profession");
pub const DIMENSION_TYPE_REGISTRY: Identifier = Identifier::vanilla_static("dimension_type");
pub const DAMAGE_TYPE_REGISTRY: Identifier = Identifier::vanilla_static("damage_type");
pub const BANNER_PATTERN_REGISTRY: Identifier = Identifier::vanilla_static("banner_pattern");
pub const ENCHANTMENT_REGISTRY: Identifier = Identifier::vanilla_static("enchantment");
pub const JUKEBOX_SONG_REGISTRY: Identifier = Identifier::vanilla_static("jukebox_song");
pub const INSTRUMENT_REGISTRY: Identifier = Identifier::vanilla_static("instrument");
pub const DIALOG_REGISTRY: Identifier = Identifier::vanilla_static("dialog");
pub const MENU_TYPE_REGISTRY: Identifier = Identifier::vanilla_static("menu");
pub const MOB_EFFECT_REGISTRY: Identifier = Identifier::vanilla_static("mob_effect");
pub const ZOMBIE_NAUTILUS_VARIANT_REGISTRY: Identifier =
    Identifier::vanilla_static("zombie_nautilus_variant");
pub const TIMELINE_REGISTRY: Identifier = Identifier::vanilla_static("timeline");
pub const LOOT_TABLE_REGISTRY: Identifier = Identifier::vanilla_static("loot_table");
pub const BLOCK_ENTITY_TYPE_REGISTRY: Identifier = Identifier::vanilla_static("block_entity_type");
pub const FLUID_REGISTRY: Identifier = Identifier::vanilla_static("fluid");
pub const ENTITY_TYPE_REGISTRY: Identifier = Identifier::vanilla_static("entity_type");
pub const SOUND_EVENT_REGISTRY: Identifier = Identifier::vanilla_static("sound_event");
pub const POI_TYPE_REGISTRY: Identifier = Identifier::vanilla_static("point_of_interest_type");
pub const WORLD_CLOCK_REGISTRY: Identifier = Identifier::vanilla_static("world_clock");
pub const CONFIGURED_CARVER_REGISTRY: Identifier =
    Identifier::vanilla_static("worldgen/configured_carver");
pub const CONFIGURED_FEATURE_REGISTRY: Identifier =
    Identifier::vanilla_static("worldgen/configured_feature");
pub const PLACED_FEATURE_REGISTRY: Identifier =
    Identifier::vanilla_static("worldgen/placed_feature");
pub const STRUCTURE_REGISTRY: Identifier = Identifier::vanilla_static("worldgen/structure");
pub const STRUCTURE_PROCESSOR_LIST_REGISTRY: Identifier =
    Identifier::vanilla_static("worldgen/processor_list");

pub struct Registry {
    pub attributes: AttributeRegistry,
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
    pub particle_types: ParticleTypeRegistry,
    pub villager_types: VillagerTypeRegistry,
    pub villager_professions: VillagerProfessionRegistry,
    pub dimension_types: DimensionTypeRegistry,
    pub damage_types: DamageTypeRegistry,
    pub banner_patterns: BannerPatternRegistry,
    pub jukebox_songs: JukeboxSongRegistry,
    pub instruments: InstrumentRegistry,
    pub dialogs: DialogRegistry,
    pub menu_types: MenuTypeRegistry,
    pub mob_effects: MobEffectRegistry,
    pub zombie_nautilus_variants: ZombieNautilusVariantRegistry,
    pub timelines: TimelineRegistry,
    pub recipes: RecipeRegistry,
    pub entity_types: EntityTypeRegistry,
    pub loot_tables: LootTableRegistry,
    pub block_entity_types: BlockEntityTypeRegistry,
    pub game_rules: GameRuleRegistry,
    pub game_events: GameEventRegistry,
    pub sound_events: SoundEventRegistry,
    pub fluids: FluidRegistry,
    pub poi_types: PoiTypeRegistry,
    pub enchantments: EnchantmentRegistry,
    pub world_clocks: WorldClockRegistry,
    pub configured_carvers: ConfiguredCarverRegistry,
    pub configured_features: ConfiguredFeatureRegistry,
    pub placed_features: PlacedFeatureRegistry,
    pub structures: StructureRegistry,
    pub structure_processors: StructureProcessorListRegistry,
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

        vanilla_attributes::register_attributes(&mut registry.attributes);

        vanilla_blocks::register_blocks(&mut registry.blocks);
        vanilla_block_tags::BlockTag::register_block_tags(&mut registry.blocks);

        vanilla_components::register_vanilla_data_components(&mut registry.data_components);

        register_vanilla_entity_data_serializers(&mut registry.entity_data_serializers);

        vanilla_items::register_items(&mut registry.items);
        vanilla_item_tags::ItemTag::register_item_tags(&mut registry.items);

        vanilla_biomes::register_biomes(&mut registry.biomes);
        vanilla_biome_tags::BiomeTag::register_biome_tags(&mut registry.biomes);
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
        vanilla_particle_types::register_particle_types(&mut registry.particle_types);
        vanilla_villager_types::register_villager_types(&mut registry.villager_types);
        vanilla_villager_professions::register_villager_professions(
            &mut registry.villager_professions,
        );
        vanilla_painting_variant_tags::PaintingVariantTag::register_painting_variant_tags(
            &mut registry.painting_variants,
        );
        vanilla_dimension_types::register_dimension_types(&mut registry.dimension_types);
        vanilla_damage_types::register_damage_types(&mut registry.damage_types);
        vanilla_damage_type_tags::DamageTypeTag::register_damage_type_tags(
            &mut registry.damage_types,
        );
        vanilla_banner_patterns::register_banner_patterns(&mut registry.banner_patterns);
        vanilla_banner_pattern_tags::BannerPatternTag::register_banner_pattern_tags(
            &mut registry.banner_patterns,
        );
        vanilla_jukebox_songs::register_jukebox_songs(&mut registry.jukebox_songs);
        vanilla_instruments::register_instruments(&mut registry.instruments);
        vanilla_instrument_tags::InstrumentTag::register_instrument_tags(&mut registry.instruments);
        vanilla_dialogs::register_dialogs(&mut registry.dialogs);
        vanilla_dialog_tags::DialogTag::register_dialog_tags(&mut registry.dialogs);
        vanilla_menu_types::register_menu_types(&mut registry.menu_types);
        vanilla_mob_effects::register_mob_effects(&mut registry.mob_effects);
        vanilla_zombie_nautilus_variants::register_zombie_nautilus_variants(
            &mut registry.zombie_nautilus_variants,
        );
        vanilla_timelines::register_timelines(&mut registry.timelines);
        vanilla_timeline_tags::TimelineTag::register_timeline_tags(&mut registry.timelines);
        vanilla_recipes::register_recipes(&mut registry.recipes);
        vanilla_entities::register_entity_types(&mut registry.entity_types);
        vanilla_entity_type_tags::EntityTypeTag::register_entity_type_tags(
            &mut registry.entity_types,
        );
        vanilla_loot_tables::register_loot_tables(&mut registry.loot_tables);
        vanilla_block_entity_types::register_block_entity_types(&mut registry.block_entity_types);
        vanilla_game_rules::register_game_rules(&mut registry.game_rules);
        vanilla_game_events::register_game_events(&mut registry.game_events);
        sound_events::register_sound_events(&mut registry.sound_events);

        vanilla_fluids::register_fluids(&mut registry.fluids);
        vanilla_fluid_tags::FluidTag::register_fluid_tags(&mut registry.fluids);

        vanilla_poi_types::register_poi_types(&mut registry.poi_types);
        vanilla_poi_type_tags::PoiTag::register_poi_tags(&mut registry.poi_types);
        registry.poi_types.build_state_index(&registry.blocks);

        vanilla_enchantments::register_enchantments(&mut registry.enchantments);
        vanilla_enchantment_tags::EnchantmentTag::register_enchantment_tags(
            &mut registry.enchantments,
        );

        vanilla_world_clocks::register_world_clocks(&mut registry.world_clocks);
        vanilla_structures::register_structures(&mut registry.structures);
        vanilla_structure_tags::StructureTag::register_structure_tags(&mut registry.structures);
        vanilla_structure_processors::register_structure_processor_lists(
            &mut registry.structure_processors,
        );

        vanilla_configured_carvers::register_configured_carvers(&mut registry.configured_carvers);
        vanilla_configured_features::register_configured_features(
            &mut registry.configured_features,
        );
        vanilla_placed_features::register_placed_features(&mut registry.placed_features);

        registry
    }

    pub fn freeze(&mut self) {
        self.validate_references();

        self.attributes.freeze();
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
        self.particle_types.freeze();
        self.villager_types.freeze();
        self.villager_professions.freeze();
        self.dimension_types.freeze();
        self.damage_types.freeze();
        self.banner_patterns.freeze();
        self.jukebox_songs.freeze();
        self.instruments.freeze();
        self.dialogs.freeze();
        self.menu_types.freeze();
        self.mob_effects.freeze();
        self.zombie_nautilus_variants.freeze();
        self.timelines.freeze();
        self.recipes.freeze();
        self.entity_types.freeze();
        self.loot_tables.freeze();
        self.block_entity_types.freeze();
        self.game_rules.freeze();
        self.game_events.freeze();
        self.sound_events.freeze();
        self.fluids.freeze();
        self.poi_types.freeze();
        self.enchantments.freeze();
        self.world_clocks.freeze();
        self.configured_carvers.freeze();
        self.configured_features.freeze();
        self.placed_features.freeze();
        self.structures.freeze();
        self.structure_processors.freeze();
    }

    fn validate_references(&self) {
        for (_, biome) in self.biomes.iter() {
            for carver_key in &biome.carvers {
                assert!(
                    self.configured_carvers.by_key(carver_key).is_some(),
                    "biome {} references unknown configured carver {}",
                    biome.key,
                    carver_key
                );
            }

            for feature_stage in &biome.features {
                for placed_feature_key in feature_stage {
                    assert!(
                        self.placed_features.by_key(placed_feature_key).is_some(),
                        "biome {} references unknown placed feature {}",
                        biome.key,
                        placed_feature_key
                    );
                }
            }
        }

        for (_, placed_feature) in self.placed_features.iter() {
            self.validate_placed_feature_data(&placed_feature.data);
        }

        for (_, configured_feature) in self.configured_features.iter() {
            self.validate_configured_feature_kind(&configured_feature.kind);
        }

        if !self.placed_features.is_empty() {
            for pool in vanilla_template_pools::vanilla_template_pools() {
                for (element, _) in &pool.elements {
                    self.validate_template_pool_feature_refs(element);
                }
            }
        }
    }

    fn validate_placed_feature_ref(&self, feature: &PlacedFeatureRef) {
        match feature {
            PlacedFeatureRef::Reference(feature) => {
                let key = &feature.key;
                assert!(
                    self.placed_features.by_key(key).is_some(),
                    "unknown placed feature reference {key}"
                );
            }
            PlacedFeatureRef::Inline(data) => self.validate_placed_feature_data(data),
        }
    }

    fn validate_placed_feature_data(&self, feature: &PlacedFeatureData) {
        self.validate_configured_feature_ref(&feature.feature);
    }

    fn validate_configured_feature_ref(&self, feature: &ConfiguredFeatureRef) {
        match feature {
            ConfiguredFeatureRef::Reference(feature) => {
                let key = &feature.key;
                assert!(
                    self.configured_features.by_key(key).is_some(),
                    "unknown configured feature reference {key}"
                );
            }
            ConfiguredFeatureRef::Inline(kind) => self.validate_configured_feature_kind(kind),
        }
    }

    fn validate_configured_feature_kind(&self, kind: &ConfiguredFeatureKind) {
        match kind {
            ConfiguredFeatureKind::RandomBooleanSelector(config) => {
                self.validate_placed_feature_ref(&config.feature_true);
                self.validate_placed_feature_ref(&config.feature_false);
            }
            ConfiguredFeatureKind::RandomSelector(config) => {
                for feature in &config.features {
                    self.validate_placed_feature_ref(&feature.feature);
                }
                self.validate_placed_feature_ref(&config.default);
            }
            ConfiguredFeatureKind::WeightedRandomSelector(config) => {
                for feature in &config.features {
                    self.validate_placed_feature_ref(&feature.data);
                }
            }
            ConfiguredFeatureKind::RootSystem(config) => {
                self.validate_placed_feature_ref(&config.feature);
            }
            ConfiguredFeatureKind::Fossil(config) => {
                assert!(
                    self.structure_processors
                        .by_key(&config.fossil_processors)
                        .is_some(),
                    "fossil configured feature references unknown processor list {}",
                    config.fossil_processors
                );
                assert!(
                    self.structure_processors
                        .by_key(&config.overlay_processors)
                        .is_some(),
                    "fossil configured feature references unknown processor list {}",
                    config.overlay_processors
                );
            }
            ConfiguredFeatureKind::SimpleRandomSelector(config) => {
                for feature in &config.features {
                    self.validate_placed_feature_ref(feature);
                }
            }
            ConfiguredFeatureKind::Sequence(config) => {
                for feature in &config.features {
                    self.validate_placed_feature_ref(feature);
                }
            }
            ConfiguredFeatureKind::VegetationPatch(config)
            | ConfiguredFeatureKind::WaterloggedVegetationPatch(config) => {
                self.validate_placed_feature_ref(&config.vegetation_feature);
            }
            _ => {}
        }
    }

    fn validate_template_pool_feature_refs(&self, element: &template_pool::PoolElement) {
        match element {
            template_pool::PoolElement::Feature { feature, .. } => {
                assert!(
                    self.placed_features.by_key(feature).is_some(),
                    "template pool references unknown placed feature {feature}"
                );
            }
            template_pool::PoolElement::List { elements, .. } => {
                for element in elements {
                    self.validate_template_pool_feature_refs(element);
                }
            }
            template_pool::PoolElement::Single { .. }
            | template_pool::PoolElement::LegacySingle { .. }
            | template_pool::PoolElement::Empty => {}
        }
    }

    #[must_use]
    pub fn new_empty() -> Self {
        Self {
            attributes: AttributeRegistry::new(),
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
            particle_types: ParticleTypeRegistry::new(),
            villager_types: VillagerTypeRegistry::new(),
            villager_professions: VillagerProfessionRegistry::new(),
            dimension_types: DimensionTypeRegistry::new(),
            damage_types: DamageTypeRegistry::new(),
            banner_patterns: BannerPatternRegistry::new(),
            jukebox_songs: JukeboxSongRegistry::new(),
            instruments: InstrumentRegistry::new(),
            dialogs: DialogRegistry::new(),
            menu_types: MenuTypeRegistry::new(),
            mob_effects: MobEffectRegistry::new(),
            zombie_nautilus_variants: ZombieNautilusVariantRegistry::new(),
            timelines: TimelineRegistry::new(),
            recipes: RecipeRegistry::new(),
            entity_types: EntityTypeRegistry::new(),
            loot_tables: LootTableRegistry::new(),
            block_entity_types: BlockEntityTypeRegistry::new(),
            game_rules: GameRuleRegistry::new(),
            game_events: GameEventRegistry::new(),
            sound_events: SoundEventRegistry::new(),
            fluids: FluidRegistry::new(),
            world_clocks: WorldClockRegistry::new(),
            poi_types: PoiTypeRegistry::new(),
            enchantments: EnchantmentRegistry::new(),
            configured_carvers: ConfiguredCarverRegistry::new(),
            configured_features: ConfiguredFeatureRegistry::new(),
            placed_features: PlacedFeatureRegistry::new(),
            structures: StructureRegistry::new(),
            structure_processors: StructureProcessorListRegistry::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use rustc_hash::FxHashMap;
    use steel_utils::Identifier;

    use crate::biome::{Biome, BiomeEffects, GrassColorModifier, TemperatureModifier};

    use super::{Registry, RegistryExt};

    fn biome_with_refs(carvers: Vec<Identifier>, features: Vec<Vec<Identifier>>) -> &'static Biome {
        Box::leak(Box::new(Biome {
            key: Identifier::new_static("test", "missing_carver_biome"),
            has_precipitation: false,
            temperature: 0.5,
            downfall: 0.0,
            temperature_modifier: TemperatureModifier::None,
            effects: BiomeEffects {
                fog_color: 0,
                sky_color: 0,
                water_color: 0,
                water_fog_color: 0,
                foliage_color: None,
                grass_color: None,
                dry_foliage_color: None,
                grass_color_modifier: GrassColorModifier::None,
                music: None,
                ambient_sound: None,
                additions_sound: None,
                mood_sound: None,
                particle: None,
            },
            creature_spawn_probability: 0.0,
            spawners: FxHashMap::default(),
            spawn_costs: FxHashMap::default(),
            carvers,
            features,
            id: OnceLock::new(),
        }))
    }

    #[test]
    #[should_panic(expected = "references unknown configured carver")]
    fn freeze_rejects_missing_biome_carver_reference() {
        let mut registry = Registry::new_empty();
        registry.biomes.register(biome_with_refs(
            vec![Identifier::vanilla_static("missing_carver")],
            Vec::new(),
        ));

        registry.freeze();
    }

    #[test]
    #[should_panic(expected = "references unknown placed feature")]
    fn freeze_rejects_missing_biome_placed_feature_reference() {
        let mut registry = Registry::new_empty();
        registry.biomes.register(biome_with_refs(
            Vec::new(),
            vec![vec![Identifier::vanilla_static("missing_feature")]],
        ));

        registry.freeze();
    }

    #[test]
    fn vanilla_feature_registries_initialize_and_validate() {
        let mut registry = Registry::new_vanilla();
        registry.freeze();

        assert!(
            registry
                .configured_features
                .by_key(&Identifier::vanilla_static("ore_diamond_small"))
                .is_some()
        );
        assert!(
            registry
                .placed_features
                .by_key(&Identifier::vanilla_static("ore_diamond"))
                .is_some()
        );
    }

    #[test]
    fn vanilla_static_entity_data_registries_initialize_in_vanilla_order() {
        let registry = Registry::new_vanilla();
        let entity_effect = Identifier::vanilla_static("entity_effect");
        let dust = Identifier::vanilla_static("dust");
        let plains = Identifier::vanilla_static("plains");
        let none = Identifier::vanilla_static("none");
        let tabby = Identifier::vanilla_static("tabby");
        let angry = Identifier::vanilla_static("angry");
        let big = Identifier::vanilla_static("big");
        let earth = Identifier::vanilla_static("earth");

        assert_eq!(
            registry.particle_types.by_id(21).map(|entry| &entry.key),
            Some(&dust)
        );
        assert_eq!(
            registry.particle_types.by_id(28).map(|entry| &entry.key),
            Some(&entity_effect)
        );
        assert_eq!(
            registry.villager_types.by_id(2).map(|entry| &entry.key),
            Some(&plains)
        );
        assert_eq!(
            registry
                .villager_professions
                .by_id(0)
                .map(|entry| &entry.key),
            Some(&none)
        );
        assert_eq!(
            registry.cat_variants.by_id(0).map(|entry| &entry.key),
            Some(&tabby)
        );
        assert_eq!(
            registry
                .wolf_sound_variants
                .by_id(3)
                .map(|entry| &entry.key),
            Some(&angry)
        );
        assert_eq!(
            registry.pig_sound_variants.by_id(1).map(|entry| &entry.key),
            Some(&big)
        );
        assert_eq!(
            registry.painting_variants.by_id(25).map(|entry| &entry.key),
            Some(&earth)
        );
    }

    #[test]
    fn vanilla_game_events_initialize_in_vanilla_order() {
        let registry = Registry::new_vanilla();
        let block_activate = Identifier::vanilla_static("block_activate");
        let unequip = Identifier::vanilla_static("unequip");
        let resonate_1 = Identifier::vanilla_static("resonate_1");
        let resonate_10 = Identifier::vanilla_static("resonate_10");

        assert_eq!(
            registry.game_events.by_id(0).map(|event| &event.key),
            Some(&block_activate)
        );
        assert_eq!(
            registry.game_events.by_id(45).map(|event| &event.key),
            Some(&unequip)
        );
        assert_eq!(
            registry.game_events.by_id(46).map(|event| &event.key),
            Some(&resonate_1)
        );
        assert_eq!(
            registry.game_events.by_id(55).map(|event| &event.key),
            Some(&resonate_10)
        );
    }
}
