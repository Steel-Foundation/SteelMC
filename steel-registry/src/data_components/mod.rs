//! Data components system for items and entities.
//!
//! Component values are type-erased through Steel's deterministic keyed
//! downcasting system. Vanilla and plugin-defined values use the same storage
//! path while retaining type-safe access through [`DataComponentType`].
//!
//! # Architecture
//!
//! - [`ComponentData`] - Type-erased component value storage
//! - [`Component`] - Object-safe behavior for stored values
//! - [`DataComponentType<T>`] - Compile-time type handle for accessing components
//! - [`DataComponentMap`] - Storage for component values on items
//! - [`DataComponentPatch`] - Diff representation for network/storage
//! - [`DataComponentRegistry`] - Registry of component types with serialization
//!
//! # Example
//! ```ignore
//! use steel_registry::data_components::vanilla_components::DAMAGE;
//!
//! // Type-safe access (compile-time checked)
//! let damage: Option<&i32> = components.get(DAMAGE);
//! components.set(DAMAGE, Some(10));
//!
//! // Raw access for plugins
//! let data = components.get_raw(&key)?;
//! ```

mod component_data;
pub mod components;
mod registry;
pub mod vanilla_components;

// Re-export core types
pub use crate::item_predicate::{AdventureModePredicate, BlockPredicate, LockCode};
pub use component_data::{Component, ComponentData};
pub use components::{
    ArmorTrim, BannerPatternLayer, BannerPatternLayers, BlocksAttacks, BundleContents,
    ChargedProjectiles, Consumable, CustomData, CustomModelData, DeathProtection, DyedItemColor,
    Enchantable, Equippable, EquippableAllowedEntities, InstrumentComponent,
    InvalidEnchantableValue, ItemContainerContents, JukeboxPlayable, MapDecorationEntry,
    MapDecorations, MapId, MapItemColor, OminousBottleAmplifier, PaintingVariantComponent,
    PotDecorations, PotionContents, ProvidesBannerPatterns, ProvidesTrimMaterial, Recipes,
    SulfurCubeContent, Tool, ToolRule, ToolRuleBlocks, UseRemainder,
};
pub use registry::{
    ComponentEntry,
    ComponentEntryRef,
    ComponentPatchEntry,
    DataComponentMap,
    DataComponentPatch,
    DataComponentRegistry,
    DataComponentType,
    NbtReader,
    NbtWriter,
    // Type aliases for reader/writer functions
    NetworkReader,
    NetworkWriter,
    component_try_into,
};
