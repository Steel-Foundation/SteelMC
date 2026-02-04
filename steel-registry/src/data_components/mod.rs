//! Data components system for items and entities.
//!
//! This module provides an ABI-stable component system where:
//! - Vanilla components get dedicated enum variants for zero-cost typed access
//! - Plugin components use opaque bytes (`ComponentData::Other`)
//!
//! # Architecture
//!
//! - [`ComponentData`] - ABI-stable enum storing component values
//! - [`Component`] - Trait for types that convert to/from `ComponentData`
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
pub use component_data::{Component, ComponentData, ComponentDataDiscriminant};
pub use components::{Equippable, EquippableSlot, Tool, ToolRule};
pub use registry::{
    ComponentEntry,
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
