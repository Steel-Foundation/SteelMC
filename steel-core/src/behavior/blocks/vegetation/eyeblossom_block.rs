use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::vanilla_block_tags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, World};

use super::{BlockRef, default_surviving_state, survives_on_tag};

#[derive(Clone, Copy)]
/// Vanilla open/closed eyeblossom type from `classes.json`.
pub enum EyeblossomType {
    /// Emits open-eyeblossom effects and transforms closed at daytime.
    Open,
    /// Emits closed-eyeblossom effects and transforms open at nighttime.
    Closed,
}

/// Vanilla `EyeblossomBlock` survival and ticking shape.
// TODO: Implement eyeblossom day/night transforms, sounds, particles, and bee effects
// once Steel has environment attributes and particle dispatch.
#[block_behavior]
pub struct EyeblossomBlock {
    block: BlockRef,
    #[json_arg(r#enum = "EyeblossomType", json = "type")]
    eyeblossom_type: EyeblossomType,
}

impl EyeblossomBlock {
    /// Creates a new eyeblossom behavior.
    #[must_use]
    pub const fn new(block: BlockRef, eyeblossom_type: EyeblossomType) -> Self {
        Self {
            block,
            eyeblossom_type,
        }
    }
}

impl BlockBehavior for EyeblossomBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        survives_on_tag(world, pos, &vanilla_block_tags::SUPPORTS_VEGETATION_TAG)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        true
    }

    fn random_tick(&self, _state: BlockStateId, _world: &Arc<World>, _pos: BlockPos) {
        let _ = self.eyeblossom_type;
    }

    fn tick(&self, _state: BlockStateId, _world: &Arc<World>, _pos: BlockPos) {
        let _ = self.eyeblossom_type;
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{REGISTRY, Registry, vanilla_blocks};
    use steel_utils::BlockPos;

    use super::*;

    struct SingleSupportLevel {
        support: BlockRef,
    }

    impl SingleSupportLevel {
        const fn new(support: BlockRef) -> Self {
            Self { support }
        }
    }

    impl LevelReader for SingleSupportLevel {
        fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
            if pos == BlockPos::new(0, 63, 0) {
                self.support.default_state()
            } else {
                vanilla_blocks::AIR.default_state()
            }
        }

        fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
            0
        }

        fn min_y(&self) -> i32 {
            -64
        }

        fn height(&self) -> i32 {
            384
        }
    }

    fn init_registry() {
        let mut registry = Registry::new_vanilla();
        registry.freeze();
        let _ = REGISTRY.init(registry);
    }

    #[test]
    fn eyeblossom_requires_vegetation_support() {
        init_registry();
        let behavior =
            EyeblossomBlock::new(&vanilla_blocks::CLOSED_EYEBLOSSOM, EyeblossomType::Closed);
        let pos = BlockPos::new(0, 64, 0);
        let state = vanilla_blocks::CLOSED_EYEBLOSSOM.default_state();

        assert!(behavior.can_survive(state, &SingleSupportLevel::new(&vanilla_blocks::DIRT), pos));
        assert!(!behavior.can_survive(state, &SingleSupportLevel::new(&vanilla_blocks::AIR), pos));
    }
}
