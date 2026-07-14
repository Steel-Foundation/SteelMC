//! Vanilla data component definitions and registration.
//!
//! This module defines all vanilla Minecraft data components and provides
//! the registration function to add them to the registry.
use steel_utils::{Identifier, nbt::NbtNumeric as _};
use text_components::TextComponent;

use super::component_data::ComponentData;
use super::registry::DataComponentRegistry;
pub use super::registry::DataComponentType;
pub use crate::attribute::AttributeModifierOperation;
pub use crate::equipment::{EquipmentSlot, EquipmentSlotGroup};

// Re-export component types for convenience
pub use super::components::{
    AttackRange, CustomData, CustomModelData, DamageResistant, DamageTypeComponent, DyedItemColor,
    Enchantable, Equippable, EquippableAllowedEntities, InstrumentComponent,
    InvalidEnchantableValue, ItemAttributeModifierDisplay, ItemAttributeModifierEntry,
    ItemAttributeModifiers, ItemEnchantments, ItemLore, ItemLoreTooLong, JukeboxPlayable, MapId,
    MapItemColor, MapPostProcessing, OminousBottleAmplifier, PiercingWeapon,
    ProvidesBannerPatterns, ProvidesTrimMaterial, Rarity, Repairable, SwingAnimation,
    SwingAnimationType, Tool, ToolRule, ToolRuleBlocks, TooltipDisplay, UseCooldown, UseEffects,
    Weapon,
};
pub use crate::sound_event::SoundEventHolder;
pub use crate::{
    AxolotlVariant, DyeColor, FoxVariant, HorseVariant, LlamaVariant, MooshroomVariant,
    ParrotVariant, RabbitVariant, SalmonVariant, TropicalFishPattern,
};

pub const MAX_STACK_SIZE: DataComponentType<i32> =
    DataComponentType::new(Identifier::vanilla_static("max_stack_size"));

pub const MAX_DAMAGE: DataComponentType<i32> =
    DataComponentType::new(Identifier::vanilla_static("max_damage"));

pub const CUSTOM_NAME: DataComponentType<TextComponent> =
    DataComponentType::new(Identifier::vanilla_static("custom_name"));

pub const ITEM_NAME: DataComponentType<TextComponent> =
    DataComponentType::new(Identifier::vanilla_static("item_name"));

pub const DAMAGE: DataComponentType<i32> =
    DataComponentType::new_ignoring_swap_animation(Identifier::vanilla_static("damage"));

pub const REPAIR_COST: DataComponentType<i32> =
    DataComponentType::new(Identifier::vanilla_static("repair_cost"));

pub const UNBREAKABLE: DataComponentType<()> =
    DataComponentType::new(Identifier::vanilla_static("unbreakable"));

pub const TOOL: DataComponentType<Tool> =
    DataComponentType::new(Identifier::vanilla_static("tool"));

pub const WEAPON: DataComponentType<Weapon> =
    DataComponentType::new(Identifier::vanilla_static("weapon"));

pub const ATTACK_RANGE: DataComponentType<AttackRange> =
    DataComponentType::new(Identifier::vanilla_static("attack_range"));

pub const EQUIPPABLE: DataComponentType<Equippable> =
    DataComponentType::new(Identifier::vanilla_static("equippable"));

pub const GLIDER: DataComponentType<()> =
    DataComponentType::new(Identifier::vanilla_static("glider"));

pub const CREATIVE_SLOT_LOCK: DataComponentType<()> =
    DataComponentType::new(Identifier::vanilla_static("creative_slot_lock"));

pub const INTANGIBLE_PROJECTILE: DataComponentType<()> =
    DataComponentType::new(Identifier::vanilla_static("intangible_projectile"));

pub const ENCHANTMENT_GLINT_OVERRIDE: DataComponentType<bool> =
    DataComponentType::new(Identifier::vanilla_static("enchantment_glint_override"));

pub const POTION_DURATION_SCALE: DataComponentType<f32> =
    DataComponentType::new(Identifier::vanilla_static("potion_duration_scale"));

/// Type marker for vanilla component IDs whose value and codecs are not ported.
///
/// The empty enum cannot be constructed and deliberately does not implement
/// [`super::Component`], so an unimplemented component cannot be inserted into
/// an item through the typed component APIs.
pub enum UnimplementedComponent {}

// These component IDs are reserved in vanilla order until their concrete value
// types and codecs are ported.

pub const CUSTOM_DATA: DataComponentType<CustomData> =
    DataComponentType::new(Identifier::vanilla_static("custom_data"));

pub const USE_EFFECTS: DataComponentType<UseEffects> =
    DataComponentType::new(Identifier::vanilla_static("use_effects"));

pub const MINIMUM_ATTACK_CHARGE: DataComponentType<f32> =
    DataComponentType::new(Identifier::vanilla_static("minimum_attack_charge"));

pub const DAMAGE_TYPE: DataComponentType<DamageTypeComponent> =
    DataComponentType::new(Identifier::vanilla_static("damage_type"));

pub const ITEM_MODEL: DataComponentType<Identifier> =
    DataComponentType::new(Identifier::vanilla_static("item_model"));

pub const LORE: DataComponentType<ItemLore> =
    DataComponentType::new(Identifier::vanilla_static("lore"));

pub const RARITY: DataComponentType<Rarity> =
    DataComponentType::new(Identifier::vanilla_static("rarity"));

pub const ENCHANTMENTS: DataComponentType<ItemEnchantments> =
    DataComponentType::new(Identifier::vanilla_static("enchantments"));

pub const CAN_PLACE_ON: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("can_place_on"));

pub const CAN_BREAK: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("can_break"));

pub const ATTRIBUTE_MODIFIERS: DataComponentType<ItemAttributeModifiers> =
    DataComponentType::new(Identifier::vanilla_static("attribute_modifiers"));

