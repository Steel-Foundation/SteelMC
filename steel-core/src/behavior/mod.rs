//! Block and item behavior system.
//!
//! This module contains the behavior traits and registries that define how
//! blocks and items behave dynamically. This is separate from the static data
//! in steel-registry to maintain a clean separation between constant data and
//! functional/dynamic behavior.
//!
//! # Architecture
//!
//! After the main registry (`steel-registry`) is frozen, behavior registries
//! are created:
//! - `BlockBehaviorRegistry` - assigns default or custom behaviors to each block
//! - `ItemBehaviorRegistry` - assigns default or custom behaviors to each item
//!
//! # Usage
//!
//! ```ignore
//! use steel_core::behavior::{init_behaviors, BLOCK_BEHAVIORS, ITEM_BEHAVIORS};
//!
//! // After registry is frozen, call once at startup:
//! init_behaviors();
//!
//! // Then access behaviors via the global registries:
//! let behavior = BLOCK_BEHAVIORS.get_behavior(block);
//! ```

mod block;
pub mod blocks;
mod context;
mod item;
pub mod items;

#[allow(warnings)]
#[rustfmt::skip]
#[path = "generated/blocks.rs"]
pub mod block_behaviours;

#[allow(warnings)]
#[rustfmt::skip]
#[path = "generated/items.rs"]
pub mod item_behaviours;

pub use block::{BlockBehaviorRegistry, BlockBehaviour, DefaultBlockBehaviour};
use block_behaviours::register_block_behaviors;
pub use context::{BlockHitResult, BlockPlaceContext, InteractionResult, UseOnContext};
pub use item::{ItemBehavior, ItemBehaviorRegistry};
use item_behaviours::register_item_behaviors;
pub use items::{BlockItemBehavior, DefaultItemBehavior, EnderEyeBehavior, FilledBucketBehavior};
use std::ops::Deref;
use std::sync::OnceLock;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::fluid::FluidState;
use steel_registry::{vanilla_blocks, vanilla_items};
use steel_utils::BlockStateId;

/// Wrapper for the global block behavior registry that implements `Deref`.
pub struct BlockBehaviorLock(OnceLock<BlockBehaviorRegistry>);

impl Deref for BlockBehaviorLock {
    type Target = BlockBehaviorRegistry;

    fn deref(&self) -> &Self::Target {
        self.0.get().expect("Block behaviors not initialized")
    }
}

/// Wrapper for the global item behavior registry that implements `Deref`.
pub struct ItemBehaviorLock(OnceLock<ItemBehaviorRegistry>);

impl Deref for ItemBehaviorLock {
    type Target = ItemBehaviorRegistry;

    fn deref(&self) -> &Self::Target {
        self.0.get().expect("Item behaviors not initialized")
    }
}

/// Extension trait for `BlockStateId` that provides access to behavior-dependent methods.
///
/// This is separate from `BlockStateExt` (in steel-registry) because these methods
/// require access to the behavior registry which lives in steel-core.
pub trait BlockStateBehaviorExt {
    /// Returns the fluid state for this block state.
    ///
    /// Delegates to the block's `BlockBehaviour::get_fluid_state` implementation.
    fn get_fluid_state(&self) -> FluidState;
}

impl BlockStateBehaviorExt for BlockStateId {
    fn get_fluid_state(&self) -> FluidState {
        let block = self.get_block();
        let behavior = BLOCK_BEHAVIORS.get_behavior(block);
        behavior.get_fluid_state(*self)
    }
}

/// Global block behavior registry.
///
/// Access behaviors directly via deref: `BLOCK_BEHAVIORS.get_behavior(block)`
pub static BLOCK_BEHAVIORS: BlockBehaviorLock = BlockBehaviorLock(OnceLock::new());

/// Global item behavior registry.
///
/// Access behaviors directly via deref: `ITEM_BEHAVIORS.get_behavior(item)`
pub static ITEM_BEHAVIORS: ItemBehaviorLock = ItemBehaviorLock(OnceLock::new());

/// Initializes the global behavior registries.
///
/// This should be called once after the main registry is frozen.
///
/// # Panics
///
/// Panics if called more than once.
pub fn init_behaviors() {
    let mut block_behaviors = BlockBehaviorRegistry::new();
    register_block_behaviors(&mut block_behaviors);

    assert!(
        BLOCK_BEHAVIORS.0.set(block_behaviors).is_ok(),
        "Block behavior registry already initialized"
    );

    let mut item_behaviors = ItemBehaviorRegistry::new();
    register_item_behaviors(&mut item_behaviors);

    // Register bucket behaviors (not auto-generated since they're not block items)
    item_behaviors.set_behavior(
        &vanilla_items::ITEMS.water_bucket,
        Box::new(FilledBucketBehavior::new(
            vanilla_blocks::WATER,
            &vanilla_items::ITEMS.bucket,
        )),
    );
    item_behaviors.set_behavior(
        &vanilla_items::ITEMS.lava_bucket,
        Box::new(FilledBucketBehavior::new(
            vanilla_blocks::LAVA,
            &vanilla_items::ITEMS.bucket,
        )),
    );

    assert!(
        ITEM_BEHAVIORS.0.set(item_behaviors).is_ok(),
        "Item behavior registry already initialized"
    );
}
