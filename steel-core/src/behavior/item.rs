//! Item behavior trait and registry.

use steel_registry::REGISTRY;
use steel_registry::items::ItemRef;

use crate::behavior::items::DefaultItemBehavior;
use crate::behavior::{InteractionResult, UseItemContext, UseOnContext};

/// Trait defining the behavior of an item.
///
/// This trait handles dynamic/functional aspects of items:
/// - Use on blocks (placing, interacting)
/// - Use in air
/// - etc.
pub trait ItemBehavior: Send + Sync {
    /// Called when this item is used on a block.
    fn use_on(&self, _context: &mut UseOnContext) -> InteractionResult {
        InteractionResult::Pass
    }

    /// Called when this item is used (e.g. right click in air).
    fn use_item(&self, _context: &mut UseItemContext) -> InteractionResult {
        InteractionResult::Pass
    }
}

/// Registry for item behaviors.
///
/// Created after the main registry is frozen. Block items get `BlockItemBehavior`,
/// other items get `DefaultItemBehavior`. Custom behaviors can be registered.
pub struct ItemBehaviorRegistry {
    behaviors: Vec<Box<dyn ItemBehavior>>,
}

impl ItemBehaviorRegistry {
    /// Creates a new behavior registry with default behaviors for all items.
    ///
    /// Call `register_item_behaviors()` after this to set up proper behaviors.
    #[must_use]
    pub fn new() -> Self {
        let item_count = REGISTRY.items.len();
        let behaviors = (0..item_count)
            .map(|_| Box::new(DefaultItemBehavior) as Box<dyn ItemBehavior>)
            .collect();

        Self { behaviors }
    }

    /// Sets a custom behavior for an item.
    pub fn set_behavior(&mut self, item: ItemRef, behavior: Box<dyn ItemBehavior>) {
        let id = *REGISTRY.items.get_id(item);
        self.behaviors[id] = behavior;
    }

    /// Gets the behavior for an item.
    #[must_use]
    pub fn get_behavior(&self, item: ItemRef) -> &dyn ItemBehavior {
        let id = *REGISTRY.items.get_id(item);
        self.behaviors[id].as_ref()
    }

    /// Gets the behavior for an item by its ID.
    #[must_use]
    pub fn get_behavior_by_id(&self, id: usize) -> Option<&dyn ItemBehavior> {
        self.behaviors.get(id).map(AsRef::as_ref)
    }
}

impl Default for ItemBehaviorRegistry {
    fn default() -> Self {
        Self::new()
    }
}
