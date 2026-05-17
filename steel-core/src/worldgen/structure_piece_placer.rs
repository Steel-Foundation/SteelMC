//! Feature-stage structure piece placement boundary.
//!
//! Structure starts are generated before noise, but vanilla emits the piece
//! blocks during biome decoration. This module is the single dispatch point for
//! that pass; individual family placers must fill in exact vanilla behavior
//! before any payload variant starts writing blocks.

use steel_registry::Registry;
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::{BoundingBox, types::UpdateFlags};

use crate::world::structure::{StructurePiece, StructurePiecePayload};
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
    /// Returns whether any block was written. Milestone 1 keeps all payload
    /// variants disabled here; later milestones must implement each variant
    /// completely before it can return `true`.
    pub(crate) fn place_piece(
        _region: &mut WorldGenRegion<'_>,
        _registry: &Registry,
        piece: &StructurePiece,
        _clip: BoundingBox,
        _random: &mut WorldgenRandom,
    ) -> bool {
        match &piece.payload {
            StructurePiecePayload::Jigsaw(_data) => {
                let _flags = Self::JIGSAW_UPDATE_FLAGS;
                // TODO: Implement full vanilla jigsaw pool-element block placement.
                false
            }
            StructurePiecePayload::Template(_data) => {
                let _flags = Self::TEMPLATE_UPDATE_FLAGS;
                // TODO: Implement full vanilla template-backed structure placement.
                false
            }
            StructurePiecePayload::Procedural(_data) => {
                // TODO: Implement full vanilla procedural structure placement.
                false
            }
        }
    }
}