pub const CUSTOM_MODEL_DATA: DataComponentType<CustomModelData> =
    DataComponentType::new(Identifier::vanilla_static("custom_model_data"));

pub const TOOLTIP_DISPLAY: DataComponentType<TooltipDisplay> =
    DataComponentType::new(Identifier::vanilla_static("tooltip_display"));

pub const TOOLTIP_STYLE: DataComponentType<Identifier> =
    DataComponentType::new(Identifier::vanilla_static("tooltip_style"));

pub const NOTE_BLOCK_SOUND: DataComponentType<Identifier> =
    DataComponentType::new(Identifier::vanilla_static("note_block_sound"));

pub const FOOD: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("food"));

pub const CONSUMABLE: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("consumable"));

pub const USE_REMAINDER: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("use_remainder"));

pub const USE_COOLDOWN: DataComponentType<UseCooldown> =
    DataComponentType::new(Identifier::vanilla_static("use_cooldown"));

pub const DAMAGE_RESISTANT: DataComponentType<DamageResistant> =
    DataComponentType::new(Identifier::vanilla_static("damage_resistant"));

pub const ENCHANTABLE: DataComponentType<Enchantable> =
    DataComponentType::new(Identifier::vanilla_static("enchantable"));

pub const REPAIRABLE: DataComponentType<Repairable> =
    DataComponentType::new(Identifier::vanilla_static("repairable"));

pub const DEATH_PROTECTION: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("death_protection"));

pub const BLOCKS_ATTACKS: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("blocks_attacks"));

pub const PIERCING_WEAPON: DataComponentType<PiercingWeapon> =
    DataComponentType::new(Identifier::vanilla_static("piercing_weapon"));

pub const KINETIC_WEAPON: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("kinetic_weapon"));

pub const SWING_ANIMATION: DataComponentType<SwingAnimation> =
    DataComponentType::new(Identifier::vanilla_static("swing_animation"));

pub const ADDITIONAL_TRADE_COST: DataComponentType<i32> =
    DataComponentType::new(Identifier::vanilla_static("additional_trade_cost"));

pub const STORED_ENCHANTMENTS: DataComponentType<ItemEnchantments> =
    DataComponentType::new(Identifier::vanilla_static("stored_enchantments"));

pub const DYE: DataComponentType<DyeColor> =
    DataComponentType::new(Identifier::vanilla_static("dye"));

pub const DYED_COLOR: DataComponentType<DyedItemColor> =
    DataComponentType::new(Identifier::vanilla_static("dyed_color"));

pub const MAP_COLOR: DataComponentType<MapItemColor> =
    DataComponentType::new(Identifier::vanilla_static("map_color"));

pub const MAP_ID: DataComponentType<MapId> =
    DataComponentType::new(Identifier::vanilla_static("map_id"));

pub const MAP_DECORATIONS: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("map_decorations"));

pub const MAP_POST_PROCESSING: DataComponentType<MapPostProcessing> =
    DataComponentType::new(Identifier::vanilla_static("map_post_processing"));

pub const CHARGED_PROJECTILES: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("charged_projectiles"));

pub const BUNDLE_CONTENTS: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("bundle_contents"));

pub const POTION_CONTENTS: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("potion_contents"));

pub const SUSPICIOUS_STEW_EFFECTS: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("suspicious_stew_effects"));

pub const WRITABLE_BOOK_CONTENT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("writable_book_content"));

pub const WRITTEN_BOOK_CONTENT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("written_book_content"));

pub const TRIM: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("trim"));

pub const DEBUG_STICK_STATE: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("debug_stick_state"));

pub const ENTITY_DATA: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("entity_data"));

pub const BUCKET_ENTITY_DATA: DataComponentType<CustomData> =
    DataComponentType::new(Identifier::vanilla_static("bucket_entity_data"));

pub const BLOCK_ENTITY_DATA: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("block_entity_data"));

pub const INSTRUMENT: DataComponentType<InstrumentComponent> =
    DataComponentType::new(Identifier::vanilla_static("instrument"));

pub const PROVIDES_TRIM_MATERIAL: DataComponentType<ProvidesTrimMaterial> =
    DataComponentType::new(Identifier::vanilla_static("provides_trim_material"));

pub const OMINOUS_BOTTLE_AMPLIFIER: DataComponentType<OminousBottleAmplifier> =
    DataComponentType::new(Identifier::vanilla_static("ominous_bottle_amplifier"));

pub const JUKEBOX_PLAYABLE: DataComponentType<JukeboxPlayable> =
    DataComponentType::new(Identifier::vanilla_static("jukebox_playable"));

pub const PROVIDES_BANNER_PATTERNS: DataComponentType<ProvidesBannerPatterns> =
    DataComponentType::new(Identifier::vanilla_static("provides_banner_patterns"));

pub const RECIPES: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("recipes"));

pub const LODESTONE_TRACKER: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("lodestone_tracker"));

pub const FIREWORK_EXPLOSION: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("firework_explosion"));

pub const FIREWORKS: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("fireworks"));

pub const PROFILE: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("profile"));

pub const BANNER_PATTERNS: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("banner_patterns"));

pub const BASE_COLOR: DataComponentType<DyeColor> =
    DataComponentType::new(Identifier::vanilla_static("base_color"));

pub const POT_DECORATIONS: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("pot_decorations"));

pub const CONTAINER: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("container"));

pub const BLOCK_STATE: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("block_state"));

pub const BEES: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("bees"));

pub const SULFUR_CUBE_CONTENT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("sulfur_cube_content"));

pub const LOCK: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("lock"));

pub const CONTAINER_LOOT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("container_loot"));

