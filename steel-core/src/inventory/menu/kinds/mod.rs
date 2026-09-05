//! Vanilla menu kind implementations.

mod anvil_menu;
mod basic_menu;
mod chest_menu;
mod crafting_menu;
mod ender_chest_menu;
mod inventory_menu;
mod shulker_box_menu;

pub use anvil_menu::{AnvilKind, anvil};
pub use basic_menu::BasicKind;
pub use chest_menu::{ChestKind, chest};
pub use crafting_menu::{CraftingKind, crafting};
pub use ender_chest_menu::{EnderChestKind, ender_chest};
pub use inventory_menu::{INVENTORY_MENU_CONTAINER_ID, InventoryKind, inventory_menu};
pub use shulker_box_menu::{ShulkerBoxKind, shulker_box};
