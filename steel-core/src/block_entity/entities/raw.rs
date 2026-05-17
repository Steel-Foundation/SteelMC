//! NBT-preserving fallback block entity.

use std::any::Any;
use std::sync::{Arc, Weak};

use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_utils::{BlockPos, BlockStateId};

use crate::block_entity::BlockEntity;
use crate::world::World;

/// Steel-specific fallback for block entity types whose runtime behavior is not implemented yet.
///
/// Vanilla has concrete classes for every block entity type. Steel uses this only to preserve
/// worldgen and disk NBT until the corresponding typed implementation is added.
pub struct RawBlockEntity {
    block_entity_type: BlockEntityTypeRef,
    level: Weak<World>,
    pos: BlockPos,
    state: BlockStateId,
    removed: bool,
    data: NbtCompound,
}

impl RawBlockEntity {
    /// Creates a raw block entity without additional NBT.
    #[must_use]
    pub fn new(
        block_entity_type: BlockEntityTypeRef,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Self {
        Self::with_data(block_entity_type, level, pos, state, NbtCompound::new())
    }

    /// Creates a raw block entity with already-owned additional NBT.
    #[must_use]
    pub const fn with_data(
        block_entity_type: BlockEntityTypeRef,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
        data: NbtCompound,
    ) -> Self {
        Self {
            block_entity_type,
            level,
            pos,
            state,
            removed: false,
            data,
        }
    }
}

impl BlockEntity for RawBlockEntity {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn get_type(&self) -> BlockEntityTypeRef {
        self.block_entity_type
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

    fn load_additional(&mut self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();
        self.data = nbt_view.to_owned();
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        *nbt = self.data.clone();
    }
}
