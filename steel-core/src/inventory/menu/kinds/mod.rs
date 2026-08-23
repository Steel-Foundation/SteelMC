//! Vanilla menu kind implementations.

mod anvil_menu;
mod basic_menu;
mod chest_menu;
mod crafting_menu;
mod inventory_menu;
mod merchant_menu;

pub use anvil_menu::{AnvilKind, anvil};
pub use basic_menu::BasicKind;
pub use chest_menu::{ChestKind, chest};
pub use crafting_menu::{CraftingKind, crafting};
pub use inventory_menu::{INVENTORY_MENU_CONTAINER_ID, InventoryKind, inventory_menu};
pub use merchant_menu::{MerchantKind, merchant};
