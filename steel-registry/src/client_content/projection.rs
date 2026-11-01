//! Selects a vanilla transport state independently of the server definition.

use steel_utils::BlockStateId;

use crate::{
    Registry,
    blocks::{
        BlockRef,
        behavior::OffsetType,
        shapes::{BooleanOp, VoxelShape, join_is_not_empty, join_unoptimized_boxes},
    },
    vanilla_blocks,
};

#[derive(Debug)]
pub(super) struct ProjectionCandidate {
    state: BlockStateId,
    block: BlockRef,
    collision: VoxelShape,
    outline: VoxelShape,
}

impl Registry {
    fn projection_candidates(&mut self) {
        if !self.client_content.projections.is_empty() {
            return;
        }
        for (_, block) in self.blocks.iter() {
            if !self.client_content.is_vanilla_block(block)
                || block.config.dynamic_shape
                || block.config.liquid
                || block.config.is_air
                || block.config.offset_type != OffsetType::None
                || self.block_entity_types.has_block_entity(block)
            {
                continue;
            }
            let base = self.blocks.get_base_state_id(block);
            let default = block.default_state_offset;
            for offset in std::iter::once(default)
                .chain((0..block.state_count()).filter(|offset| *offset != default))
            {
                if !block.fluid_state.value(offset).is_empty() {
                    continue;
                }
                let outline = block.get_outline_shape(offset);
                if outline.is_empty() {
                    continue;
                }
                let collision = block.get_collision_shape(offset);
                // State flags with identical geometry do not improve projection.
                if self.client_content.projections.iter().any(|candidate| {
                    candidate.block == block
                        && candidate.collision == collision
                        && candidate.outline == outline
                }) {
                    continue;
                }
                self.client_content.projections.push(ProjectionCandidate {
                    state: BlockStateId(base.0 + offset),
                    block,
                    collision,
                    outline,
                });
            }
        }
    }

    /// Approximation uses a collision contained within the authored collision,
    /// so a visible carrier cannot project outside the supplied model outline.
    /// Minimum missing collision volume wins; ties prefer the requested material,
    /// then invisible carriers, then stable vanilla state ordering.
    pub(super) fn select_client_state(
        &mut self,
        collision: VoxelShape,
        outline: VoxelShape,
        preferred: Option<BlockRef>,
        approximate: bool,
    ) -> Result<BlockStateId, String> {
        self.projection_candidates();
        let mut best: Option<(f64, u8, BlockStateId)> = None;
        for candidate in &self.client_content.projections {
            let invisible = candidate.block == &vanilla_blocks::BARRIER
                || candidate.block == &vanilla_blocks::STRUCTURE_VOID;
            if (!invisible && join_is_not_empty(candidate.outline, outline, BooleanOp::OnlyFirst))
                || join_is_not_empty(candidate.collision, collision, BooleanOp::OnlyFirst)
            {
                continue;
            }
            let missing =
                join_unoptimized_boxes(collision, candidate.collision, BooleanOp::OnlyFirst);
            if !approximate && !missing.is_empty() {
                continue;
            }
            let error = missing
                .iter()
                .map(|b| {
                    (b.max_x() - b.min_x()) * (b.max_y() - b.min_y()) * (b.max_z() - b.min_z())
                })
                .sum::<f64>();
            let priority = if preferred == Some(candidate.block) {
                0
            } else if invisible {
                1
            } else {
                2
            };
            let score = (error, priority, candidate.state);
            if best.as_ref().is_none_or(|best| {
                score
                    .0
                    .total_cmp(&best.0)
                    .then(score.1.cmp(&best.1))
                    .then(score.2.0.cmp(&best.2.0))
                    .is_lt()
            }) {
                best = Some(score);
            }
        }
        best.map(|(_,_,state)| state).ok_or_else(|| "no compatible vanilla collision projection; opt into approximation for an unsupported shape".into())
    }
}
