pub use glam::DVec3;
pub use std::sync::{Arc, Weak};
pub use steel_core::{
    behavior::blocks::vegetation::{GrowingPlantHeadBehavior, bonemealable::Bonemealable},
    behavior::{
        BlockBehavior, BlockCollisionBoxes, BlockCollisionContext, BlockEntityCreation,
        BlockHitResult, BlockLootContext, BlockPlaceContext, BrushableData, EntityFallDamage,
        EntityFallOnContext, EntityLandingContext, Fallable, InteractionResult, InventoryAccess,
        PathComputationType, PickupResult, PlacementSource, RailBehavior,
    },
    block_entity::{BlockEntity, BlockEntityTicker, SharedBlockEntity},
    entity::{Entity, InsideBlockEffectCollector, projectile::Projectile},
    player::Player,
    world::{
        BlockInteraction, ClipHitResult, Explosion, LevelAccessor, LevelReader,
        ScheduledTickAccess, SignalQueryContext, World, game_event::SharedGameEventListener,
    },
};
pub use steel_registry::{
    DyeColor,
    block_entity_type::BlockEntityTypeRef,
    blocks::{
        BlockRef,
        properties::Direction,
        shapes::{SupportType, VoxelShape},
    },
    entity_type::EntityTypeRef,
    fluid::{FluidRef, FluidState},
    item_stack::ItemStack,
};
pub use steel_utils::{
    BlockPos, BlockStateId,
    types::{InteractionHand, UpdateFlags},
};

pub use rand::Rng;
pub use steel_core::behavior::blocks::vegetation::bonemealable::BonemealAction;
pub use steel_core::entity::{damage::DamageSource, entities::FallingBlockEntity};

pub use simdnbt::{borrow::BaseNbtCompound as BorrowedNbtCompound, owned::NbtCompound};
pub use steel_core::{block_entity::BlockEntityBase, inventory::lock::ContainerRef};

pub type ExplosionDropConsumer<'a> = dyn FnMut(ItemStack, BlockPos) + 'a;
