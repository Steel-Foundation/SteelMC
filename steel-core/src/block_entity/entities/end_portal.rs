//! End portal block entity.

use std::any::Any;
use std::sync::{Arc, Weak};

use simdnbt::borrow::BaseNbtCompound as BorrowedNbtCompound;
use simdnbt::owned::NbtCompound;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId};

use crate::block_entity::BlockEntity;
use crate::world::World;

/// Vanilla `TheEndPortalBlockEntity`.
pub struct EndPortalBlockEntity {
    level: Weak<World>,
    pos: BlockPos,
    state: BlockStateId,
    removed: bool,
}

impl EndPortalBlockEntity {
    /// Creates an End portal block entity.
    #[must_use]
    pub const fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            level,
            pos,
            state,
            removed: false,
        }
    }
}

impl BlockEntity for EndPortalBlockEntity {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn get_type(&self) -> BlockEntityTypeRef {
        &vanilla_block_entity_types::END_PORTAL
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

    fn load_additional(&mut self, _nbt: &BorrowedNbtCompound<'_>) {}

    fn save_additional(&self, _nbt: &mut NbtCompound) {}

    fn get_update_tag(&self) -> Option<NbtCompound> {
        Some(NbtCompound::new())
    }
}
