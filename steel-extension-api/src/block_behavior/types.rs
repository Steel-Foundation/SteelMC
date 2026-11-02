//! ABI-safe values used by block behavior methods.
//!
//! Borrowed payloads cannot escape their callback:
//!
//! ```compile_fail
//! use steel_extension_api::block_behavior::types::Inventory;
//! fn retain(value: Inventory<'_>) -> Inventory<'static> { value }
//! ```
//!
//! Exclusive host payloads cannot be sent to a worker:
//!
//! ```compile_fail
//! use steel_extension_api::block_behavior::types::Inventory;
//! fn send(value: Inventory<'_>) {
//!     std::thread::scope(|scope| { scope.spawn(move || drop(value)); });
//! }
//! ```
#[cfg(feature = "host")]
use super::core_types;
use super::value::host_value;
#[cfg(feature = "host")]
use std::sync::Weak;
host_value!(block_hit_storage, BlockHit<'a> = &'a core_types::BlockHitResult);
host_value!(block_loot_context_storage, BlockLootContext<'a> = &'a core_types::BlockLootContext<'a>);
host_value!(block_place_context_storage, BlockPlaceContext<'a> = &'a core_types::BlockPlaceContext<'a>);
host_value!(clip_hit_storage, ClipHit<'a> = &'a core_types::ClipHitResult);
host_value!(fall_damage_ref_storage, FallDamageRef<'a> = &'a core_types::EntityFallDamage);
host_value!(item_stack_ref_storage, ItemStackRef<'a> = &'a core_types::ItemStack);
host_value!(placement_source_storage, PlacementSource<'a> = &'a core_types::PlacementSource<'a>);
host_value!(player_storage, Player<'a> = &'a core_types::Player; read);
host_value!(block_entity_ref_storage, BlockEntityRef<'a> = &'a dyn core_types::BlockEntity; read);
host_value!(entity_storage, Entity<'a> = &'a dyn core_types::Entity; read);
host_value!(level_accessor_storage, LevelAccessor<'a> = &'a dyn core_types::LevelAccessor);
host_value!(level_reader_storage, LevelReader<'a> = &'a dyn core_types::LevelReader);
host_value!(explosion_storage, Explosion<'a> = &'a dyn core_types::Explosion; read);
host_value!(explosion_drop_consumer_storage, ExplosionDropConsumer<'a> = &'a mut core_types::ExplosionDropConsumer<'a>; reborrow);
host_value!(damage_source_ref_storage, DamageSourceRef<'a> = &'a core_types::DamageSource);
/// Whether explosion-affected blocks survive or contribute normal or decayed loot.
#[stabby::stabby]
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockInteraction {
    /// Preserve blocks.
    Keep,
    /// Destroy blocks without explosion loot decay.
    Destroy,
    /// Destroy blocks with the explosion radius in their loot context.
    DestroyWithDecay,
}
host_value!(projectile_storage, Projectile<'a> = &'a dyn core_types::Projectile);
host_value!(scheduled_tick_access_storage, ScheduledTickAccess<'a> = &'a dyn core_types::ScheduledTickAccess);
host_value!(inside_block_effects_storage, InsideBlockEffects<'a> = &'a mut core_types::InsideBlockEffectCollector);
host_value!(inventory_storage, Inventory<'a> = &'a mut core_types::InventoryAccess; read,reborrow);
host_value!(
    collision_boxes_storage,
    CollisionBoxes = core_types::BlockCollisionBoxes
);
host_value!(
    collision_context_storage,
    CollisionContext = core_types::BlockCollisionContext
);
host_value!(
    block_entity_creation_storage,
    BlockEntityCreation = core_types::BlockEntityCreation
);
host_value!(
    block_entity_type_storage,
    BlockEntityType = core_types::BlockEntityTypeRef
);
host_value!(block_storage, Block = core_types::BlockRef);
host_value!(vector3_storage, Vector3 = core_types::DVec3);
host_value!(direction_storage, Direction = core_types::Direction);
host_value!(fall_on_context_storage, FallOnContext<'a> = core_types::EntityFallOnContext<'a>);
host_value!(
    landing_context_storage,
    LandingContext = core_types::EntityLandingContext
);
host_value!(entity_type_storage, EntityType = core_types::EntityTypeRef);
host_value!(fluid_storage, Fluid = core_types::FluidRef);
host_value!(fluid_state_storage, FluidState = core_types::FluidState);
host_value!(
    interaction_hand_storage,
    InteractionHand = core_types::InteractionHand
);
host_value!(
    path_computation_type_storage,
    PathComputationType = core_types::PathComputationType
);
host_value!(
    signal_query_context_storage,
    SignalQueryContext = core_types::SignalQueryContext
);
host_value!(support_type_storage, SupportType = core_types::SupportType);
host_value!(update_flags_storage, UpdateFlags = core_types::UpdateFlags);
host_value!(voxel_shape_storage, VoxelShape = core_types::VoxelShape);
host_value!(weak_world_storage, WeakWorld = Weak<core_types::World>; read);
host_value!(block_entity_storage, BlockEntity = core_types::SharedBlockEntity; read);
host_value!(
    game_event_listener_storage,
    GameEventListener = core_types::SharedGameEventListener
);
host_value!(
    block_entity_ticker_storage,
    BlockEntityTicker = core_types::BlockEntityTicker
);
host_value!(
    brushable_data_storage,
    BrushableData = core_types::BrushableData
);
host_value!(dye_color_storage, DyeColor = core_types::DyeColor);
host_value!(
    fall_damage_storage,
    FallDamage = core_types::EntityFallDamage
);
host_value!(item_stack_storage, ItemStack = core_types::ItemStack);
host_value!(
    pickup_result_storage,
    PickupResult = core_types::PickupResult
);
host_value!(item_stacks_storage, ItemStacks = Vec<core_types::ItemStack>);
host_value!(block_entity_borrow_storage, BlockEntityBorrow<'a> = &'a core_types::SharedBlockEntity);
host_value!(bonemealable_ref_storage, BonemealableRef = &'static dyn core_types::Bonemealable; read);
host_value!(fallable_ref_storage, FallableRef = &'static dyn core_types::Fallable; read);
host_value!(growing_plant_head_ref_storage, GrowingPlantHeadRef = &'static dyn core_types::GrowingPlantHeadBehavior; read);
host_value!(rail_ref_storage, RailRef = &'static dyn core_types::RailBehavior; read);
host_value!(random_storage, Random<'a> = &'a mut dyn core_types::Rng);
host_value!(falling_block_entity_ref_storage, FallingBlockEntityRef<'a> = &'a core_types::FallingBlockEntity);
host_value!(shared_falling_block_entity_storage, SharedFallingBlockEntity<'a> = &'a core_types::Arc<core_types::FallingBlockEntity>);
host_value!(
    damage_source_storage,
    DamageSource = core_types::DamageSource
);
host_value!(
    bonemeal_action_storage,
    BonemealAction = core_types::BonemealAction
);
host_value!(borrowed_nbt_storage, BorrowedNbt<'a> = &'a core_types::BorrowedNbtCompound<'a>; read);
host_value!(nbt_mut_storage, NbtMut<'a> = &'a mut core_types::NbtCompound; reborrow);
host_value!(nbt_storage, Nbt = core_types::NbtCompound; write);
host_value!(container_storage, Container = core_types::ContainerRef);

#[cfg(feature = "host")]
const _: fn() = || {
    const fn require_shared<T: Send + Sync>() {}
    require_shared::<core_types::SharedBlockEntity>();
};
// SAFETY: this nominal handle owns an Arc<dyn BlockEntity + Send + Sync>.
// The host checks those bounds, and its paired allocator and destructor stay pinned.
unsafe impl Send for BlockEntity {}
// SAFETY: shared access only exposes the thread-safe Arc payload through host methods.
unsafe impl Sync for BlockEntity {}
