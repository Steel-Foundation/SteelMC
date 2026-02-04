//! ABI-stable component data storage.
//!
//! This module provides the core types for storing component values in an ABI-stable way.
//! Vanilla components get dedicated enum variants for zero-cost access, while plugin
//! components use the `Other` variant with opaque bytes.
use super::components::{Equippable, Tool};
use text_components::TextComponent;

/// Discriminant for [`ComponentData`] variants.
///
/// Used for runtime type validation to ensure plugins don't
/// set wrong types on vanilla components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentDataDiscriminant {
    Empty,
    Bool,
    I32,
    Float,
    Tool,
    Equippable,
    TextComponent,
    Todo,
    Other,
}

/// ABI-stable component value storage.
///
/// Each vanilla component type gets its own variant for type-safe, zero-cost access.
/// Plugin-defined components use the `Other` variant with serialized bytes that
/// the plugin is responsible for interpreting.
///
/// # Example (vanilla code)
/// ```ignore
/// let data = ComponentData::I32(10);
/// if let ComponentData::I32(d) = data {
///     println!("Value: {}", d);
/// }
/// ```
///
/// # Example (plugin code)
/// ```ignore
/// // Plugin stores its own serialized data
/// let my_bytes = my_energy.serialize();
/// let data = ComponentData::Other(my_bytes);
///
/// // Plugin retrieves and deserializes
/// if let ComponentData::Other(bytes) = data {
///     let energy = MyEnergy::deserialize(&bytes)?;
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ComponentData {
    // ==================== Unit types (marker components) ====================
    /// Component with no data (e.g., Unbreakable, Glider, CreativeSlotLock)
    Empty,

    // ==================== Primitives ====================
    /// Boolean component (e.g., EnchantmentGlintOverride)
    Bool(bool),
    /// i32 component (e.g., MaxStackSize, MaxDamage, Damage, RepairCost)
    /// Stored as VarInt on network.
    I32(i32),
    /// Float component (e.g., PotionDurationScale)
    Float(f32),

    // ==================== Complex structured components ====================
    /// minecraft:tool
    Tool(Tool),
    /// minecraft:equippable
    Equippable(Equippable),
    /// TextComponent component (e.g., CustomName, ItemName)
    TextComponent(Box<TextComponent>),

    // ==================== Not yet implemented ====================
    /// Placeholder for components that aren't implemented yet.
    Todo,

    // ==================== Plugin fallback ====================
    /// Opaque bytes for plugin-defined components.
    /// The plugin is responsible for serialization/deserialization.
    Other(Vec<u8>),
}

