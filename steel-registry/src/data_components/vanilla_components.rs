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
    AttackRange, DamageTypeComponent, Equippable, EquippableAllowedEntities,
    ItemAttributeModifierDisplay, ItemAttributeModifierEntry, ItemAttributeModifiers,
    ItemEnchantments, PiercingWeapon, Tool, ToolRule, ToolRuleBlocks, UseCooldown, Weapon,
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
    DataComponentType::new(Identifier::vanilla_static("damage"));

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

pub const CUSTOM_DATA: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("custom_data"));

pub const USE_EFFECTS: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("use_effects"));

pub const MINIMUM_ATTACK_CHARGE: DataComponentType<f32> =
    DataComponentType::new(Identifier::vanilla_static("minimum_attack_charge"));

pub const DAMAGE_TYPE: DataComponentType<DamageTypeComponent> =
    DataComponentType::new(Identifier::vanilla_static("damage_type"));

pub const ITEM_MODEL: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("item_model"));

pub const LORE: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("lore"));

pub const RARITY: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("rarity"));

pub const ENCHANTMENTS: DataComponentType<ItemEnchantments> =
    DataComponentType::new(Identifier::vanilla_static("enchantments"));

pub const CAN_PLACE_ON: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("can_place_on"));

pub const CAN_BREAK: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("can_break"));

pub const ATTRIBUTE_MODIFIERS: DataComponentType<ItemAttributeModifiers> =
    DataComponentType::new(Identifier::vanilla_static("attribute_modifiers"));

pub const CUSTOM_MODEL_DATA: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("custom_model_data"));

pub const TOOLTIP_DISPLAY: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("tooltip_display"));

pub const TOOLTIP_STYLE: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("tooltip_style"));

pub const NOTE_BLOCK_SOUND: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("note_block_sound"));

pub const FOOD: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("food"));

pub const CONSUMABLE: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("consumable"));

pub const USE_REMAINDER: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("use_remainder"));

pub const USE_COOLDOWN: DataComponentType<UseCooldown> =
    DataComponentType::new(Identifier::vanilla_static("use_cooldown"));

pub const DAMAGE_RESISTANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("damage_resistant"));

pub const ENCHANTABLE: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("enchantable"));

pub const REPAIRABLE: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("repairable"));

pub const DEATH_PROTECTION: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("death_protection"));

pub const BLOCKS_ATTACKS: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("blocks_attacks"));

pub const PIERCING_WEAPON: DataComponentType<PiercingWeapon> =
    DataComponentType::new(Identifier::vanilla_static("piercing_weapon"));

pub const KINETIC_WEAPON: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("kinetic_weapon"));

pub const SWING_ANIMATION: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("swing_animation"));

pub const ADDITIONAL_TRADE_COST: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("additional_trade_cost"));

pub const STORED_ENCHANTMENTS: DataComponentType<ItemEnchantments> =
    DataComponentType::new(Identifier::vanilla_static("stored_enchantments"));

pub const DYE: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("dye"));

pub const DYED_COLOR: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("dyed_color"));

pub const MAP_COLOR: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("map_color"));

pub const MAP_ID: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("map_id"));

pub const MAP_DECORATIONS: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("map_decorations"));

pub const MAP_POST_PROCESSING: DataComponentType<UnimplementedComponent> =
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

pub const BUCKET_ENTITY_DATA: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("bucket_entity_data"));

pub const BLOCK_ENTITY_DATA: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("block_entity_data"));

pub const INSTRUMENT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("instrument"));

pub const PROVIDES_TRIM_MATERIAL: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("provides_trim_material"));

pub const OMINOUS_BOTTLE_AMPLIFIER: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("ominous_bottle_amplifier"));

pub const JUKEBOX_PLAYABLE: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("jukebox_playable"));

pub const PROVIDES_BANNER_PATTERNS: DataComponentType<UnimplementedComponent> =
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

pub const BASE_COLOR: DataComponentType<UnimplementedComponent> =
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

