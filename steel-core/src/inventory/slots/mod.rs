//! All the different types of Slots

mod anvil_slots;
mod armor_slot;
mod crafting_slots;
mod normal_slot;
mod restricted_slot;
mod result_handler;
mod result_slot;
pub mod slot;

pub use anvil_slots::*;
pub use armor_slot::ArmorSlot;
pub use crafting_slots::CraftingHandler;
pub use normal_slot::NormalSlot;
pub use restricted_slot::*;
pub use result_handler::ResultHandler;
pub use result_slot::*;
pub use slot::*;