impl ComponentData {
    /// Returns true if this is the empty/unit variant.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    /// Returns the raw bytes if this is an `Other` variant.
    #[must_use]
    pub fn as_other(&self) -> Option<&[u8]> {
        match self {
            Self::Other(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// Returns the discriminant of this component data variant.
    /// Used for runtime type validation.
    #[must_use]
    pub const fn discriminant(&self) -> ComponentDataDiscriminant {
        match self {
            Self::Empty => ComponentDataDiscriminant::Empty,
            Self::Bool(_) => ComponentDataDiscriminant::Bool,
            Self::I32(_) => ComponentDataDiscriminant::I32,
            Self::Float(_) => ComponentDataDiscriminant::Float,
            Self::Tool(_) => ComponentDataDiscriminant::Tool,
            Self::Equippable(_) => ComponentDataDiscriminant::Equippable,
            Self::TextComponent(_) => ComponentDataDiscriminant::TextComponent,
            Self::Todo => ComponentDataDiscriminant::Todo,
            Self::Other(_) => ComponentDataDiscriminant::Other,
        }
    }

    /// Computes a hash of this component value for validation.
    ///
    /// Uses CRC32C hashing matching Minecraft's `HashOps` implementation.
    #[must_use]
    pub fn compute_hash(&self) -> i32 {
        use steel_utils::hash::{ComponentHasher, HashComponent};

        let mut hasher = ComponentHasher::new();

        match self {
            // Primitives
            Self::Empty => hasher.put_empty(),
            Self::Bool(v) => v.hash_component(&mut hasher),
            Self::I32(v) => v.hash_component(&mut hasher),
            Self::Float(v) => v.hash_component(&mut hasher),

            // Complex types
            Self::Tool(v) => v.hash_component(&mut hasher),
            Self::Equippable(v) => v.hash_component(&mut hasher),
            Self::TextComponent(v) => v.hash_component(&mut hasher),

            // Stub/plugin types - hash as empty map for now
            // TODO: Implement proper hashing when these types are implemented
            Self::Todo | Self::Other(_) => {
                hasher.start_map();
                hasher.end_map();
            }
        }

        hasher.finish()
    }
}

/// Trait for types that can be converted to/from [`ComponentData`].
///
/// This provides compile-time type safety for vanilla components while
/// the actual storage uses the ABI-stable `ComponentData` enum.
///
/// # Example
/// ```ignore
/// impl Component for Damage {
///     fn into_data(self) -> ComponentData {
///         ComponentData::Damage(self)
///     }
///
///     fn from_data(data: ComponentData) -> Option<Self> {
///         match data {
///             ComponentData::Damage(d) => Some(d),
///             _ => None,
///         }
///     }
/// }
/// ```
pub trait Component: Sized + Clone {
    /// Converts this component value into `ComponentData`.
    fn into_data(self) -> ComponentData;

    /// Attempts to extract this component type from `ComponentData`.
    /// Returns `None` if the data is a different variant.
    fn from_data(data: ComponentData) -> Option<Self>;

    /// Attempts to get a reference to this component type from `ComponentData`.
    /// Returns `None` if the data is a different variant or if the type
    /// cannot be referenced directly (e.g., needs conversion).
    fn from_data_ref(data: &ComponentData) -> Option<&Self>;
}

// ==================== Component implementations ====================

// Unit type for marker components
impl Component for () {
    fn into_data(self) -> ComponentData {
        ComponentData::Empty
    }

    fn from_data(data: ComponentData) -> Option<Self> {
        match data {
            ComponentData::Empty => Some(()),
            _ => None,
        }
    }

    fn from_data_ref(data: &ComponentData) -> Option<&Self> {
        match data {
            ComponentData::Empty => Some(&()),
            _ => None,
        }
    }
}

impl Component for bool {
    fn into_data(self) -> ComponentData {
        ComponentData::Bool(self)
    }

    fn from_data(data: ComponentData) -> Option<Self> {
        match data {
            ComponentData::Bool(v) => Some(v),
            _ => None,
        }
    }

    fn from_data_ref(data: &ComponentData) -> Option<&Self> {
        match data {
            ComponentData::Bool(v) => Some(v),
            _ => None,
        }
    }
}

impl Component for i32 {
    fn into_data(self) -> ComponentData {
        ComponentData::I32(self)
    }

    fn from_data(data: ComponentData) -> Option<Self> {
        match data {
            ComponentData::I32(v) => Some(v),
            _ => None,
        }
    }

    fn from_data_ref(data: &ComponentData) -> Option<&Self> {
        match data {
            ComponentData::I32(v) => Some(v),
            _ => None,
        }
    }
}

impl Component for f32 {
    fn into_data(self) -> ComponentData {
        ComponentData::Float(self)
    }

    fn from_data(data: ComponentData) -> Option<Self> {
        match data {
            ComponentData::Float(v) => Some(v),
            _ => None,
        }
    }

    fn from_data_ref(data: &ComponentData) -> Option<&Self> {
        match data {
            ComponentData::Float(v) => Some(v),
            _ => None,
        }
    }
}

impl Component for Tool {
    fn into_data(self) -> ComponentData {
        ComponentData::Tool(self)
    }

    fn from_data(data: ComponentData) -> Option<Self> {
        match data {
            ComponentData::Tool(v) => Some(v),
            _ => None,
        }
    }

    fn from_data_ref(data: &ComponentData) -> Option<&Self> {
        match data {
            ComponentData::Tool(v) => Some(v),
            _ => None,
        }
    }
}

impl Component for Equippable {
    fn into_data(self) -> ComponentData {
        ComponentData::Equippable(self)
    }

    fn from_data(data: ComponentData) -> Option<Self> {
        match data {
            ComponentData::Equippable(v) => Some(v),
            _ => None,
        }
    }

    fn from_data_ref(data: &ComponentData) -> Option<&Self> {
        match data {
            ComponentData::Equippable(v) => Some(v),
            _ => None,
        }
    }
}

impl Component for TextComponent {
    fn into_data(self) -> ComponentData {
        ComponentData::TextComponent(Box::new(self))
    }

    fn from_data(data: ComponentData) -> Option<Self> {
        match data {
            ComponentData::TextComponent(v) => Some(*v),
            _ => None,
        }
    }

    fn from_data_ref(data: &ComponentData) -> Option<&Self> {
        match data {
            ComponentData::TextComponent(v) => Some(v),
            _ => None,
        }
    }
}

// TextComponent and Identifier need special handling since they're used
// for multiple component types. We'll handle these through the DataComponentType
// registration rather than a blanket Component impl.
