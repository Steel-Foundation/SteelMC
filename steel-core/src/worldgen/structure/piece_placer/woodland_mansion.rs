use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::vanilla_blocks;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BoundingBox};
use steel_worldgen::structure::{StructurePiece, StructureStart};

use crate::worldgen::region::WorldGenRegion;

use super::StructurePiecePlacer;

impl StructurePiecePlacer {
    /// Fills cobblestone from the mansion's lowest layer down to terrain.
    pub(super) fn after_place_woodland_mansion(
        region: &mut WorldGenRegion<'_>,
        pieces: &[StructurePiece],
        clip: BoundingBox,
    ) {
        let Some(bounding_box) = StructureStart::compute_bounding_box(pieces, 0) else {
            return;
        };
        let y_start = bounding_box.min_y();
        let min_y = region.min_y();
        let cobblestone = vanilla_blocks::COBBLESTONE.default_state();

        for x in clip.min_x()..=clip.max_x() {
            for z in clip.min_z()..=clip.max_z() {
                let start_pos = BlockPos::new(x, y_start, z);
                if region.block_state(start_pos).is_air()
                    || !pieces
                        .iter()
                        .any(|piece| piece.bounding_box.contains_blockpos(start_pos))
                {
                    continue;
                }

                for y in (min_y + 1..y_start).rev() {
                    let pos = BlockPos::new(x, y, z);
                    let state = region.block_state(pos);
                    if !state.is_air() && !state.get_block().config.liquid {
                        break;
                    }
                    let _ = region.set_block_state(pos, cobblestone, UpdateFlags::UPDATE_CLIENTS);
                }
            }
        }
    }
}