pub const BREAK_SOUND: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("break_sound"));

// Entity variant components
pub const VILLAGER_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("villager/variant"));

pub const WOLF_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("wolf/variant"));

pub const WOLF_SOUND_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("wolf/sound_variant"));

pub const WOLF_COLLAR: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("wolf/collar"));

pub const FOX_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("fox/variant"));

pub const SALMON_SIZE: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("salmon/size"));

pub const PARROT_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("parrot/variant"));

pub const TROPICAL_FISH_PATTERN: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("tropical_fish/pattern"));

pub const TROPICAL_FISH_BASE_COLOR: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("tropical_fish/base_color"));

pub const TROPICAL_FISH_PATTERN_COLOR: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("tropical_fish/pattern_color"));

pub const MOOSHROOM_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("mooshroom/variant"));

pub const RABBIT_VARIANT: DataComponentType<UnimplementedComponent> =
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

pub const HORSE_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("horse/variant"));

pub const PAINTING_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("painting/variant"));

pub const LLAMA_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("llama/variant"));

pub const AXOLOTL_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("axolotl/variant"));

pub const CAT_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("cat/variant"));

pub const CAT_SOUND_VARIANT: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("cat/sound_variant"));

pub const CAT_COLLAR: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("cat/collar"));

pub const SHEEP_COLOR: DataComponentType<UnimplementedComponent> =
    DataComponentType::new(Identifier::vanilla_static("sheep/color"));

pub const SHULKER_COLOR: DataComponentType<UnimplementedComponent> =
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

