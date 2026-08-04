use steel_registry::{blocks::block_state_ext::BlockStateExt, vanilla_blocks};
use steel_utils::{BlockPos, BlockStateId, ChunkPos, Direction, SectionPos};

use super::{
    CachedLightBlock, LIGHT_BLOCKED, LightAxisDirection, LightCacheLayout, LightDirectionSet,
    LightLayer, LightLayerEdit, LightQueueFlags, LightSectionEmptinessChange,
    LightSectionReadCache, LightWorkset, MAX_LIGHT_LEVEL, PackedLightPropagationQueues,
    PackedLightQueueEntry, get_light_block_into, get_light_opacity, light_occlusion_shape,
};

/// Error returned when a sky-light propagation context is built from mismatched caches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkyLightPropagationContextError {
    /// Sky-light propagation requires a sky light edit cache.
    WrongLayer {
        /// Layer supplied by the edit cache.
        layer: LightLayer,
    },
    /// Section and light caches were built from different cache layouts.
    LayoutMismatch {
        /// Layout used by the section cache.
        section_layout: Box<LightCacheLayout>,
        /// Layout used by the light cache.
        light_layout: Box<LightCacheLayout>,
    },
    /// The workset does not contain its center chunk.
    MissingCenterChunk {
        /// Missing center chunk position.
        chunk_pos: ChunkPos,
    },
}

impl SkyLightPropagationContextError {
    fn layout_mismatch(section_layout: LightCacheLayout, light_layout: LightCacheLayout) -> Self {
        Self::LayoutMismatch {
            section_layout: Box::new(section_layout),
            light_layout: Box::new(light_layout),
        }
    }
}

/// Sections whose visible sky-light data changed during a scoped update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkyLightUpdateResult {
    /// Light sections that should be reported to the world/chunk update layer.
    pub updated_sections: Vec<SectionPos>,
}

/// Whether chunk sky-light generation must validate edge consistency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkyLightChunkEdgeChecks {
    /// Seed skylight and validate this chunk's horizontal edges against neighbors.
    Required,
    /// Trust existing neighboring light and pull initialized edge levels inward.
    Skipped,
}

/// Seeds and propagates sky light for the center chunk without edge checks.
pub fn propagate_sky_light_chunk_without_edge_checks(
    workset: &LightWorkset,
) -> Result<SkyLightUpdateResult, SkyLightPropagationContextError> {
    propagate_sky_light_chunk(workset, SkyLightChunkEdgeChecks::Skipped)
}

/// Seeds and propagates sky light for the center chunk of a scoped workset.
///
/// This matches `ScalableLux` `SkyStarLightEngine.lightChunk`: sky sections
/// around non-empty sections are initialized, full skylight is propagated
/// downward, then the caller chooses between validating edge consistency or
/// pulling already-initialized neighbor levels inward.
pub fn propagate_sky_light_chunk(
    workset: &LightWorkset,
    edge_checks: SkyLightChunkEdgeChecks,
) -> Result<SkyLightUpdateResult, SkyLightPropagationContextError> {
    workset.with_chunk_read_cache(|chunk_cache| {
        let layout = chunk_cache.layout();
        let Some(center_slot) = layout.cached_chunk(layout.center_chunk()) else {
            return Err(SkyLightPropagationContextError::MissingCenterChunk {
                chunk_pos: layout.center_chunk(),
            });
        };
        if chunk_cache.chunk(center_slot).is_none() {
            return Err(SkyLightPropagationContextError::MissingCenterChunk {
                chunk_pos: layout.center_chunk(),
            });
        }

        chunk_cache.with_section_read_cache(|section_cache| {
            chunk_cache.with_light_edit(LightLayer::Sky, |mut light_edit| {
                let mut queues = PackedLightPropagationQueues::new();

                {
                    let mut context = SkyLightPropagationContext::new(
                        section_cache,
                        &mut light_edit,
                        &mut queues,
                    )?;
                    context.reset_center_chunk_sections();
                    context.handle_unlit_empty_section_changes(layout.center_chunk());
                    context.light_chunk(layout.center_chunk(), edge_checks);
                    if edge_checks == SkyLightChunkEdgeChecks::Required {
                        context.deinit_and_lazy_init_empty_sections(layout.center_chunk(), true);
                    }
                }

                let mut updated_sections = Vec::new();
                light_edit.commit(None, |section_pos| updated_sections.push(section_pos));
                Ok(SkyLightUpdateResult { updated_sections })
            })
        })
    })
}

/// Force-synchronizes sky-light sections for an already-lit loaded chunk.
pub fn force_load_sky_light_chunk(
    workset: &LightWorkset,
) -> Result<SkyLightUpdateResult, SkyLightPropagationContextError> {
    workset.with_chunk_read_cache(|chunk_cache| {
        let layout = ensure_center_chunk(chunk_cache)?;

        chunk_cache.with_section_read_cache(|section_cache| {
            chunk_cache.with_light_edit(LightLayer::Sky, |mut light_edit| {
                let mut queues = PackedLightPropagationQueues::new();

                {
                    let mut context = SkyLightPropagationContext::new(
                        section_cache,
                        &mut light_edit,
                        &mut queues,
                    )?;
                    context.handle_loaded_empty_section_changes(layout.center_chunk());
                }

                let mut updated_sections = Vec::new();
                light_edit.commit(None, |section_pos| updated_sections.push(section_pos));
                Ok(SkyLightUpdateResult { updated_sections })
            })
        })
    })
}

