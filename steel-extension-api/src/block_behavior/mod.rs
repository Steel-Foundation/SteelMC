//! Complete native block behavior contract, with explicit access to originals.
pub mod block_entity;
#[cfg(feature = "host")]
pub(crate) mod bridge;
pub mod capabilities;
pub(crate) mod capability_api;
#[cfg(feature = "host")]
pub(crate) mod capability_host;
mod contract;
#[cfg(feature = "host")]
pub(crate) mod core_types;
pub(crate) mod living_api;
mod results;
pub mod types;
mod value;
pub(crate) mod values_api;
use crate::{BlockPos, BlockState, World};
pub use results::{InteractionResult, MovementResult};
use stabby::option::Option as StableOption;
#[cfg(feature = "host")]
use std::sync::Arc;
use types::{
    Block, BlockEntity, BlockEntityBorrow, BlockEntityCreation, BlockEntityRef, BlockEntityTicker,
    BlockEntityType, BlockHit, BlockLootContext, BlockPlaceContext, BonemealableRef, BrushableData,
    ClipHit, CollisionBoxes, CollisionContext, Direction, DyeColor, Entity, EntityType, Explosion,
    ExplosionDropConsumer, FallDamage, FallDamageRef, FallOnContext, FallableRef, Fluid,
    FluidState, GameEventListener, GrowingPlantHeadRef, InsideBlockEffects, InteractionHand,
    Inventory, ItemStack, ItemStackRef, ItemStacks, LandingContext, LevelAccessor, LevelReader,
    PathComputationType, PickupResult, PlacementSource, Player, Projectile, RailRef,
    ScheduledTickAccess, SignalQueryContext, SupportType, UpdateFlags, Vector3, VoxelShape,
    WeakWorld,
};

include!("schema.rs");

pub use original::api::OriginalBlockBehavior;

impl Copy for OriginalBlockBehavior {}
#[expect(
    clippy::expl_impl_clone_on_copy,
    reason = "opaque storage is private, so Clone must copy the handle without accessing its fields"
)]
impl Clone for OriginalBlockBehavior {
    fn clone(&self) -> Self {
        *self
    }
}

#[cfg(feature = "host")]
const _: fn() = || {
    const fn require_shared<T: Send + Sync>() {}
    require_shared::<Arc<dyn core_types::BlockBehavior>>();
};
// SAFETY: this handle borrows a process-lifetime Arc<dyn BlockBehavior + Send + Sync>.
// The immutable host object and all native capability views remain pinned.
unsafe impl Send for OriginalBlockBehavior {}
// SAFETY: the captured behavior is thread-safe and host access is immutable.
unsafe impl Sync for OriginalBlockBehavior {}

/// Owning, ABI-checked extension behavior. The extension's destructor frees its own data.
pub type DynBlockBehavior = stabby::dynptr!(stabby::boxed::Box<dyn BlockBehavior + Send + Sync>);

#[cfg(all(test, feature = "host"))]
mod tests;