fn i32_nbt_writer(data: &ComponentData) -> simdnbt::owned::NbtTag {
    let Some(value) = data.downcast_ref::<i32>() else {
        panic!("validated i32 component failed to downcast");
    };
    simdnbt::owned::NbtTag::Int(*value)
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

fn f32_nbt_writer(data: &ComponentData) -> simdnbt::owned::NbtTag {
    let Some(value) = data.downcast_ref::<f32>() else {
        panic!("validated f32 component failed to downcast");
    };
    simdnbt::owned::NbtTag::Float(*value)
}

fn bool_nbt_reader(tag: simdnbt::borrow::NbtTag) -> Option<ComponentData> {
    tag.codec_bool().map(ComponentData::new)
}

fn bool_nbt_writer(data: &ComponentData) -> simdnbt::owned::NbtTag {
    let Some(value) = data.downcast_ref::<bool>() else {
        panic!("validated bool component failed to downcast");
    };
    simdnbt::owned::NbtTag::Byte(i8::from(*value))
}

fn unit_nbt_reader(tag: simdnbt::borrow::NbtTag) -> Option<ComponentData> {
    tag.compound().map(|_| ComponentData::new(()))
}

fn unit_nbt_writer(_data: &ComponentData) -> simdnbt::owned::NbtTag {
    simdnbt::owned::NbtTag::Compound(simdnbt::owned::NbtCompound::new())
}

macro_rules! register_ranged_i32 {
    ($registry:expr, $component:expr, $min:expr, $max:expr) => {
        $registry.register_with_codecs(
            $component,
            varint_reader,
            varint_writer,
            ranged_i32_nbt_reader::<{ $min }, { $max }>,
            i32_nbt_writer,
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
    registry.register_unimplemented(CUSTOM_DATA, true);
    // 1: max_stack_size
    register_ranged_i32!(registry, MAX_STACK_SIZE, 1, 99);
    // 2: max_damage
    register_ranged_i32!(registry, MAX_DAMAGE, 1, i32::MAX);
    // 3: damage
    register_ranged_i32!(registry, DAMAGE, 0, i32::MAX);
    // 4: unbreakable
    register_unit!(registry, UNBREAKABLE);
    // 5: use_effects
    registry.register_unimplemented(USE_EFFECTS, true);
    // 6: custom_name
    registry.register(CUSTOM_NAME);
    // 7: minimum_attack_charge
    registry.register_with_codecs(
        MINIMUM_ATTACK_CHARGE,
        float_reader,
        float_writer,
        minimum_attack_charge_nbt_reader,
        f32_nbt_writer,
    );
    // 8: damage_type
    registry.register(DAMAGE_TYPE);
    // 9: item_name
    registry.register(ITEM_NAME);
    // 10: item_model
    registry.register_unimplemented(ITEM_MODEL, true);
    // 11: lore
    registry.register_unimplemented(LORE, true);
    // 12: rarity
    registry.register_unimplemented(RARITY, true);
    // 13: enchantments
    registry.register(ENCHANTMENTS);
    // 14: can_place_on
    registry.register_unimplemented(CAN_PLACE_ON, true);
    // 15: can_break
    registry.register_unimplemented(CAN_BREAK, true);
    // 16: attribute_modifiers
    registry.register(ATTRIBUTE_MODIFIERS);
    // 17: custom_model_data
    registry.register_unimplemented(CUSTOM_MODEL_DATA, true);
    // 18: tooltip_display
    registry.register_unimplemented(TOOLTIP_DISPLAY, true);
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
    registry.register_unimplemented(DAMAGE_RESISTANT, true);
    // 28: tool
    registry.register(TOOL);
    // 29: weapon
    registry.register(WEAPON);
    // 30: attack_range
    registry.register(ATTACK_RANGE);
    // 31: enchantable
    registry.register_unimplemented(ENCHANTABLE, true);
    // 32: equippable
    registry.register(EQUIPPABLE);
    // 33: repairable
    registry.register_unimplemented(REPAIRABLE, true);
    // 34: glider
    register_unit!(registry, GLIDER);
    // 35: tooltip_style
    registry.register_unimplemented(TOOLTIP_STYLE, true);
    // 36: death_protection
    registry.register_unimplemented(DEATH_PROTECTION, true);
    // 37: blocks_attacks
    registry.register_unimplemented(BLOCKS_ATTACKS, true);
    // 38: piercing_weapon
    registry.register(PIERCING_WEAPON);
    // 39: kinetic_weapon
    registry.register_unimplemented(KINETIC_WEAPON, true);
    // 40: swing_animation
    registry.register_unimplemented(SWING_ANIMATION, true);
    // 41: additional_trade_cost
    registry.register_unimplemented(ADDITIONAL_TRADE_COST, false);
    // 42: stored_enchantments
    registry.register(STORED_ENCHANTMENTS);
    // 43: dye
    registry.register_unimplemented(DYE, true);
    // 44: dyed_color
    registry.register_unimplemented(DYED_COLOR, true);
    // 45: map_color
    registry.register_unimplemented(MAP_COLOR, true);
    // 46: map_id
    registry.register_unimplemented(MAP_ID, true);
    // 47: map_decorations
    registry.register_unimplemented(MAP_DECORATIONS, true);
    // 48: map_post_processing
    registry.register_unimplemented(MAP_POST_PROCESSING, false);
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
        f32_nbt_writer,
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
    registry.register_unimplemented(BUCKET_ENTITY_DATA, true);
    // 60: block_entity_data
    registry.register_unimplemented(BLOCK_ENTITY_DATA, true);
    // 61: instrument
    registry.register_unimplemented(INSTRUMENT, true);
    // 62: provides_trim_material
    registry.register_unimplemented(PROVIDES_TRIM_MATERIAL, true);
    // 63: ominous_bottle_amplifier
    registry.register_unimplemented(OMINOUS_BOTTLE_AMPLIFIER, true);
    // 64: jukebox_playable
    registry.register_unimplemented(JUKEBOX_PLAYABLE, true);
    // 65: provides_banner_patterns
    registry.register_unimplemented(PROVIDES_BANNER_PATTERNS, true);
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
    registry.register_unimplemented(NOTE_BLOCK_SOUND, true);
    // 72: banner_patterns
    registry.register_unimplemented(BANNER_PATTERNS, true);
    // 73: base_color
    registry.register_unimplemented(BASE_COLOR, true);
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
    registry.register_unimplemented(BREAK_SOUND, true);
    // 82: villager/variant
    registry.register_unimplemented(VILLAGER_VARIANT, true);
    // 83: wolf/variant
    registry.register_unimplemented(WOLF_VARIANT, true);
    // 84: wolf/sound_variant
    registry.register_unimplemented(WOLF_SOUND_VARIANT, true);
    // 85: wolf/collar
    registry.register_unimplemented(WOLF_COLLAR, true);
    // 86: fox/variant
    registry.register_unimplemented(FOX_VARIANT, true);
    // 87: salmon/size
    registry.register_unimplemented(SALMON_SIZE, true);
    // 88: parrot/variant
    registry.register_unimplemented(PARROT_VARIANT, true);
    // 89: tropical_fish/pattern
    registry.register_unimplemented(TROPICAL_FISH_PATTERN, true);
    // 90: tropical_fish/base_color
    registry.register_unimplemented(TROPICAL_FISH_BASE_COLOR, true);
    // 91: tropical_fish/pattern_color
    registry.register_unimplemented(TROPICAL_FISH_PATTERN_COLOR, true);
    // 92: mooshroom/variant
    registry.register_unimplemented(MOOSHROOM_VARIANT, true);
    // 93: rabbit/variant
    registry.register_unimplemented(RABBIT_VARIANT, true);
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
    registry.register_unimplemented(HORSE_VARIANT, true);
    // 103: painting/variant
    registry.register_unimplemented(PAINTING_VARIANT, true);
    // 104: llama/variant
    registry.register_unimplemented(LLAMA_VARIANT, true);
    // 105: axolotl/variant
    registry.register_unimplemented(AXOLOTL_VARIANT, true);
    // 106: cat/variant
    registry.register_unimplemented(CAT_VARIANT, true);
    // 107: cat/sound_variant
    registry.register_unimplemented(CAT_SOUND_VARIANT, true);
    // 108: cat/collar
    registry.register_unimplemented(CAT_COLLAR, true);
    // 109: sheep/color
    registry.register_unimplemented(SHEEP_COLOR, true);
    // 110: shulker/color
    registry.register_unimplemented(SHULKER_COLOR, true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RegistryExt;
    use simdnbt::owned::{NbtCompound, NbtTag};

    #[test]
    fn sulfur_cube_content_keeps_vanilla_26_2_component_order() {
        let mut registry = DataComponentRegistry::new();
        register_vanilla_data_components(&mut registry);

        assert_eq!(registry.get_key_by_id(77), Some(&BEES.key));
        assert_eq!(registry.get_key_by_id(78), Some(&SULFUR_CUBE_CONTENT.key));
        assert_eq!(registry.get_key_by_id(79), Some(&LOCK.key));
        assert_eq!(registry.get_key_by_id(80), Some(&CONTAINER_LOOT.key));
        assert_eq!(registry.get_key_by_id(81), Some(&BREAK_SOUND.key));
        assert_eq!(registry.get_key_by_id(82), Some(&VILLAGER_VARIANT.key));
    }

    #[test]
    fn vanilla_transient_components_are_marked_non_persistent() {
        let mut registry = DataComponentRegistry::new();
        register_vanilla_data_components(&mut registry);

        for key in [
            &CREATIVE_SLOT_LOCK.key,
            &ADDITIONAL_TRADE_COST.key,
            &MAP_POST_PROCESSING.key,
        ] {
            assert!(
                registry
                    .by_key(key)
                    .is_some_and(|entry| !entry.is_persistent())
            );
        }
        assert!(matches!(
            registry.by_key(&MAX_STACK_SIZE.key),
            Some(entry) if entry.is_persistent()
        ));
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
            .expect("custom_data should reserve its vanilla registry ID");
        assert!(!custom_data.is_implemented());
        assert!(!custom_data.validates(&ComponentData::new(())));
        assert!(
            custom_data
                .read_network(&mut std::io::Cursor::new(&[]))
                .is_err()
        );
    }
}
