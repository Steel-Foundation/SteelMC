//! Equipment system for entities.
//!
//! This module provides the core equipment infrastructure:
//! - [`EquipmentSlot`] - Enum representing equipment slots (main hand, armor, etc.)
//! - [`EquipmentSlotType`] - Categories of equipment slots
//! - [`EntityEquipment`] - Storage for entity equipment with closure-based access

mod entity_equipment;

pub use entity_equipment::EntityEquipment;
pub use steel_registry::equipment::{EquipmentSlot, EquipmentSlotType};
