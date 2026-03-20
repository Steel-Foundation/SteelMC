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
pub mod fluid;
mod item;
pub mod items;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/blocks.rs"]
pub mod block_behaviors;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/items.rs"]
pub mod item_behaviors;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/waxables.rs"]
pub mod waxables;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/weathering.rs"]
pub mod weathering;

pub use block::{BlockBehavior, BlockBehaviorRegistry, DefaultBlockBehavior};
use block_behaviors::register_block_behaviors;
pub use context::{
    BlockHitResult, BlockPlaceContext, InteractionResult, UseItemContext, UseOnContext,
};
pub use fluid::{FLUID_BEHAVIORS, FluidBehaviorRegistry};
pub use item::{ItemBehavior, ItemBehaviorRegistry};
use item_behaviors::register_item_behaviors;
pub use items::{
    BlockItemBehavior, BucketItemBehavior, DefaultItemBehavior, DoubleHighBlockItemBehavior,
    EnderEyeBehavior, HangingSignItemBehavior, ShovelBehavior, SignItemBehavior,
    StandingAndWallBlockItem,
};
use std::ops::Deref;
use std::sync::OnceLock;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::fluid::FluidState;
use steel_registry::vanilla_fluids;
use steel_utils::BlockStateId;

use crate::fluid::{FluidBehavior, LavaFluid, WaterFluid};

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
    /// Delegates to the block's `BlockBehavior::get_fluid_state` implementation.
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

    let mut fluid_behaviors = FluidBehaviorRegistry::new();

    // Water: WaterFluid implements FluidBehavior directly
    let water_behavior: Box<dyn FluidBehavior> = Box::new(WaterFluid);
    // Both WATER and FLOWING_WATER share the same behavior
    fluid_behaviors.set_behavior(&vanilla_fluids::WATER, water_behavior);
    fluid_behaviors.set_behavior(&vanilla_fluids::FLOWING_WATER, Box::new(WaterFluid));

    // Lava: LavaFluid implements FluidBehavior directly
    let lava_behavior: Box<dyn FluidBehavior> = Box::new(LavaFluid);
    fluid_behaviors.set_behavior(&vanilla_fluids::LAVA, lava_behavior);
    fluid_behaviors.set_behavior(&vanilla_fluids::FLOWING_LAVA, Box::new(LavaFluid));

    assert!(
        FLUID_BEHAVIORS.0.set(fluid_behaviors).is_ok(),
        "Fluid behavior registry already initialized"
    );

    let mut item_behaviors = ItemBehaviorRegistry::new();
    register_item_behaviors(&mut item_behaviors);
    assert!(
        ITEM_BEHAVIORS.0.set(item_behaviors).is_ok(),
        "Item behavior registry already initialized"
    );
}
