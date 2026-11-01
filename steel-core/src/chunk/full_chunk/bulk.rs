//! Section-batched block writes for [`World::fill_blocks`](crate::world::World::fill_blocks).
//!
//! Unlike [`FullChunkRef::set_block_state`], which locks a section per block and runs placement,
//! removal and neighbor callbacks, a bulk write takes each section's write lock once and only
//! keeps the chunk-local derived state consistent: section counters, block entities (detached
//! when their block changes, created for new block-entity blocks), heightmaps, sky-light
//! sources and light emptiness. This mirrors what Paper and FAWE fast fills do: no physics,
//! no `onPlace`/`onRemove`, no container drops.

use rustc_hash::FxHashMap;
use steel_registry::{REGISTRY, blocks::block_state_ext::BlockStateExt};
use steel_utils::{BlockPos, BlockStateId, PackedSectionBlockPos, SectionPos};

use super::FullChunkRef;
use crate::block_entity::{DetachedBlockEntity, LifecycleDispatchers};
use crate::chunk::light::{LightSectionEmptinessChange, has_different_light_properties};

/// World-level follow-up work collected while filling one chunk.
#[derive(Debug, Default)]
pub(crate) struct ChunkFillOutput {
    /// Changed positions per section index, for client section updates.
    pub sections: Vec<(usize, Vec<PackedSectionBlockPos>)>,
    /// Positions whose light properties changed.
    pub light: Vec<BlockPos>,
    /// Sections whose emptiness flipped.
    pub emptiness: Vec<LightSectionEmptinessChange>,
    /// Changes whose point-of-interest type differs (old, new).
    pub poi: Vec<(BlockPos, BlockStateId, BlockStateId)>,
    /// Replaced states and how often each was replaced.
    pub replaced: FxHashMap<BlockStateId, u64>,
    /// Cells left as they were.
    pub unchanged: u64,
}

struct SectionChange {
    local: (usize, usize, usize),
    pos: BlockPos,
    old: BlockStateId,
    new: BlockStateId,
}

impl FullChunkRef<'_> {
    /// Writes every cell of the inclusive box `min..=max` (inside this chunk) for which
    /// `decide(pos, old)` returns a new state.
    pub(crate) fn fill_box(
        &self,
        min: BlockPos,
        max: BlockPos,
        decide: &mut dyn FnMut(BlockPos, BlockStateId) -> Option<BlockStateId>,
        out: &mut ChunkFillOutput,
    ) {
        let min_y = min.0.y.max(self.min_y());
        let max_y = max.0.y.min(self.min_y() + self.height() - 1);
        if min_y > max_y {
            return;
        }
        let mut changed_any = false;
        for section_index in self.get_section_index(min_y)..=self.get_section_index(max_y) {
            let section_min_y = self.min_y() + i32::try_from(section_index * 16).unwrap_or(0);
            let low = min_y.max(section_min_y);
            let high = max_y.min(section_min_y + 15);
            let changes = self.fill_section(section_index, (min, max), (low, high), decide, out);
            if changes.is_empty() {
                continue;
            }
            changed_any = true;
            self.after_section_fill(section_index, &changes, out);
        }
        if changed_any {
            self.mark_unsaved();
        }
    }

    /// Palette writes for one section under a single write lock.
    fn fill_section(
        &self,
        section_index: usize,
        (min, max): (BlockPos, BlockPos),
        (low_y, high_y): (i32, i32),
        decide: &mut dyn FnMut(BlockPos, BlockStateId) -> Option<BlockStateId>,
        out: &mut ChunkFillOutput,
    ) -> Vec<SectionChange> {
        let section = &self.chunk.sections.sections[section_index];
        let mut detached: Vec<(BlockPos, DetachedBlockEntity)> = Vec::new();
        let mut changes = Vec::new();
        let (was_empty, is_empty) = {
            let mut guard = section.write();
            let was_empty = guard.is_empty();
            for y in low_y..=high_y {
                let local_y = (y & 15) as usize;
                for z in min.0.z..=max.0.z {
                    let local_z = (z & 15) as usize;
                    for x in min.0.x..=max.0.x {
                        let local_x = (x & 15) as usize;
                        let pos = BlockPos::new(x, y, z);
                        let old = guard.states().get(local_x, local_y, local_z);
                        let Some(new) = decide(pos, old) else {
                            continue;
                        };
                        if new == old {
                            out.unchanged += 1;
                            continue;
                        }
                        // The per-block path asks the new block whether to keep the entity;
                        // bulk fills replace whole regions, so a changed block always drops it.
                        if old.has_block_entity() && old.get_block() != new.get_block() {
                            detached.push((
                                pos,
                                self.chunk
                                    .block_entity_storage()
                                    .detach_and_queue_removal(pos),
                            ));
                        }
                        guard.set_block_state(local_x, local_y, local_z, new);
                        *out.replaced.entry(old).or_default() += 1;
                        changes.push(SectionChange {
                            local: (local_x, local_y, local_z),
                            pos,
                            old,
                            new,
                        });
                    }
                }
            }
            (was_empty, guard.is_empty())
        };
        if was_empty != is_empty {
            let section_y = SectionPos::block_to_section_coord(low_y);
            self.chunk
                .light
                .write()
                .set_section_empty(section_y, is_empty);
            out.emptiness.push(LightSectionEmptinessChange {
                section_pos: SectionPos::new(self.chunk.pos.0.x, section_y, self.chunk.pos.0.y),
                empty: is_empty,
            });
        }
        for (
            pos,
            DetachedBlockEntity {
                entity,
                dispatch_removed,
            },
        ) in detached
        {
            let Some(block_entity) = entity else {
                continue;
            };
            let mut dispatchers = LifecycleDispatchers::new();
            if dispatch_removed {
                dispatchers.push(block_entity);
            }
            self.finish_block_entity_change(pos, dispatchers);
        }
        changes
    }

    /// Derived chunk state for one section's changes, without any section lock held.
    fn after_section_fill(
        &self,
        section_index: usize,
        changes: &[SectionChange],
        out: &mut ChunkFillOutput,
    ) {
        let min_y = self.min_y();
        let sections = &self.chunk.sections;
        {
            let mut heightmaps = self.chunk.heightmaps.write();
            for change in changes {
                let (x, _, z) = change.local;
                heightmaps.update_final(x, change.pos.0.y, z, change.new, |lx, scan_y, lz| {
                    let scan_section = ((scan_y - min_y) / 16) as usize;
                    let scan_local_y = ((scan_y - min_y) % 16) as usize;
                    sections.sections[scan_section]
                        .read()
                        .states()
                        .get(lx, scan_local_y, lz)
                });
            }
        }
        let mut packed = Vec::with_capacity(changes.len());
        for change in changes {
            packed.push(SectionPos::section_relative_pos(change.pos));
            if has_different_light_properties(change.old, change.new) {
                let (x, _, z) = change.local;
                self.update_sky_light_sources(x, change.pos.0.y, z);
                out.light.push(change.pos);
            }
            if change.new.has_block_entity() {
                self.reconcile_block_entity_after_set(change.pos, change.new);
            }
            if REGISTRY.poi_types.type_id_for_state(change.old)
                != REGISTRY.poi_types.type_id_for_state(change.new)
            {
                out.poi.push((change.pos, change.old, change.new));
            }
        }
        out.sections.push((section_index, packed));
    }
}