pub const BREAK_SOUND: DataComponentType<SoundEventHolder> =
    DataComponentType::new(Identifier::vanilla_static("break_sound"));

// Entity variant components
pub const VILLAGER_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("villager/variant"));

pub const WOLF_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("wolf/variant"));

pub const WOLF_SOUND_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("wolf/sound_variant"));

pub const WOLF_COLLAR: DataComponentType<DyeColor> =
    DataComponentType::new(Identifier::vanilla_static("wolf/collar"));

pub const FOX_VARIANT: DataComponentType<FoxVariant> =
    DataComponentType::new(Identifier::vanilla_static("fox/variant"));

pub const SALMON_SIZE: DataComponentType<SalmonVariant> =
    DataComponentType::new(Identifier::vanilla_static("salmon/size"));

pub const PARROT_VARIANT: DataComponentType<ParrotVariant> =
    DataComponentType::new(Identifier::vanilla_static("parrot/variant"));

pub const TROPICAL_FISH_PATTERN: DataComponentType<TropicalFishPattern> =
    DataComponentType::new(Identifier::vanilla_static("tropical_fish/pattern"));

pub const TROPICAL_FISH_BASE_COLOR: DataComponentType<DyeColor> =
    DataComponentType::new(Identifier::vanilla_static("tropical_fish/base_color"));

pub const TROPICAL_FISH_PATTERN_COLOR: DataComponentType<DyeColor> =
    DataComponentType::new(Identifier::vanilla_static("tropical_fish/pattern_color"));

pub const MOOSHROOM_VARIANT: DataComponentType<MooshroomVariant> =
    DataComponentType::new(Identifier::vanilla_static("mooshroom/variant"));

pub const RABBIT_VARIANT: DataComponentType<RabbitVariant> =
    DataComponentType::new(Identifier::vanilla_static("rabbit/variant"));

pub const PIG_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("pig/variant"));

pub const PIG_SOUND_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("pig/sound_variant"));

pub const COW_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("cow/variant"));

pub const COW_SOUND_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("cow/sound_variant"));

pub const CHICKEN_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("chicken/variant"));

pub const CHICKEN_SOUND_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("chicken/sound_variant"));

pub const ZOMBIE_NAUTILUS_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("zombie_nautilus/variant"));

pub const FROG_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("frog/variant"));

pub const HORSE_VARIANT: DataComponentType<HorseVariant> =
    DataComponentType::new(Identifier::vanilla_static("horse/variant"));

pub const PAINTING_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("painting/variant"));

pub const LLAMA_VARIANT: DataComponentType<LlamaVariant> =
    DataComponentType::new(Identifier::vanilla_static("llama/variant"));

pub const AXOLOTL_VARIANT: DataComponentType<AxolotlVariant> =
    DataComponentType::new(Identifier::vanilla_static("axolotl/variant"));

pub const CAT_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("cat/variant"));

pub const CAT_SOUND_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("cat/sound_variant"));

pub const CAT_COLLAR: DataComponentType<DyeColor> =
    DataComponentType::new(Identifier::vanilla_static("cat/collar"));

pub const SHEEP_COLOR: DataComponentType<DyeColor> =
    DataComponentType::new(Identifier::vanilla_static("sheep/color"));

pub const SHULKER_COLOR: DataComponentType<DyeColor> =
    DataComponentType::new(Identifier::vanilla_static("shulker/color"));

/// Network reader for VarInt-encoded i32 components.
fn varint_reader(cursor: &mut std::io::Cursor<&[u8]>) -> std::io::Result<ComponentData> {
    use steel_utils::{codec::VarInt, serial::ReadFrom};
    let value = VarInt::read(cursor)?;
    Ok(ComponentData::new(value.0))
}

/// Network writer for VarInt-encoded i32 components.
fn varint_writer(data: &ComponentData, writer: &mut Vec<u8>) -> std::io::Result<()> {
    use steel_utils::{codec::VarInt, serial::WriteTo};
    if let Some(v) = data.downcast_ref::<i32>() {
        VarInt(*v).write(writer)
    } else {
        Err(std::io::Error::other("Component type mismatch"))
    }
}

fn float_reader(cursor: &mut std::io::Cursor<&[u8]>) -> std::io::Result<ComponentData> {
    use steel_utils::serial::ReadFrom;
    Ok(ComponentData::new(f32::read(cursor)?))
}

fn float_writer(data: &ComponentData, writer: &mut Vec<u8>) -> std::io::Result<()> {
    use steel_utils::serial::WriteTo;
    let Some(value) = data.downcast_ref::<f32>() else {
        return Err(std::io::Error::other("Component type mismatch"));
    };
    value.write(writer)
}

fn bool_reader(cursor: &mut std::io::Cursor<&[u8]>) -> std::io::Result<ComponentData> {
    use steel_utils::serial::ReadFrom;
    Ok(ComponentData::new(bool::read(cursor)?))
}

fn bool_writer(data: &ComponentData, writer: &mut Vec<u8>) -> std::io::Result<()> {
    use steel_utils::serial::WriteTo;
    let Some(value) = data.downcast_ref::<bool>() else {
        return Err(std::io::Error::other("Component type mismatch"));
    };
    value.write(writer)
}

fn custom_data_codec_reader(cursor: &mut std::io::Cursor<&[u8]>) -> std::io::Result<ComponentData> {
    CustomData::read_codec_network(cursor).map(ComponentData::new)
}

fn custom_data_writer(data: &ComponentData, writer: &mut Vec<u8>) -> std::io::Result<()> {
    use steel_utils::serial::WriteTo as _;
    let Some(value) = data.downcast_ref::<CustomData>() else {
        return Err(std::io::Error::other("Component type mismatch"));
    };
    value.write(writer)
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "network reader function pointers return io::Result"
)]
fn unit_reader(_cursor: &mut std::io::Cursor<&[u8]>) -> std::io::Result<ComponentData> {
    Ok(ComponentData::new(()))
}

