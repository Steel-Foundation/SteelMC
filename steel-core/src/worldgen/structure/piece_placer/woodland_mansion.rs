use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BoundingBox, types::UpdateFlags};
use steel_worldgen::structure::{StructurePiece, StructureStart};

use super::StructurePiecePlacer;
use crate::worldgen::region::WorldGenRegion;

impl StructurePiecePlacer {
    /// Fills cobblestone below the mansion footprint down to the first block
    /// that is neither air nor liquid.
    pub(super) fn after_place_woodland_mansion(
        region: &mut WorldGenRegion<'_>,
        pieces: &[StructurePiece],
        clip: BoundingBox,
    ) {
        let Some(structure_box) = StructureStart::compute_bounding_box(pieces, 0) else {
            return;
        };
        let min_y = region.min_y();
        let y_start = structure_box.min_y();
        let cobblestone = vanilla_blocks::COBBLESTONE.default_state();

        for x in clip.min_x()..=clip.max_x() {
            for z in clip.min_z()..=clip.max_z() {
                let top = BlockPos::new(x, y_start, z);
                if region.block_state(top).is_air()
                    || !structure_box.contains_blockpos(top)
                    || !pieces
                        .iter()
                        .any(|piece| piece.bounding_box.contains_blockpos(top))
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
