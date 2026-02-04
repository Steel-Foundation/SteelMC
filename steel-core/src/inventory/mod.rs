//! Inventory and container management system.
//!
//! This module provides the core inventory system including containers,
//! menus, crafting, equipment, and recipes.

pub mod chest_menu;
pub mod container;
pub mod crafting;
pub mod crafting_menu;
pub mod equipment;
pub mod inventory_menu;
pub mod lock;
pub mod menu;
pub mod menu_provider;
pub mod recipe_manager;
pub mod slot;

pub use chest_menu::{ChestMenu, ChestMenuProvider};
pub use crafting_menu::{CraftingMenu, CraftingMenuProvider};
pub use lock::SyncPlayerInv;
pub use menu_provider::{MenuInstance, MenuProvider};
