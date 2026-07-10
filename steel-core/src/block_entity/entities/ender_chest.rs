//! Ender chest block entity implementation.
//!
//! Handles lid animations and sound effects for ender chests.

use std::any::Any;
use std::sync::{Arc, Weak};

use simdnbt::borrow::BaseNbtCompound;
use simdnbt::owned::NbtCompound;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId};

use crate::block_entity::BlockEntity;
use crate::world::World;

/// Ender chest block entity.
///
/// Unlike regular chests, this block entity does not store items. The items are
/// stored in the player's ender chest inventory. This block entity handles lid
/// animations and sounds.
pub struct EnderChestBlockEntity {
    /// Weak reference to the world.
    level: Weak<World>,
    /// Position in the world.
    pos: BlockPos,
    /// Current block state.
    state: BlockStateId,
    /// Whether this entity has been marked for removal.
    removed: bool,
}

impl EnderChestBlockEntity {
    /// Creates a new ender chest block entity.
    #[must_use]
    pub const fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            level,
            pos,
            state,
            removed: false,
        }
    }

    /// Called when a player starts looking into the ender chest.
    #[expect(
        clippy::unused_self,
        reason = "Will use self once ContainerOpenersCounter is implemented"
    )]
    pub const fn start_open(&mut self) {
        // TODO: Implement ContainerOpenersCounter to track open state and play sounds.
        // Needs a lid controller and block events.
    }

    /// Called when a player stops looking into the ender chest.
    #[expect(
        clippy::unused_self,
        reason = "Will use self once ContainerOpenersCounter is implemented"
    )]
    pub const fn stop_open(&mut self) {
        // TODO: Implement ContainerOpenersCounter to track open state and play sounds.
        // Needs a lid controller and block events.
    }
}

impl BlockEntity for EnderChestBlockEntity {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn get_type(&self) -> BlockEntityTypeRef {
        &vanilla_block_entity_types::ENDER_CHEST
    }

    fn get_block_pos(&self) -> BlockPos {
        self.pos
    }

    fn get_block_state(&self) -> BlockStateId {
        self.state
    }

    fn set_block_state(&mut self, state: BlockStateId) {
        self.state = state;
    }

    fn is_removed(&self) -> bool {
        self.removed
    }

    fn set_removed(&mut self) {
        self.removed = true;
    }

    fn clear_removed(&mut self) {
        self.removed = false;
    }

    fn get_level(&self) -> Option<Arc<World>> {
        self.level.upgrade()
    }

    fn load_additional(&mut self, _nbt: &BaseNbtCompound<'_>) {}

    fn save_additional(&self, _nbt: &mut NbtCompound) {}
}
