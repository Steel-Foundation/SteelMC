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
#[path = "generated/candle_cakes.rs"]
pub mod candle_cakes;

#[allow(warnings)]
#[rustfmt::skip]
#[path = "generated/items.rs"]
pub mod item_behaviors;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/strippables.rs"]
pub mod strippables;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/waxables.rs"]
pub mod waxables;

#[expect(warnings)]
#[rustfmt::skip]
#[path = "generated/weathering.rs"]
pub mod weathering;

pub(crate) use block::pickup_waterlogged_block;
pub use block::{
    BlockBehavior, BlockBehaviorRegistry, BlockCollisionContext, DefaultBlockBehavior,
    EntityFallDamage, EntityFallOnContext, EntityFallOnFacts, EntityLandingContext,
};
use block_behaviors::register_block_behaviors;
pub use context::{
    BlockHitResult, BlockPlaceContext, InteractionResult, InventoryAccess, UseItemContext,
    UseOnContext,
};
pub use fluid::{FLUID_BEHAVIORS, FluidBehaviorRegistry};
pub use item::{ItemBehavior, ItemBehaviorRegistry};
use item_behaviors::register_item_behaviors;
pub use items::{
    BlockItem, BucketItem, DefaultItemBehavior, DoubleHighBlockItem, EnderEyeItem, HangingSignItem,
    ShovelItem, SignItem, StandingAndWallBlockItem,
};
use std::ops::Deref;
use std::sync::OnceLock;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::fluid::FluidState;
use steel_registry::vanilla_fluids;
use steel_utils::BlockStateId;

use crate::entity::ai::path::PathComputationType;
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

    /// Returns true when vanilla `BlockState.getFluidState()` is non-empty.
    fn has_fluid(&self) -> bool;

    /// Returns whether this block state belongs to a vanilla `LiquidBlockContainer`.
    fn is_liquid_container(&self) -> bool;

    /// Returns whether this block state can be replaced by the given fluid block.
    fn can_be_replaced_by_fluid(&self, fluid_block: BlockRef) -> bool;

    /// Returns whether this block state is pathfindable for the supplied vanilla computation type.
    fn is_pathfindable(&self, computation_type: PathComputationType) -> bool;
}

impl BlockStateBehaviorExt for BlockStateId {
    fn get_fluid_state(&self) -> FluidState {
        let block = self.get_block();
        let behavior = BLOCK_BEHAVIORS.get_behavior(block);
        behavior.get_fluid_state(*self)
    }

    fn has_fluid(&self) -> bool {
        !self.get_fluid_state().is_empty()
    }

    fn is_liquid_container(&self) -> bool {
        let block = self.get_block();
        let behavior = BLOCK_BEHAVIORS.get_behavior(block);
        behavior.is_liquid_container(*self)
    }

    fn can_be_replaced_by_fluid(&self, fluid_block: BlockRef) -> bool {
        let block = self.get_block();
        let behavior = BLOCK_BEHAVIORS.get_behavior(block);
        behavior.can_be_replaced_by_fluid(*self, fluid_block)
    }

    fn is_pathfindable(&self, computation_type: PathComputationType) -> bool {
        let block = self.get_block();
        let behavior = BLOCK_BEHAVIORS.get_behavior(block);
        behavior.is_pathfindable(*self, computation_type)
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
/// This should be called after the main registry is frozen. Repeated calls are a no-op.
pub fn init_behaviors() {
    BLOCK_BEHAVIORS.0.get_or_init(|| {
        let mut block_behaviors = BlockBehaviorRegistry::new();
        register_block_behaviors(&mut block_behaviors);
        block_behaviors
    });

    FLUID_BEHAVIORS.0.get_or_init(|| {
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

        fluid_behaviors
    });

    ITEM_BEHAVIORS.0.get_or_init(|| {
        let mut item_behaviors = ItemBehaviorRegistry::new();
        register_item_behaviors(&mut item_behaviors);
        item_behaviors
    });
}