fn unit_writer(data: &ComponentData, _writer: &mut Vec<u8>) -> std::io::Result<()> {
    if data.downcast_ref::<()>().is_some() {
        Ok(())
    } else {
        Err(std::io::Error::other("Component type mismatch"))
    }
}

fn ranged_i32_nbt_reader<const MIN: i32, const MAX: i32>(
    tag: simdnbt::borrow::NbtTag,
) -> Option<ComponentData> {
    let value = tag.codec_i32()?;
    (MIN..=MAX)
        .contains(&value)
        .then(|| ComponentData::new(value))
}

fn ranged_i32_nbt_writer<const MIN: i32, const MAX: i32>(
    data: &ComponentData,
) -> std::io::Result<simdnbt::owned::NbtTag> {
    let Some(value) = data.downcast_ref::<i32>() else {
        return Err(std::io::Error::other("Component type mismatch"));
    };
    if !(MIN..=MAX).contains(value) {
        return Err(std::io::Error::other(format!(
            "Value {value} outside of range [{MIN}:{MAX}]"
        )));
    }
    Ok(simdnbt::owned::NbtTag::Int(*value))
}

fn minimum_attack_charge_nbt_reader(tag: simdnbt::borrow::NbtTag) -> Option<ComponentData> {
    let value = tag.codec_f32()?;
    (value.is_finite() && !value.is_sign_negative() && value <= 1.0)
        .then(|| ComponentData::new(value))
}

fn potion_duration_scale_nbt_reader(tag: simdnbt::borrow::NbtTag) -> Option<ComponentData> {
    let value = tag.codec_f32()?;
    (value.is_finite() && !value.is_sign_negative()).then(|| ComponentData::new(value))
}

fn minimum_attack_charge_nbt_writer(
    data: &ComponentData,
) -> std::io::Result<simdnbt::owned::NbtTag> {
    let Some(value) = data.downcast_ref::<f32>() else {
        return Err(std::io::Error::other("Component type mismatch"));
    };
    if !value.is_finite() || value.is_sign_negative() || *value > 1.0 {
        return Err(std::io::Error::other(format!(
            "Value {value} outside of range [0:1]"
        )));
    }
    Ok(simdnbt::owned::NbtTag::Float(*value))
}

fn potion_duration_scale_nbt_writer(
    data: &ComponentData,
) -> std::io::Result<simdnbt::owned::NbtTag> {
    let Some(value) = data.downcast_ref::<f32>() else {
        return Err(std::io::Error::other("Component type mismatch"));
    };
    if !value.is_finite() || value.is_sign_negative() {
        return Err(std::io::Error::other(format!(
            "Value {value} must be non-negative and finite"
        )));
    }
    Ok(simdnbt::owned::NbtTag::Float(*value))
}

fn bool_nbt_reader(tag: simdnbt::borrow::NbtTag) -> Option<ComponentData> {
    tag.codec_bool().map(ComponentData::new)
}

fn bool_nbt_writer(data: &ComponentData) -> std::io::Result<simdnbt::owned::NbtTag> {
    let Some(value) = data.downcast_ref::<bool>() else {
        return Err(std::io::Error::other("Component type mismatch"));
    };
    Ok(simdnbt::owned::NbtTag::Byte(i8::from(*value)))
}

fn unit_nbt_reader(tag: simdnbt::borrow::NbtTag) -> Option<ComponentData> {
    tag.compound().map(|_| ComponentData::new(()))
}

fn unit_nbt_writer(data: &ComponentData) -> std::io::Result<simdnbt::owned::NbtTag> {
    if data.downcast_ref::<()>().is_none() {
        return Err(std::io::Error::other("Component type mismatch"));
    }
    Ok(simdnbt::owned::NbtTag::Compound(
        simdnbt::owned::NbtCompound::new(),
    ))
}

fn jukebox_playable_network_reader(
    cursor: &mut std::io::Cursor<&[u8]>,
) -> std::io::Result<ComponentData> {
    use steel_utils::serial::ReadFrom;
    JukeboxPlayable::read(cursor).map(ComponentData::new)
}

fn jukebox_playable_network_writer(
    data: &ComponentData,
    writer: &mut Vec<u8>,
) -> std::io::Result<()> {
    use steel_utils::serial::WriteTo;
    let Some(value) = data.downcast_ref::<JukeboxPlayable>() else {
        return Err(std::io::Error::other("Component type mismatch"));
    };
    value.write(writer)
}

fn jukebox_playable_nbt_reader(tag: simdnbt::borrow::NbtTag) -> Option<ComponentData> {
    JukeboxPlayable::from_persistent_nbt(tag).map(ComponentData::new)
}

fn jukebox_playable_nbt_writer(data: &ComponentData) -> std::io::Result<simdnbt::owned::NbtTag> {
    let Some(value) = data.downcast_ref::<JukeboxPlayable>() else {
        return Err(std::io::Error::other("Component type mismatch"));
    };
    Ok(value.to_persistent_nbt())
}

macro_rules! register_ranged_i32 {
    ($registry:expr, $component:expr, $min:expr, $max:expr) => {
        $registry.register_with_codecs(
            $component,
            varint_reader,
            varint_writer,
            ranged_i32_nbt_reader::<{ $min }, { $max }>,
            ranged_i32_nbt_writer::<{ $min }, { $max }>,
        );
    };
}

macro_rules! register_unit {
    ($registry:expr, $component:expr) => {
        $registry.register_with_codecs(
            $component,
            unit_reader,
            unit_writer,
            unit_nbt_reader,
            unit_nbt_writer,
        );
    };
}

