//! Vanilla's block display implementation.

use crate::block_entity::block_state_nbt;
use crate::entity::damage::DamageSource;
use crate::entity::entities::objects::display_ui::Display;
use crate::entity::entities::objects::display_ui::Transformation;
use crate::entity::entities::objects::display_ui::display::PrivateDisplay;
use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntitySyncedData};
use crate::world::World;
use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use std::sync::Weak;
use steel_macros::entity_behavior;
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_entity_data::BlockDisplayEntityData;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockStateId, DowncastType, DowncastTypeKey};

/// The Vanilla block display entity.
///
/// In addition to having the common display entity properties, this entity
/// also stores a [`BlockStateId`] to render as.
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

    /// Gets the block state (by ID) of this block display.
    pub fn block_state(&self) -> BlockStateId {
        *self.entity_data.lock().block_state.get()
    }

    /// Sets the block state (by ID) of this block display.
    pub fn set_block_state(&self, id: BlockStateId) {
        self.entity_data.lock().block_state.set(id);
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
        self.save_display(nbt);

        nbt.insert("block_state", block_state_nbt::save(vanilla_blocks::BLACK_CONCRETE_POWDER.default_state()) /*block_state_nbt::save(self.block_state())*/);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_display(nbt);

        self.set_block_state(
            nbt.compound("block_state")
                .and_then(block_state_nbt::load)
                .unwrap_or(vanilla_blocks::AIR.default_state()),
        );
    }
}

display_impl!(BlockDisplayEntity);
