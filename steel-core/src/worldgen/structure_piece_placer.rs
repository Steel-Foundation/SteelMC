//! Feature-stage structure piece placement boundary.
//!
//! Structure starts are generated before noise, but vanilla emits the piece
//! blocks during biome decoration. This module is the single dispatch point for
//! that pass; individual family placers must fill in exact vanilla behavior
//! before any payload variant starts writing blocks.

mod buried_treasure;
mod mineshaft;
mod pool_element;
mod ruined_portal;
mod template_piece;
mod template_processors;

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::{Registry, vanilla_blocks};
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::{BlockPos, BlockStateId, BoundingBox, Direction, types::UpdateFlags};

use crate::world::structure::{ProceduralPieceData, StructurePiece, StructurePiecePayload};
use crate::worldgen::region::WorldGenRegion;

pub(crate) struct StructurePiecePlacer;

impl StructurePiecePlacer {
    /// Vanilla jigsaw pool-element placement flags: `UPDATE_CLIENTS | UPDATE_KNOWN_SHAPE`.
    pub(crate) const JIGSAW_UPDATE_FLAGS: UpdateFlags =
        UpdateFlags::UPDATE_CLIENTS.union(UpdateFlags::UPDATE_KNOWN_SHAPE);
    /// Vanilla template-piece placement flags: `UPDATE_CLIENTS`.
    pub(crate) const TEMPLATE_UPDATE_FLAGS: UpdateFlags = UpdateFlags::UPDATE_CLIENTS;

    /// Places one already-clipped structure piece.
    ///
    /// Returns whether the vanilla placement call succeeded. Later milestones
    /// must implement each remaining payload variant completely before it can
    /// return `true`.
    pub(crate) fn place_piece(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        piece: &mut StructurePiece,
        reference_pos: BlockPos,
        clip: BoundingBox,
        random: &mut WorldgenRandom,
        biome_zoom_seed: i64,
    ) -> bool {
        let mut piece_bounding_box = piece.bounding_box;
        let piece_orientation = piece.orientation;
        let placed = match &mut piece.payload {
            StructurePiecePayload::Jigsaw(data) => Self::place_pool_element(
                region,
                registry,
                &data.pool_element,
                BlockPos::new(data.position.0, data.position.1, data.position.2),
                reference_pos,
                data.rotation,
                clip,
                random,
                data.liquid_settings,
                biome_zoom_seed,
            ),
            StructurePiecePayload::Template(data) => Self::place_template_piece(
                region,
                registry,
                data,
                &mut piece_bounding_box,
                reference_pos,
                clip,
                random,
            ),
            StructurePiecePayload::Procedural(ProceduralPieceData::Mineshaft(data)) => {
                Self::place_mineshaft_piece(
                    region,
                    registry,
                    piece_bounding_box,
                    piece_orientation,
                    data,
                    clip,
                    random,
                    biome_zoom_seed,
                )
            }
            StructurePiecePayload::Procedural(ProceduralPieceData::BuriedTreasure) => {
                Self::place_buried_treasure_piece(region, &mut piece_bounding_box, clip, random)
            }
            StructurePiecePayload::Procedural(ProceduralPieceData::Unimplemented) => false,
        };
        piece.bounding_box = piece_bounding_box;
        placed
    }

    const VANILLA_HORIZONTAL_DIRECTIONS: [Direction; 4] = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];

    pub(super) fn reorient_chest(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockStateId {
        let mut solid_neighbor = None;

        for direction in Self::VANILLA_HORIZONTAL_DIRECTIONS {
            let relative_pos = pos.relative(direction);
            let neighbor = region.block_state(relative_pos);
            if neighbor.get_block() == &vanilla_blocks::CHEST {
                return state;
            }

            if neighbor.is_solid_render() {
                if solid_neighbor.is_some() {
                    solid_neighbor = None;
                    break;
                }
                solid_neighbor = Some(direction);
            }
        }

        if let Some(direction) = solid_neighbor {
            return state.set_value(
                &BlockStateProperties::HORIZONTAL_FACING,
                direction.opposite(),
            );
        }

        let mut lock_dir = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        let mut relative_pos = pos.relative(lock_dir);
        if region.block_state(relative_pos).is_solid_render() {
            lock_dir = lock_dir.opposite();
            relative_pos = pos.relative(lock_dir);
        }
        if region.block_state(relative_pos).is_solid_render() {
            lock_dir = lock_dir.rotate_y_clockwise();
            relative_pos = pos.relative(lock_dir);
        }
        if region.block_state(relative_pos).is_solid_render() {
            lock_dir = lock_dir.opposite();
        }
        state.set_value(&BlockStateProperties::HORIZONTAL_FACING, lock_dir)
    }
}