/// Registers all vanilla data components.
///
/// IMPORTANT: The registration order MUST match vanilla's DataComponents.java exactly,
/// as the component's network ID is determined by its registration order.
pub fn register_vanilla_data_components(registry: &mut DataComponentRegistry) {
    // Order must match vanilla's DataComponents.java exactly!
    // 0: custom_data
    registry.register_custom_network(CUSTOM_DATA, custom_data_codec_reader, custom_data_writer);
    // 1: max_stack_size
    register_ranged_i32!(registry, MAX_STACK_SIZE, 1, 99);
    // 2: max_damage
    register_ranged_i32!(registry, MAX_DAMAGE, 1, i32::MAX);
    // 3: damage
    register_ranged_i32!(registry, DAMAGE, 0, i32::MAX);
    // 4: unbreakable
    register_unit!(registry, UNBREAKABLE);
    // 5: use_effects
    registry.register(USE_EFFECTS);
    // 6: custom_name
    registry.register(CUSTOM_NAME);
    // 7: minimum_attack_charge
    registry.register_with_codecs(
        MINIMUM_ATTACK_CHARGE,
        float_reader,
        float_writer,
        minimum_attack_charge_nbt_reader,
        minimum_attack_charge_nbt_writer,
    );
    // 8: damage_type
    registry.register(DAMAGE_TYPE);
    // 9: item_name
    registry.register(ITEM_NAME);
    // 10: item_model
    registry.register(ITEM_MODEL);
    // 11: lore
    registry.register(LORE);
    // 12: rarity
    registry.register(RARITY);
    // 13: enchantments
    registry.register(ENCHANTMENTS);
    // 14: can_place_on
    registry.register_unimplemented(CAN_PLACE_ON, true);
    // 15: can_break
    registry.register_unimplemented(CAN_BREAK, true);
    // 16: attribute_modifiers
    registry.register(ATTRIBUTE_MODIFIERS);
    // 17: custom_model_data
    registry.register(CUSTOM_MODEL_DATA);
    // 18: tooltip_display
    registry.register(TOOLTIP_DISPLAY);
    // 19: repair_cost
    register_ranged_i32!(registry, REPAIR_COST, 0, i32::MAX);
    // 20: creative_slot_lock
    registry.register_transient(CREATIVE_SLOT_LOCK);
    // 21: enchantment_glint_override
    registry.register_with_codecs(
        ENCHANTMENT_GLINT_OVERRIDE,
        bool_reader,
        bool_writer,
        bool_nbt_reader,
        bool_nbt_writer,
    );
    // 22: intangible_projectile
    register_unit!(registry, INTANGIBLE_PROJECTILE);
    // 23: food
    registry.register_unimplemented(FOOD, true);
    // 24: consumable
    registry.register_unimplemented(CONSUMABLE, true);
    // 25: use_remainder
    registry.register_unimplemented(USE_REMAINDER, true);
    // 26: use_cooldown
    registry.register(USE_COOLDOWN);
    // 27: damage_resistant
    registry.register(DAMAGE_RESISTANT);
    // 28: tool
    registry.register(TOOL);
    // 29: weapon
    registry.register(WEAPON);
    // 30: attack_range
    registry.register(ATTACK_RANGE);
    // 31: enchantable
    registry.register(ENCHANTABLE);
    // 32: equippable
    registry.register(EQUIPPABLE);
    // 33: repairable
    registry.register(REPAIRABLE);
    // 34: glider
    register_unit!(registry, GLIDER);
    // 35: tooltip_style
    registry.register(TOOLTIP_STYLE);
    // 36: death_protection
    registry.register_unimplemented(DEATH_PROTECTION, true);
    // 37: blocks_attacks
    registry.register_unimplemented(BLOCKS_ATTACKS, true);
    // 38: piercing_weapon
    registry.register(PIERCING_WEAPON);
    // 39: kinetic_weapon
    registry.register_unimplemented(KINETIC_WEAPON, true);
    // 40: swing_animation
    registry.register(SWING_ANIMATION);
    // 41: additional_trade_cost
    registry.register_transient_with_codecs(ADDITIONAL_TRADE_COST, varint_reader, varint_writer);
    // 42: stored_enchantments
    registry.register(STORED_ENCHANTMENTS);
    // 43: dye
    registry.register(DYE);
    // 44: dyed_color
    registry.register(DYED_COLOR);
    // 45: map_color
    registry.register(MAP_COLOR);
    // 46: map_id
    registry.register(MAP_ID);
    // 47: map_decorations
    registry.register_unimplemented(MAP_DECORATIONS, true);
    // 48: map_post_processing
    registry.register_transient(MAP_POST_PROCESSING);
    // 49: charged_projectiles
    registry.register_unimplemented(CHARGED_PROJECTILES, true);
    // 50: bundle_contents
    registry.register_unimplemented(BUNDLE_CONTENTS, true);
    // 51: potion_contents
    registry.register_unimplemented(POTION_CONTENTS, true);
    // 52: potion_duration_scale
    registry.register_with_codecs(
        POTION_DURATION_SCALE,
        float_reader,
        float_writer,
        potion_duration_scale_nbt_reader,
        potion_duration_scale_nbt_writer,
    );
    // 53: suspicious_stew_effects
    registry.register_unimplemented(SUSPICIOUS_STEW_EFFECTS, true);
    // 54: writable_book_content
    registry.register_unimplemented(WRITABLE_BOOK_CONTENT, true);
    // 55: written_book_content
    registry.register_unimplemented(WRITTEN_BOOK_CONTENT, true);
    // 56: trim
    registry.register_unimplemented(TRIM, true);
    // 57: debug_stick_state
    registry.register_unimplemented(DEBUG_STICK_STATE, true);
    // 58: entity_data
    registry.register_unimplemented(ENTITY_DATA, true);
    // 59: bucket_entity_data
    registry.register(BUCKET_ENTITY_DATA);
    // 60: block_entity_data
    registry.register_unimplemented(BLOCK_ENTITY_DATA, true);
    // 61: instrument
    registry.register(INSTRUMENT);
    // 62: provides_trim_material
    registry.register(PROVIDES_TRIM_MATERIAL);
    // 63: ominous_bottle_amplifier
    registry.register(OMINOUS_BOTTLE_AMPLIFIER);
    // 64: jukebox_playable
    registry.register_with_codecs(
        JUKEBOX_PLAYABLE,
        jukebox_playable_network_reader,
        jukebox_playable_network_writer,
        jukebox_playable_nbt_reader,
        jukebox_playable_nbt_writer,
    );
    // 65: provides_banner_patterns
    registry.register(PROVIDES_BANNER_PATTERNS);
    // 66: recipes
    registry.register_unimplemented(RECIPES, true);
    // 67: lodestone_tracker
    registry.register_unimplemented(LODESTONE_TRACKER, true);
    // 68: firework_explosion
    registry.register_unimplemented(FIREWORK_EXPLOSION, true);
    // 69: fireworks
    registry.register_unimplemented(FIREWORKS, true);
    // 70: profile
    registry.register_unimplemented(PROFILE, true);
    // 71: note_block_sound
    registry.register(NOTE_BLOCK_SOUND);
    // 72: banner_patterns
    registry.register_unimplemented(BANNER_PATTERNS, true);
    // 73: base_color
    registry.register(BASE_COLOR);
    // 74: pot_decorations
    registry.register_unimplemented(POT_DECORATIONS, true);
    // 75: container
    registry.register_unimplemented(CONTAINER, true);
    // 76: block_state
    registry.register_unimplemented(BLOCK_STATE, true);
    // 77: bees
    registry.register_unimplemented(BEES, true);
    // 78: sulfur_cube_content
    registry.register_unimplemented(SULFUR_CUBE_CONTENT, true);
    // 79: lock
    registry.register_unimplemented(LOCK, true);
    // 80: container_loot
    registry.register_unimplemented(CONTAINER_LOOT, true);
    // 81: break_sound
    registry.register(BREAK_SOUND);
    // 82: villager/variant
    registry.register_unimplemented(VILLAGER_VARIANT, true);
    // 83: wolf/variant
    registry.register_unimplemented(WOLF_VARIANT, true);
    // 84: wolf/sound_variant
    registry.register_unimplemented(WOLF_SOUND_VARIANT, true);
    // 85: wolf/collar
    registry.register(WOLF_COLLAR);
    // 86: fox/variant
    registry.register(FOX_VARIANT);
    // 87: salmon/size
    registry.register(SALMON_SIZE);
    // 88: parrot/variant
    registry.register(PARROT_VARIANT);
    // 89: tropical_fish/pattern
    registry.register(TROPICAL_FISH_PATTERN);
    // 90: tropical_fish/base_color
    registry.register(TROPICAL_FISH_BASE_COLOR);
    // 91: tropical_fish/pattern_color
    registry.register(TROPICAL_FISH_PATTERN_COLOR);
    // 92: mooshroom/variant
    registry.register(MOOSHROOM_VARIANT);
    // 93: rabbit/variant
    registry.register(RABBIT_VARIANT);
    // 94: pig/variant
    registry.register_unimplemented(PIG_VARIANT, true);
    // 95: pig/sound_variant
    registry.register_unimplemented(PIG_SOUND_VARIANT, true);
    // 96: cow/variant
    registry.register_unimplemented(COW_VARIANT, true);
    // 97: cow/sound_variant
    registry.register_unimplemented(COW_SOUND_VARIANT, true);
    // 98: chicken/variant
    registry.register_unimplemented(CHICKEN_VARIANT, true);
    // 99: chicken/sound_variant
    registry.register_unimplemented(CHICKEN_SOUND_VARIANT, true);
    // 100: zombie_nautilus/variant
    registry.register_unimplemented(ZOMBIE_NAUTILUS_VARIANT, true);
    // 101: frog/variant
    registry.register_unimplemented(FROG_VARIANT, true);
    // 102: horse/variant
    registry.register(HORSE_VARIANT);
    // 103: painting/variant
    registry.register_unimplemented(PAINTING_VARIANT, true);
    // 104: llama/variant
    registry.register(LLAMA_VARIANT);
    // 105: axolotl/variant
    registry.register(AXOLOTL_VARIANT);
    // 106: cat/variant
    registry.register_unimplemented(CAT_VARIANT, true);
    // 107: cat/sound_variant
    registry.register_unimplemented(CAT_SOUND_VARIANT, true);
    // 108: cat/collar
    registry.register(CAT_COLLAR);
    // 109: sheep/color
    registry.register(SHEEP_COLOR);
    // 110: shulker/color
    registry.register(SHULKER_COLOR);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RegistryExt;
    use serde::Deserialize;
    use simdnbt::owned::{NbtCompound, NbtTag};

    #[derive(Deserialize)]
    struct ExtractedComponentCatalog {
        components: Vec<ExtractedComponent>,
    }

    #[derive(Deserialize)]
    struct ExtractedComponent {
        id: usize,
        key: String,
        persistent: bool,
        ignore_swap_animation: bool,
    }

    #[test]
    fn registry_matches_extracted_vanilla_catalog() {
        let catalog: ExtractedComponentCatalog =
            serde_json::from_str(include_str!("../../build_assets/data_components.json"))
                .expect("extracted component catalog should be valid");
        let mut registry = DataComponentRegistry::new();
        register_vanilla_data_components(&mut registry);

        assert_eq!(catalog.components.len(), 111);
        assert_eq!(registry.len(), catalog.components.len());
        for (expected_id, component) in catalog.components.into_iter().enumerate() {
            assert_eq!(component.id, expected_id, "{}", component.key);
            let entry = registry
                .by_id(component.id)
                .unwrap_or_else(|| panic!("missing component registry ID {}", component.id));
            assert_eq!(entry.key.to_string(), component.key);
            assert_eq!(entry.is_persistent(), component.persistent, "{}", entry.key);
            assert_eq!(
                entry.ignore_swap_animation(),
                component.ignore_swap_animation,
                "{}",
                entry.key
            );
        }
    }

    #[test]
    fn vanilla_transient_components_are_marked_non_persistent() {
        let mut registry = DataComponentRegistry::new();
        register_vanilla_data_components(&mut registry);

        for (key, value) in [
            (&CREATIVE_SLOT_LOCK.key, ComponentData::new(())),
            (&ADDITIONAL_TRADE_COST.key, ComponentData::new(3_i32)),
            (
                &MAP_POST_PROCESSING.key,
                ComponentData::new(MapPostProcessing::Lock),
            ),
        ] {
            let entry = registry
                .by_key(key)
                .unwrap_or_else(|| panic!("missing transient component {key}"));
            assert!(entry.is_implemented(), "{key}");
            assert!(!entry.is_persistent(), "{key}");
            assert!(entry.write_nbt(&value).is_err(), "{key}");
            assert!(entry.compute_hash(&value).is_err(), "{key}");
        }
        assert!(matches!(
            registry.by_key(&MAX_STACK_SIZE.key),
            Some(entry) if entry.is_persistent()
        ));
    }

    #[test]
    fn transient_component_network_codecs_match_vanilla() {
        let mut registry = DataComponentRegistry::new();
        register_vanilla_data_components(&mut registry);

        let additional_trade_cost = registry
            .by_key(&ADDITIONAL_TRADE_COST.key)
            .expect("additional_trade_cost should be registered");
        let mut encoded = Vec::new();
        additional_trade_cost
            .write_network(&ComponentData::new(-7_i32), &mut encoded)
            .expect("additional_trade_cost should encode");
        assert_eq!(
            additional_trade_cost
                .read_network(&mut std::io::Cursor::new(encoded.as_slice()))
                .expect("additional_trade_cost should decode"),
            ComponentData::new(-7_i32)
        );

        let map_post_processing = registry
            .by_key(&MAP_POST_PROCESSING.key)
            .expect("map_post_processing should be registered");
        let mut encoded = Vec::new();
        map_post_processing
            .write_network(&ComponentData::new(MapPostProcessing::Scale), &mut encoded)
            .expect("map_post_processing should encode");
        assert_eq!(
            map_post_processing
                .read_network(&mut std::io::Cursor::new(encoded.as_slice()))
                .expect("map_post_processing should decode"),
            ComponentData::new(MapPostProcessing::Scale)
        );
    }

    #[test]
    fn identifier_component_codecs_use_vanilla_namespace_rules() {
        use steel_utils::codec::VarInt;
        use steel_utils::hash::HashComponent as _;
        use steel_utils::serial::PrefixedWrite as _;

        let mut registry = DataComponentRegistry::new();
        register_vanilla_data_components(&mut registry);
        let expected = Identifier::vanilla_static("stone");

        for component in [ITEM_MODEL, TOOLTIP_STYLE, NOTE_BLOCK_SOUND] {
            let entry = registry
                .by_key(&component.key)
                .unwrap_or_else(|| panic!("missing identifier component {}", component.key));
            assert!(entry.is_implemented(), "{}", component.key);
            let data = ComponentData::new(expected.clone());
            assert_eq!(
                entry.read_nbt_owned(&NbtTag::String("stone".into())),
                Some(ComponentData::new(expected.clone())),
                "{}",
                component.key
            );
            assert_eq!(
                entry
                    .write_nbt(&data)
                    .expect("persistent identifier should encode"),
                NbtTag::String("minecraft:stone".into()),
                "{}",
                component.key
            );

            let mut abbreviated = Vec::new();
            "stone"
                .write_prefixed::<VarInt>(&mut abbreviated)
                .expect("abbreviated identifier should encode");
            assert_eq!(
                entry
                    .read_network(&mut std::io::Cursor::new(abbreviated.as_slice()))
                    .expect("abbreviated identifier should decode"),
                ComponentData::new(expected.clone()),
                "{}",
                component.key
            );
            let mut encoded = Vec::new();
            entry
                .write_network(&data, &mut encoded)
                .expect("network identifier should encode");
            let mut canonical = Vec::new();
            "minecraft:stone"
                .write_prefixed::<VarInt>(&mut canonical)
                .expect("canonical identifier should encode");
            assert_eq!(encoded, canonical, "{}", component.key);

            assert_eq!(
                entry
                    .compute_hash(&data)
                    .expect("persistent identifier should hash"),
                expected.compute_hash(),
                "{}",
                component.key
            );
        }
    }

    #[test]
    fn persistent_scalar_codecs_coerce_numeric_tags_and_enforce_ranges() {
        let mut registry = DataComponentRegistry::new();
        register_vanilla_data_components(&mut registry);

        let max_stack_size = registry
            .by_key(&MAX_STACK_SIZE.key)
            .expect("max_stack_size should be registered");
        assert_eq!(
            max_stack_size.read_nbt_owned(&NbtTag::Double(16.9)),
            Some(ComponentData::new(16_i32))
        );
        assert_eq!(max_stack_size.read_nbt_owned(&NbtTag::Int(0)), None);

        let minimum_attack_charge = registry
            .by_key(&MINIMUM_ATTACK_CHARGE.key)
            .expect("minimum_attack_charge should be registered");
        assert_eq!(
            minimum_attack_charge.read_nbt_owned(&NbtTag::Double(0.5)),
            Some(ComponentData::new(0.5_f32))
        );
        assert_eq!(
            minimum_attack_charge.read_nbt_owned(&NbtTag::Double(1.5)),
            None
        );

        let glint = registry
            .by_key(&ENCHANTMENT_GLINT_OVERRIDE.key)
            .expect("enchantment_glint_override should be registered");
        assert_eq!(
            glint.read_nbt_owned(&NbtTag::Long(2)),
            Some(ComponentData::new(true))
        );
    }

    #[test]
    fn unit_component_persistence_requires_a_compound() {
        let mut registry = DataComponentRegistry::new();
        register_vanilla_data_components(&mut registry);
        let unbreakable = registry
            .by_key(&UNBREAKABLE.key)
            .expect("unbreakable should be registered");

        assert_eq!(
            unbreakable.read_nbt_owned(&NbtTag::Compound(NbtCompound::new())),
            Some(ComponentData::new(()))
        );
        assert_eq!(unbreakable.read_nbt_owned(&NbtTag::Byte(1)), None);
    }

    #[test]
    fn registry_validation_uses_concrete_downcast_keys() {
        let mut registry = DataComponentRegistry::new();
        register_vanilla_data_components(&mut registry);

        let max_stack_size = registry
            .by_key(&MAX_STACK_SIZE.key)
            .expect("max_stack_size should be registered");
        assert!(max_stack_size.validates(&ComponentData::new(16_i32)));
        assert!(!max_stack_size.validates(&ComponentData::new(16.0_f32)));

        let custom_data = registry
            .by_key(&CUSTOM_DATA.key)
            .expect("custom_data should be registered");
        assert!(custom_data.is_implemented());
        assert!(custom_data.validates(&ComponentData::new(CustomData::default())));
        assert!(!custom_data.validates(&ComponentData::new(())));

        let custom_model_data = registry
            .by_key(&CUSTOM_MODEL_DATA.key)
            .expect("custom_model_data should be registered");
        assert!(custom_model_data.is_implemented());
        assert!(custom_model_data.validates(&ComponentData::new(CustomModelData::EMPTY)));
        assert!(!custom_model_data.validates(&ComponentData::new(CustomData::default())));

        let enchantable = registry
            .by_key(&ENCHANTABLE.key)
            .expect("enchantable should be registered");
        assert!(enchantable.is_implemented());
        assert!(enchantable.validates(&ComponentData::new(
            Enchantable::new(15).expect("15 is positive")
        )));
        assert!(!enchantable.validates(&ComponentData::new(15_i32)));

        for component in [
            DYE,
            BASE_COLOR,
            WOLF_COLLAR,
            TROPICAL_FISH_BASE_COLOR,
            TROPICAL_FISH_PATTERN_COLOR,
            CAT_COLLAR,
            SHEEP_COLOR,
            SHULKER_COLOR,
        ] {
            let entry = registry
                .by_key(&component.key)
                .unwrap_or_else(|| panic!("missing dye color component {}", component.key));
            assert!(entry.is_implemented(), "{}", component.key);
            assert!(entry.validates(&ComponentData::new(DyeColor::Red)));
            assert!(!entry.validates(&ComponentData::new(14_i32)));
        }

        for (key, value) in [
            (
                &DYED_COLOR.key,
                ComponentData::new(DyedItemColor::new(0x123456)),
            ),
            (&MAP_COLOR.key, ComponentData::new(MapItemColor::DEFAULT)),
            (&MAP_ID.key, ComponentData::new(MapId::new(7))),
            (
                &OMINOUS_BOTTLE_AMPLIFIER.key,
                ComponentData::new(OminousBottleAmplifier::new(2)),
            ),
        ] {
            let entry = registry
                .by_key(key)
                .unwrap_or_else(|| panic!("missing component {key}"));
            assert!(entry.is_implemented(), "{key}");
            assert!(entry.validates(&value), "{key}");
            assert!(!entry.validates(&ComponentData::new(())), "{key}");
        }

        for (key, value) in [
            (&FOX_VARIANT.key, ComponentData::new(FoxVariant::Snow)),
            (&SALMON_SIZE.key, ComponentData::new(SalmonVariant::Large)),
            (&PARROT_VARIANT.key, ComponentData::new(ParrotVariant::Gray)),
            (
                &TROPICAL_FISH_PATTERN.key,
                ComponentData::new(TropicalFishPattern::Clayfish),
            ),
            (
                &MOOSHROOM_VARIANT.key,
                ComponentData::new(MooshroomVariant::Brown),
            ),
            (&RABBIT_VARIANT.key, ComponentData::new(RabbitVariant::Evil)),
            (
                &HORSE_VARIANT.key,
                ComponentData::new(HorseVariant::DarkBrown),
            ),
            (&LLAMA_VARIANT.key, ComponentData::new(LlamaVariant::Gray)),
            (
                &AXOLOTL_VARIANT.key,
                ComponentData::new(AxolotlVariant::Blue),
            ),
        ] {
            let entry = registry
                .by_key(key)
                .unwrap_or_else(|| panic!("missing variant component {key}"));
            assert!(entry.is_implemented(), "{key}");
            assert!(entry.validates(&value), "{key}");
            assert!(!entry.validates(&ComponentData::new(())), "{key}");
        }

        let food = registry
            .by_key(&FOOD.key)
            .expect("food should reserve its vanilla registry ID");
        assert!(!food.is_implemented());
        assert!(food.read_network(&mut std::io::Cursor::new(&[])).is_err());
    }
}
