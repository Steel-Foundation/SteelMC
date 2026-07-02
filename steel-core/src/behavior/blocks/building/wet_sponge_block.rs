use std::sync::Arc;

use steel_macros::block_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::BlockRef;
use steel_registry::{level_events, sound_events, vanilla_blocks};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::world::World;

#[block_behavior]
/// Wet sponge behavior
pub struct WetSpongeBlock {
    block: BlockRef,
}

impl WetSpongeBlock {
    /// New wet sponge block
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for WetSpongeBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn on_place(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        if !world.dimension_type.water_evaporates {
            return;
        }

        world.set_block(
            pos,
            vanilla_blocks::SPONGE.default_state(),
            UpdateFlags::UPDATE_ALL,
        );
        world.level_event(level_events::PARTICLES_WATER_EVAPORATING, pos, 0, None);
        world.play_sound(
            &sound_events::BLOCK_WET_SPONGE_DRIES,
            SoundSource::Blocks,
            pos,
            1.0,
            (1.0 + rand::random::<f32>() * 0.2) * 0.7,
            None,
        );
    }
}