/// Validates already-loaded sky-light chunk edges without resetting sections.
pub fn check_sky_light_chunk_edges(
    workset: &LightWorkset,
) -> Result<SkyLightUpdateResult, SkyLightPropagationContextError> {
    workset.with_chunk_read_cache(|chunk_cache| {
        let layout = ensure_center_chunk(chunk_cache)?;

        chunk_cache.with_section_read_cache(|section_cache| {
            chunk_cache.with_light_edit(LightLayer::Sky, |mut light_edit| {
                let mut queues = PackedLightPropagationQueues::new();

                {
                    let mut context = SkyLightPropagationContext::new(
                        section_cache,
                        &mut light_edit,
                        &mut queues,
                    )?;
                    context.light.rewrite_missing_sections_for_skylight();
                    for section_y in (layout.range().min_section_y()
                        ..layout.range().max_section_y_exclusive())
                        .rev()
                    {
                        context.check_missing_section(layout.center_chunk(), section_y, true);
                    }
                    context.check_chunk_edges(
                        layout.center_chunk(),
                        layout.range().min_section_y(),
                        layout.range().max_section_y_exclusive() - 1,
                    );
                }

                let mut updated_sections = Vec::new();
                light_edit.commit(None, |section_pos| updated_sections.push(section_pos));
                Ok(SkyLightUpdateResult { updated_sections })
            })
        })
    })
}

/// Loads already-persisted sky light and validates chunk edges without resetting sections.
pub fn load_sky_light_chunk(
    workset: &LightWorkset,
) -> Result<SkyLightUpdateResult, SkyLightPropagationContextError> {
    let mut updated_sections = force_load_sky_light_chunk(workset)?.updated_sections;
    updated_sections.extend(check_sky_light_chunk_edges(workset)?.updated_sections);
    Ok(SkyLightUpdateResult { updated_sections })
}

fn ensure_center_chunk(
    chunk_cache: &super::LightChunkReadCache<'_>,
) -> Result<LightCacheLayout, SkyLightPropagationContextError> {
    let layout = chunk_cache.layout();
    let Some(center_slot) = layout.cached_chunk(layout.center_chunk()) else {
        return Err(SkyLightPropagationContextError::MissingCenterChunk {
            chunk_pos: layout.center_chunk(),
        });
    };
    if chunk_cache.chunk(center_slot).is_none() {
        return Err(SkyLightPropagationContextError::MissingCenterChunk {
            chunk_pos: layout.center_chunk(),
        });
    }

    Ok(layout)
}

/// Runs ScalableLux-style sky-light propagation for changed blocks in a scoped workset.
pub fn propagate_sky_light_changes(
    workset: &LightWorkset,
    positions: impl IntoIterator<Item = BlockPos>,
) -> Result<SkyLightUpdateResult, SkyLightPropagationContextError> {
    propagate_sky_light_changes_with_empty_sections(workset, positions, [])
}

/// Runs sky-light propagation after applying real section emptiness transitions.
pub fn propagate_sky_light_changes_with_empty_sections(
    workset: &LightWorkset,
    positions: impl IntoIterator<Item = BlockPos>,
    empty_sections: impl IntoIterator<Item = LightSectionEmptinessChange>,
) -> Result<SkyLightUpdateResult, SkyLightPropagationContextError> {
    let positions = positions.into_iter().collect::<Vec<_>>();
    let empty_sections = empty_sections.into_iter().collect::<Vec<_>>();

    workset.with_chunk_read_cache(|chunk_cache| {
        let layout = chunk_cache.layout();
        // ScalableLux drops queued dynamic changes once the center chunk leaves the light cache.
        let Some(center_slot) = layout.cached_chunk(layout.center_chunk()) else {
            return Ok(SkyLightUpdateResult {
                updated_sections: Vec::new(),
            });
        };
        if chunk_cache.chunk(center_slot).is_none() {
            return Ok(SkyLightUpdateResult {
                updated_sections: Vec::new(),
            });
        }

        chunk_cache.with_section_read_cache(|section_cache| {
            chunk_cache.with_light_edit(LightLayer::Sky, |mut light_edit| {
                let mut queues = PackedLightPropagationQueues::new();

                {
                    let mut context = SkyLightPropagationContext::new(
                        section_cache,
                        &mut light_edit,
                        &mut queues,
                    )?;
                    let mut changed_chunks = Vec::new();
                    for change in &empty_sections {
                        let chunk_pos =
                            ChunkPos::new(change.section_pos.x(), change.section_pos.z());
                        context
                            .light
                            .set_section_empty(change.section_pos, change.empty);
                        if !changed_chunks.contains(&chunk_pos) {
                            changed_chunks.push(chunk_pos);
                        }
                    }
                    for chunk_pos in changed_chunks {
                        context.deinit_and_lazy_init_empty_sections(chunk_pos, false);
                    }
                    context.propagate_block_changes(&positions);
                }

                let mut updated_sections = Vec::new();
                light_edit.commit(None, |section_pos| updated_sections.push(section_pos));
                Ok(SkyLightUpdateResult { updated_sections })
            })
        })
    })
}

mod context;

pub use context::SkyLightPropagationContext;

mod algorithms;
mod queue_engine;

#[cfg(test)]
mod tests;
