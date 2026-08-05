//! Comparator block-entity output storage.

use std::sync::Weak;

use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::world::World;

struct ComparatorState {
    output_signal: i32,
}

/// Vanilla `ComparatorBlockEntity`.
pub struct ComparatorBlockEntity {
    base: BlockEntityBase,
    state: SyncMutex<ComparatorState>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ComparatorBlockEntity`.
unsafe impl DowncastType for ComparatorBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/comparator");
}

impl ComparatorBlockEntity {
    /// Creates comparator storage with vanilla's zero output.
    #[must_use]
    pub fn new(world: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            base: BlockEntityBase::new(&vanilla_block_entity_types::COMPARATOR, world, pos, state),
            state: SyncMutex::new(ComparatorState { output_signal: 0 }),
        }
    }

    /// Returns the comparator's cached output signal.
    #[must_use]
    pub fn output_signal(&self) -> i32 {
        self.state.lock().output_signal
    }

    /// Replaces the comparator's cached output signal.
    pub fn set_output_signal(&self, output_signal: i32) {
        self.state.lock().output_signal = output_signal;
    }
}

impl BlockEntity for ComparatorBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt: NbtCompoundView<'_, '_> = nbt.into();
        self.state.lock().output_signal = nbt.int("OutputSignal").unwrap_or(0);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        nbt.insert("OutputSignal", self.output_signal());
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    use super::*;

    fn comparator() -> ComparatorBlockEntity {
        init_test_registry();
        ComparatorBlockEntity::new(
            Weak::new(),
            BlockPos::new(4, 65, -9),
            vanilla_blocks::COMPARATOR.default_state(),
        )
    }

    #[test]
    fn output_signal_round_trips_with_vanilla_nbt_key() {
        let source = comparator();
        source.set_output_signal(11);
        let mut nbt = NbtCompound::new();
        source.save_additional(&mut nbt);
        assert_eq!(nbt.int("OutputSignal"), Some(11));

        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test NBT should reborrow");
        let loaded = comparator();
        loaded.load_additional(&borrowed);
        assert_eq!(loaded.output_signal(), 11);
    }

    #[test]
    fn missing_output_signal_loads_vanilla_default() {
        let nbt = NbtCompound::new();
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test NBT should reborrow");
        let loaded = comparator();
        loaded.set_output_signal(15);
        loaded.load_additional(&borrowed);
        assert_eq!(loaded.output_signal(), 0);
    }
}
