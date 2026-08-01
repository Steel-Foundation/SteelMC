//! Stateless ticking storage for daylight detectors.

use std::sync::{Arc, Weak};

use simdnbt::borrow::BaseNbtCompound as BorrowedNbtCompound;
use simdnbt::owned::NbtCompound;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_block_entity_types;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::behavior::blocks::DaylightDetectorBlock;
use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::world::World;

/// Vanilla `DaylightDetectorBlockEntity`.
pub struct DaylightDetectorBlockEntity {
    base: BlockEntityBase,
}

// SAFETY: This key is owned by Steel and uniquely identifies
// `DaylightDetectorBlockEntity`.
unsafe impl DowncastType for DaylightDetectorBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/daylight_detector");
}

impl DaylightDetectorBlockEntity {
    /// Creates daylight-detector ticking storage.
    #[must_use]
    pub fn new(world: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            base: BlockEntityBase::new(
                &vanilla_block_entity_types::DAYLIGHT_DETECTOR,
                world,
                pos,
                state,
            ),
        }
    }
}

impl BlockEntity for DaylightDetectorBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn load_additional(&self, _nbt: &BorrowedNbtCompound<'_>) {}

    fn save_additional(&self, _nbt: &mut NbtCompound) {}

    fn tick(&self, world: &Arc<World>) {
        if world.game_time() % 20 != 0 {
            return;
        }

        let pos = self.get_block_pos();
        let state = self.get_block_state();
        let target = DaylightDetectorBlock::signal_strength(world, pos, state);
        if state.get_value(&BlockStateProperties::POWER) == target {
            return;
        }

        world.set_block(
            pos,
            state.set_value(&BlockStateProperties::POWER, target),
            UpdateFlags::UPDATE_ALL,
        );
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;
    use steel_registry::{vanilla_blocks, vanilla_world_clocks};
    use steel_utils::ChunkPos;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn detector_updates_only_on_vanilla_twenty_game_tick_cadence() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("daylight_detector_cadence");
        assert_eq!(
            world.set_clock_total_ticks(&vanilla_world_clocks::OVERWORLD, 18_000),
            Some(())
        );
        let pos = BlockPos::new(4, 64, 4);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let state = vanilla_blocks::DAYLIGHT_DETECTOR
            .default_state()
            .set_value(&BlockStateProperties::INVERTED, true);
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_ALL));
        let detector = DaylightDetectorBlockEntity::new(Arc::downgrade(&world), pos, state);

        detector.tick(&world);
        assert_eq!(
            world
                .get_block_state(pos)
                .get_value(&BlockStateProperties::POWER),
            11
        );

        world.level_data.write().set_game_time(1);
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_ALL));
        detector.tick(&world);
        assert_eq!(
            world
                .get_block_state(pos)
                .get_value(&BlockStateProperties::POWER),
            0
        );
    }
}
