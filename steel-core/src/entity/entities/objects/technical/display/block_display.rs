//! Vanilla's block display implementation.

use crate::block_entity::block_state_nbt;
use crate::entity::damage::DamageSource;
use crate::entity::entities::objects::technical::display::{
    Display, DisplayView, PrivateDisplayView,
};
use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntitySyncedData};
use crate::world::World;
use glam::DVec3;
use parking_lot::MutexGuard;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use std::sync::Weak;
use steel_macros::entity_behavior;
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_entity_data::{BlockDisplayEntityData, DisplayEntityData};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockStateId, DowncastType, DowncastTypeKey};

/// The Vanilla block display entity.
///
/// In addition to having the common display entity properties, this entity
/// also stores a [`BlockStateId`] to render as.
///
/// Like any display entity, to **access** or **modify** the data of a block display,
/// you will need to use [`Display::with_view`]. This method takes a function with a
/// [`BlockDisplayView`] as a parameter, which can be used within the function.
#[entity_behavior(class = "BlockDisplay")]
pub struct BlockDisplayEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<BlockDisplayEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `BlockDisplayEntity`.
unsafe impl DowncastType for BlockDisplayEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/block_display");
}

impl BlockDisplayEntity {
    /// Creates a new block display entity.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Creates a block display entity from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::new_with_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    #[must_use]
    fn new_with_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        Self {
            base,
            entity_type,
            entity_data: SyncMutex::new(BlockDisplayEntityData::new()),
        }
    }
}

impl Entity for BlockDisplayEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn tick(&self) {
        self.tick_display();
    }
    fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        self.hurt_display(world, source, amount)
    }
    fn piston_push_reaction(&self) -> PushReaction {
        self.piston_push_reaction_display()
    }
    fn is_ignoring_block_triggers(&self) -> bool {
        self.is_ignoring_block_triggers_display()
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.with_view(|view| {
            <Self as Display>::save_display(&view, nbt);
            nbt.insert("block_state", block_state_nbt::save(view.block_state()));
        });
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.with_view(|mut view| {
            <Self as Display>::load_display(&mut view, nbt);

            view.set_block_state(
                nbt.compound("block_state")
                    .and_then(block_state_nbt::load)
                    .unwrap_or(vanilla_blocks::AIR.default_state()),
            );
        });
    }
}

impl Display for BlockDisplayEntity {
    type View<'a> = BlockDisplayView<'a>;

    fn with_view(&self, f: impl FnOnce(Self::View<'_>)) {
        f(BlockDisplayView(self.entity_data.lock()));
    }
}

/// A view to the data of a block display.
///
/// Along with having the methods in [`DisplayView`], this view also has additional methods
/// to access and manipulate the block state shown by the block display.
pub struct BlockDisplayView<'a>(MutexGuard<'a, BlockDisplayEntityData>);

impl<'a> PrivateDisplayView<'a> for BlockDisplayView<'a> {
    fn display_data(&self) -> &DisplayEntityData {
        self.0.display()
    }

    fn display_data_mut(&mut self) -> &mut DisplayEntityData {
        self.0.display_mut()
    }
}

impl<'a> DisplayView<'a> for BlockDisplayView<'a> {}

impl BlockDisplayView<'_> {
    /// Gets the block state (by ID) of this block display.
    #[must_use]
    pub fn block_state(&self) -> BlockStateId {
        *self.0.block_state.get()
    }

    /// Sets the block state (by ID) of this block display.
    pub fn set_block_state(&mut self, id: BlockStateId) {
        self.0.block_state.set(id);
    }
}
