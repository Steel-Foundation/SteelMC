#![expect(missing_docs, reason = "self-explanatory utility types")]

mod codec_glue;
mod gameplay;
mod identifier;
mod packed_position;
mod position;

pub use codec_glue::{BlockStateId, Todo};
pub use gameplay::{Difficulty, GameType, InteractionHand, UpdateFlags};
pub use identifier::Identifier;
pub use packed_position::{
    InvalidPackedSectionBlockPos, PackedBlockPos, PackedChunkLocalXZ, PackedChunkPos,
    PackedSectionBlockPos, PackedSectionPos,
};
pub use position::{
    BlockPos, BlockPosWithinManhattan, ChunkPos, GlobalPos, SectionPos, SpiralAround,
    TraversalNodeStatus,
};

#[cfg(test)]
mod tests;
