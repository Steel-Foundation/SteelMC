use std::{mem, sync::Arc};

use parking_lot::{RwLockReadGuard, RwLockWriteGuard};
use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::{BlockStateId, ChunkPos, SectionPos};

use crate::chunk::{
    chunk_access::{ChunkAccess, ChunkStatus},
    chunk_holder::ChunkHolder,
    section::ChunkSection,
};

use super::{
    CachedLightBlock, CachedLightChunk, ChunkLightData, ChunkLightLayerStorage,
    LightCacheChunkScope, LightCacheLayout, LightCacheSetupRadius, LightChunkSlotArray, LightLayer,
    LightSection, LightSectionData, LightSectionSlotArray, LightUpdateNotificationCache,
};

/// Error returned when a scoped light workset cannot acquire required chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightWorksetSetupError {
    /// A chunk inside `ScalableLux`'s required 1-radius cache was unavailable.
    MissingRequiredChunk {
        /// Missing chunk position.
        chunk_pos: ChunkPos,
    },
}

/// Scoped chunk admission for one light operation.
///
/// This keeps the `ScalableLux` cache-window admission rules without storing
/// long-lived borrows into chunk internals. The workset pins admitted chunk
/// holders, then builds short-lived read caches with locks acquired in stable
/// cache-slot order.
pub struct LightWorkset {
    layout: LightCacheLayout,
    chunks: LightChunkSlotArray<LightWorksetChunk>,
}

struct LightWorksetChunk {
    holder: Arc<ChunkHolder>,
    section_readable: bool,
    light_writable: bool,
}

impl LightWorkset {
    /// Creates a scoped cache window by scanning chunks in `ScalableLux` setup order.
    pub fn setup(
        layout: LightCacheLayout,
        radius: LightCacheSetupRadius,
        relaxed: bool,
        mut chunk_for_lighting: impl FnMut(ChunkPos) -> Option<Arc<ChunkHolder>>,
        mut can_use_chunk: impl FnMut(&ChunkAccess) -> bool,
    ) -> Result<Self, LightWorksetSetupError> {
        Self::setup_with_scopes(
            layout,
            radius,
            relaxed,
            &mut chunk_for_lighting,
            |_, _, chunk| {
                let usable = can_use_chunk(chunk);
                (usable, usable)
            },
        )
    }

    /// Creates a scoped cache window with separate section-read and light-write admission.
    pub fn setup_with_scopes(
        layout: LightCacheLayout,
        radius: LightCacheSetupRadius,
        relaxed: bool,
        mut chunk_for_lighting: impl FnMut(ChunkPos) -> Option<Arc<ChunkHolder>>,
        mut can_use_chunk: impl FnMut(CachedLightChunk, &ChunkHolder, &ChunkAccess) -> (bool, bool),
    ) -> Result<Self, LightWorksetSetupError> {
        let mut chunks = LightChunkSlotArray::new();

        for cached_chunk in layout.setup_chunks(radius) {
            let Some(holder) =
                Self::try_get_holder(cached_chunk, relaxed, &mut chunk_for_lighting)?
            else {
                continue;
            };

            let Some(chunk) = holder.try_chunk(ChunkStatus::Empty) else {
                continue;
            };
            let (section_readable, light_writable) = can_use_chunk(cached_chunk, &holder, &chunk);
            if !section_readable && !light_writable {
                continue;
            }
            drop(chunk);

            chunks.insert(
                cached_chunk,
                LightWorksetChunk {
                    holder,
                    section_readable,
                    light_writable,
                },
            );
        }

        Ok(Self { layout, chunks })
    }

    /// Returns this workset's cache layout.
    #[must_use]
    pub const fn layout(&self) -> LightCacheLayout {
        self.layout
    }

    /// Returns the holder for a cached chunk slot.
    #[must_use]
    pub fn chunk_holder(&self, cached_chunk: CachedLightChunk) -> Option<&Arc<ChunkHolder>> {
        self.chunks.get(cached_chunk).map(|chunk| &chunk.holder)
    }

    /// Returns whether a cached chunk was admitted for section reads.
    #[must_use]
    pub fn can_read_sections(&self, cached_chunk: CachedLightChunk) -> bool {
        self.chunks
            .get(cached_chunk)
            .is_some_and(|chunk| chunk.section_readable)
    }

    /// Returns whether a cached chunk was admitted for light writes.
    #[must_use]
    pub fn can_write_light(&self, cached_chunk: CachedLightChunk) -> bool {
        self.chunks
            .get(cached_chunk)
            .is_some_and(|chunk| chunk.light_writable)
    }

    /// Builds a chunk-read cache for the duration of `f`.
    ///
    /// Chunk locks are acquired in cache-slot order and released before this
    /// method returns. The workset keeps holder `Arc`s alive, while this cache
    /// keeps guarded chunk data stable during the scoped operation.
    pub fn with_chunk_read_cache<R>(&self, f: impl FnOnce(&LightChunkReadCache<'_>) -> R) -> R {
        let mut chunks = LightChunkSlotArray::new();
        let mut light_chunks = LightChunkSlotArray::new();

        for chunk_slot in 0..self.chunks.slot_count() {
            let Some(workset_chunk) = self.chunks.get_slot(chunk_slot) else {
                continue;
            };
            if workset_chunk.section_readable
                && let Some(chunk) = workset_chunk.holder.try_chunk(ChunkStatus::Empty)
            {
                chunks.insert_slot(chunk_slot, chunk);
            }
            if workset_chunk.light_writable
                && let Some(chunk) = workset_chunk.holder.try_chunk(ChunkStatus::Empty)
            {
                light_chunks.insert_slot(chunk_slot, chunk);
            }
        }

        let cache = LightChunkReadCache {
            layout: self.layout,
            chunks,
            light_chunks,
        };
        f(&cache)
    }

    fn try_get_holder(
        cached_chunk: CachedLightChunk,
        relaxed: bool,
        chunk_for_lighting: &mut impl FnMut(ChunkPos) -> Option<Arc<ChunkHolder>>,
    ) -> Result<Option<Arc<ChunkHolder>>, LightWorksetSetupError> {
        let required = !relaxed && cached_chunk.scope == LightCacheChunkScope::Inner;
        let holder = chunk_for_lighting(cached_chunk.chunk_pos)
            .filter(|holder| holder.try_chunk(ChunkStatus::Empty).is_some());

        if holder.is_none() && required {
            return Err(LightWorksetSetupError::MissingRequiredChunk {
                chunk_pos: cached_chunk.chunk_pos,
            });
        }

        Ok(holder)
    }
}

/// Flat cached chunk reads for one scoped lighting operation.
pub struct LightChunkReadCache<'a> {
    layout: LightCacheLayout,
    chunks: LightChunkSlotArray<RwLockReadGuard<'a, ChunkAccess>>,
    light_chunks: LightChunkSlotArray<RwLockReadGuard<'a, ChunkAccess>>,
}

impl LightChunkReadCache<'_> {
    /// Returns this read cache's layout.
    #[must_use]
    pub const fn layout(&self) -> LightCacheLayout {
        self.layout
    }

    /// Returns the cached chunk for a chunk slot.
    #[must_use]
    pub fn chunk(&self, cached_chunk: CachedLightChunk) -> Option<&ChunkAccess> {
        self.chunks.get(cached_chunk).map(|chunk| &**chunk)
    }

    /// Builds a section-read cache for the duration of `f`.
    ///
    /// Section locks are acquired in cache-slot order and released before this
    /// method returns. Emptiness maps are copied into the cache so propagation
    /// can query known section emptiness without keeping additional borrows.
    pub fn with_section_read_cache<R>(&self, f: impl FnOnce(&LightSectionReadCache<'_>) -> R) -> R {
        let mut sections = LightSectionSlotArray::new(self.layout);
        let mut emptiness_maps = LightChunkSlotArray::new();

        for chunk_slot in 0..self.chunks.slot_count() {
            let Some(chunk_guard) = self.chunks.get_slot(chunk_slot) else {
                continue;
            };
            let Some(chunk_pos) = self.layout.chunk_pos_for_slot(chunk_slot) else {
                continue;
            };

            let chunk_sections = chunk_guard.sections();
            emptiness_maps.insert_slot(chunk_slot, chunk_sections.section_emptiness_map());

            let Some(section_slots) = self.layout.inner_light_section_slots_for_chunk(chunk_pos)
            else {
                continue;
            };

            for cached_section in section_slots {
                let Some(section_index) = self
                    .layout
                    .range()
                    .chunk_section_index(cached_section.section_pos.y())
                else {
                    continue;
                };
                let Some(section) = chunk_sections.sections.get(section_index) else {
                    continue;
                };
                sections.insert(cached_section, section.read());
            }
        }

        let cache = LightSectionReadCache {
            layout: self.layout,
            sections,
            emptiness_maps,
        };
        f(&cache)
    }

    /// Builds a layer-specific light edit cache for the duration of `f`.
    ///
    /// Committed chunk light storage is copied into the edit cache before
    /// propagation mutates it. The edit writes back only through
    /// [`LightLayerEdit::commit`], so chunk-owned light does not regain the old
    /// persistent visible/updating split.
    pub fn with_light_edit<R>(
        &self,
        layer: LightLayer,
        f: impl FnOnce(LightLayerEdit<'_>) -> R,
    ) -> R {
        let mut chunks = LightChunkSlotArray::new();

        for chunk_slot in 0..self.light_chunks.slot_count() {
            let Some(chunk_guard) = self.light_chunks.get_slot(chunk_slot) else {
                continue;
            };
            chunks.insert_slot(chunk_slot, chunk_guard.light_mut());
        }

        let mut edits = Vec::new();
        let sections = LightLayerEdit::build_section_edits(self.layout, layer, &chunks, &mut edits);
        let edit = LightLayerEdit {
            layout: self.layout,
            layer,
            chunks,
            sections,
            removed_missing_sections: LightSectionSlotArray::new(self.layout),
            edits,
        };
        f(edit)
    }
}

/// Flat cached chunk-section reads for block-state access during lighting.
pub struct LightSectionReadCache<'a> {
    layout: LightCacheLayout,
    sections: LightSectionSlotArray<RwLockReadGuard<'a, ChunkSection>>,
    emptiness_maps: LightChunkSlotArray<Box<[bool]>>,
}

impl LightSectionReadCache<'_> {
    /// Returns this read cache's layout.
    #[must_use]
    pub const fn layout(&self) -> LightCacheLayout {
        self.layout
    }

    /// Returns the block state for a cached light block, or air for missing sections.
    #[must_use]
    pub fn get_block_state(&self, cached_block: CachedLightBlock) -> BlockStateId {
        let Some(section) = self.sections.get_slot(cached_block.section_slot) else {
            return Self::air();
        };

        if section.is_empty() {
            return Self::air();
        }

        let (local_x, local_y, local_z) = local_block_coords(cached_block.local_index);
        section.states.get(local_x, local_y, local_z)
    }

    /// Returns whether a cached section exists and is non-empty.
    #[must_use]
    pub fn has_non_empty_section(&self, section_pos: SectionPos) -> bool {
        let Some(cached_section) = self.layout.cached_section(section_pos) else {
            return false;
        };
        self.sections
            .get_slot(cached_section.section_slot)
            .is_some_and(|section| !section.is_empty())
    }

    /// Returns whether a cached section was admitted into the section-read cache.
    #[must_use]
    pub fn has_cached_section(&self, section_pos: SectionPos) -> bool {
        let Some(cached_section) = self.layout.cached_section(section_pos) else {
            return false;
        };
        self.sections
            .get_slot(cached_section.section_slot)
            .is_some()
    }

    /// Returns known real-section emptiness for a readable cached chunk column.
    #[must_use]
    pub fn section_empty(&self, section_pos: SectionPos) -> Option<bool> {
        let chunk_pos = ChunkPos::new(section_pos.x(), section_pos.z());
        let cached_chunk = self.layout.cached_chunk(chunk_pos)?;
        let emptiness_map = self.emptiness_maps.get_slot(cached_chunk.chunk_slot)?;
        let section_index = self.layout.range().chunk_section_index(section_pos.y())?;
        emptiness_map.get(section_index).copied()
    }

    fn air() -> BlockStateId {
        REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR)
    }
}

const fn local_block_coords(local_index: usize) -> (usize, usize, usize) {
    let local_x = local_index & 15;
    let local_z = (local_index >> 4) & 15;
    let local_y = (local_index >> 8) & 15;
    (local_x, local_y, local_z)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StoredLightSectionEdit {
    chunk_slot: usize,
    section_index: usize,
}

#[derive(Debug, PartialEq, Eq)]
struct LightSectionEdit {
    section: LightSection,
    dirty: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LightSectionEditEntry {
    Stored {
        target: StoredLightSectionEdit,
        edit_index: usize,
    },
    Transient {
        edit_index: usize,
    },
}

/// Scoped mutable light edits for one layer and one workset.
pub struct LightLayerEdit<'a> {
    layout: LightCacheLayout,
    layer: LightLayer,
    chunks: LightChunkSlotArray<RwLockWriteGuard<'a, ChunkLightData>>,
    sections: LightSectionSlotArray<LightSectionEditEntry>,
    removed_missing_sections: LightSectionSlotArray<StoredLightSectionEdit>,
    edits: Vec<LightSectionEdit>,
}

impl LightLayerEdit<'_> {
    /// Returns this edit cache's layout.
    #[must_use]
    pub const fn layout(&self) -> LightCacheLayout {
        self.layout
    }

    /// Returns this edit cache's light layer.
    #[must_use]
    pub const fn layer(&self) -> LightLayer {
        self.layer
    }

    /// Returns an edited light value for a cached light block.
    #[must_use]
    pub fn get(&self, cached_block: CachedLightBlock) -> u8 {
        self.get_at_section_index(cached_block.section_slot, cached_block.local_index)
    }

    /// Returns whether a cached block has a non-missing edited section.
    #[must_use]
    pub fn has_non_missing(&self, cached_block: CachedLightBlock) -> bool {
        self.section_edit(cached_block.section_slot)
            .is_some_and(|section| !matches!(section.section, LightSection::Missing))
    }

    /// Returns whether a cached section has a non-missing edited section.
    #[must_use]
    pub fn has_non_missing_section(&self, section_pos: SectionPos) -> bool {
        let Some(section_slot) = self.layout.section_slot(section_pos) else {
            return false;
        };
        self.section_edit(section_slot)
            .is_some_and(|section| !matches!(section.section, LightSection::Missing))
    }

    /// Returns whether a cached section has an edited missing section.
    #[must_use]
    pub fn is_section_missing(&self, section_pos: SectionPos) -> bool {
        let Some(section_slot) = self.layout.section_slot(section_pos) else {
            return false;
        };
        self.section_edit(section_slot)
            .is_some_and(|section| matches!(section.section, LightSection::Missing))
    }

    /// Returns whether a cached section was admitted into the edit cache.
    #[must_use]
    pub fn has_cached_section(&self, section_pos: SectionPos) -> bool {
        let Some(section_slot) = self.layout.section_slot(section_pos) else {
            return false;
        };
        self.sections.get_slot(section_slot).is_some()
    }

    /// Returns true when a cached section has edited light data.
    #[must_use]
    pub fn has_light_data_section(&self, section_pos: SectionPos) -> bool {
        let Some(section_slot) = self.layout.section_slot(section_pos) else {
            return false;
        };
        self.section_edit(section_slot)
            .is_some_and(|section| section_has_light_data(&section.section))
    }

    /// Returns known real-section emptiness for a writable cached chunk column.
    #[must_use]
    pub fn section_empty(&self, section_pos: SectionPos) -> Option<bool> {
        let chunk_pos = ChunkPos::new(section_pos.x(), section_pos.z());
        let cached_chunk = self.layout.cached_chunk(chunk_pos)?;
        let light_data = self.chunks.get_slot(cached_chunk.chunk_slot)?;

        Self::layer_storage(light_data, self.layer).section_empty(section_pos.y())
    }

    /// Updates the cached light layer's real-section emptiness map.
    ///
    /// Returns the previous value when the target layer and section are writable.
    pub fn set_section_empty(&mut self, section_pos: SectionPos, empty: bool) -> Option<bool> {
        let chunk_pos = ChunkPos::new(section_pos.x(), section_pos.z());
        let cached_chunk = self.layout.cached_chunk(chunk_pos)?;
        let layer = self.layer;
        let light_data = self.chunks.get_mut_slot(cached_chunk.chunk_slot)?;

        Self::layer_storage_mut(light_data, layer).set_section_empty(section_pos.y(), empty)
    }

    /// Marks a cached light section non-missing without allocating packed bytes.
    ///
    /// Returns false when the section has no writable cached edit entry.
    pub fn set_section_non_missing(&mut self, section_pos: SectionPos) -> bool {
        let Some(section_slot) = self.layout.section_slot(section_pos) else {
            return false;
        };
        self.set_section_slot_non_missing(section_slot)
    }

    /// Marks a cached light section missing and drops edited bytes.
    ///
    /// Returns false when the section has no writable cached edit entry.
    pub fn set_section_missing(&mut self, section_pos: SectionPos) -> bool {
        let Some(section_slot) = self.layout.section_slot(section_pos) else {
            return false;
        };
        let Some(section) = self.section_edit_mut(section_slot) else {
            return false;
        };
        let was_present = !matches!(section.section, LightSection::Missing);
        section.section = LightSection::missing();
        section.dirty |= was_present;
        was_present
    }

    /// Hides a cached section from external packet/save conversion.
    ///
    /// Missing and visible zero sections become missing, matching old
    /// `Uninitialized -> Null` hidden-state behavior.
    pub fn set_section_internal(&mut self, section_pos: SectionPos) -> bool {
        let Some(section_slot) = self.layout.section_slot(section_pos) else {
            return false;
        };
        let Some(section) = self.section_edit_mut(section_slot) else {
            return false;
        };
        let was_present = !matches!(section.section, LightSection::Missing);
        section.section = take_internal_section(&mut section.section);
        section.dirty |= was_present;
        was_present
    }

    /// Replaces one cached chunk column's layer sections with fresh missing sections.
    ///
    /// Initial chunk lighting lights into a fresh center layer, so previous
    /// neighbor-written data cannot become the center chunk's canonical light.
    pub fn reset_chunk_sections_to_missing(&mut self, chunk_pos: ChunkPos) -> bool {
        let Some(cached_chunk) = self.layout.cached_chunk(chunk_pos) else {
            return false;
        };
        if self.chunks.get_slot(cached_chunk.chunk_slot).is_none() {
            return false;
        }

        let mut reset_any = false;
        for section_y in
            self.layout.range().min_section_y()..self.layout.range().max_section_y_exclusive()
        {
            let section_pos = SectionPos::new(chunk_pos.0.x, section_y, chunk_pos.0.y);
            if self.set_section_missing(section_pos) {
                reset_any = true;
            }
        }
        reset_any
    }

    /// Removes missing sky sections from the temporary edit cache.
    ///
    /// Later materialization can create transient sections for propagation; those
    /// transient sections notify on commit but do not write into chunk storage.
    pub fn rewrite_missing_sections_for_skylight(&mut self) {
        debug_assert_eq!(self.layer, LightLayer::Sky);

        for section_slot in 0..self.sections.slot_count() {
            let Some(LightSectionEditEntry::Stored { target, edit_index }) =
                self.sections.get_slot(section_slot).copied()
            else {
                continue;
            };
            if !matches!(
                self.edits.get(edit_index).map(|edit| &edit.section),
                Some(LightSection::Missing)
            ) {
                continue;
            }

            self.sections.take_slot(section_slot);
            self.removed_missing_sections
                .insert_slot(section_slot, target);
        }
    }

    /// Materializes a sky section that was removed from the temporary edit cache.
    ///
    /// Returns false when the section was not part of the writable cache or was
    /// not removed by [`Self::rewrite_missing_sections_for_skylight`].
    pub fn materialize_removed_missing_section(&mut self, section_pos: SectionPos) -> bool {
        let Some(section_slot) = self.layout.section_slot(section_pos) else {
            return false;
        };
        if self.sections.get_slot(section_slot).is_some() {
            return true;
        }
        if self
            .removed_missing_sections
            .get_slot(section_slot)
            .is_none()
        {
            return false;
        }

        let edit_index = self.edits.len();
        self.edits.push(LightSectionEdit {
            section: LightSection::missing(),
            dirty: false,
        });
        self.sections.insert_slot(
            section_slot,
            LightSectionEditEntry::Transient { edit_index },
        );
        true
    }

    /// Fills a cached section with one edited light value.
    ///
    /// Returns false when the section has no writable cached edit entry.
    pub fn fill_section(&mut self, section_pos: SectionPos, value: u8) -> bool {
        let Some(section_slot) = self.layout.section_slot(section_pos) else {
            return false;
        };
        let Some(section) = self.section_edit_mut(section_slot) else {
            return false;
        };

        fill_section(&mut section.section, value);
        section.dirty = true;
        true
    }

    /// Extrudes the lower row from the first non-missing cached section above.
    ///
    /// Returns false when the target section or source section is unavailable.
    pub fn extrude_lower_from_first_section_above(&mut self, section_pos: SectionPos) -> bool {
        let Some(target_slot) = self.layout.section_slot(section_pos) else {
            return false;
        };

        let mut source_row = None;
        for source_y in (section_pos.y() + 1)..self.layout.range().max_section_y_exclusive() {
            let source_pos = SectionPos::new(section_pos.x(), source_y, section_pos.z());
            let Some(source_slot) = self.layout.section_slot(source_pos) else {
                continue;
            };
            let Some(source) = self.section_edit(source_slot) else {
                continue;
            };
            if matches!(source.section, LightSection::Missing) {
                continue;
            }
            source_row = Some(lower_row(&source.section));
            break;
        }

        let Some(source_row) = source_row else {
            return false;
        };
        let Some(target) = self.section_edit_mut(target_slot) else {
            return false;
        };
        extrude_lower_row(&mut target.section, source_row.as_ref());
        target.dirty = true;
        true
    }

    /// Returns an edited light value for a section slot and local light index.
    #[must_use]
    pub fn get_at_section_index(&self, section_slot: usize, local_index: usize) -> u8 {
        let Some(section) = self.section_edit(section_slot) else {
            return 0;
        };
        get_section_value(&section.section, local_index)
    }

    /// Sets an edited light value for a cached light block.
    ///
    /// Returns false when no writable non-missing section was cached for the block.
    pub fn set(&mut self, cached_block: CachedLightBlock, level: u8) -> bool {
        self.set_at_section_index(cached_block.section_slot, cached_block.local_index, level)
    }

    /// Sets an edited light value for a section slot and local light index.
    ///
    /// Returns false when no writable non-missing section was cached for the slot.
    pub fn set_at_section_index(
        &mut self,
        section_slot: usize,
        local_index: usize,
        level: u8,
    ) -> bool {
        let Some(section) = self.section_edit_mut(section_slot) else {
            return false;
        };
        if matches!(section.section, LightSection::Missing) {
            return false;
        }

        set_section_value(&mut section.section, local_index, level);
        section.dirty = true;
        true
    }

    /// Commits edited stored sections and publishes changed or notified sections.
    pub fn commit(
        mut self,
        notifications: Option<&LightUpdateNotificationCache>,
        mut on_update: impl FnMut(SectionPos),
    ) -> usize {
        debug_assert!(notifications.is_none_or(|cache| cache.layout() == self.layout));
        let mut updated = 0;

        for section_slot in 0..self.sections.slot_count() {
            let marked =
                notifications.is_some_and(|cache| cache.is_marked_section_slot(section_slot));
            let Some(entry) = self.sections.take_slot(section_slot) else {
                continue;
            };

            let changed = match entry {
                LightSectionEditEntry::Stored { target, edit_index } => {
                    self.commit_stored_section(target, edit_index)
                }
                LightSectionEditEntry::Transient { edit_index } => self
                    .edits
                    .get(edit_index)
                    .is_some_and(|section| section.dirty),
            };

            if (changed || marked)
                && let Some(section_pos) = self.layout.section_pos_for_slot(section_slot)
            {
                on_update(section_pos);
                updated += 1;
            }
        }

        updated
    }

    fn build_section_edits(
        layout: LightCacheLayout,
        layer: LightLayer,
        chunks: &LightChunkSlotArray<RwLockWriteGuard<'_, ChunkLightData>>,
        edits: &mut Vec<LightSectionEdit>,
    ) -> LightSectionSlotArray<LightSectionEditEntry> {
        let mut sections = LightSectionSlotArray::new(layout);

        for chunk_slot in 0..chunks.slot_count() {
            let Some(light_data) = chunks.get_slot(chunk_slot) else {
                continue;
            };
            let Some(chunk_pos) = layout.chunk_pos_for_slot(chunk_slot) else {
                continue;
            };
            let Some(section_slots) = layout.inner_light_section_slots_for_chunk(chunk_pos) else {
                continue;
            };

            let layer_storage = Self::layer_storage(light_data, layer);
            for cached_section in section_slots {
                let Some(section_index) = layer_storage
                    .range()
                    .section_index(cached_section.section_pos.y())
                else {
                    continue;
                };
                let Some(section) = layer_storage.sections().get(section_index) else {
                    continue;
                };

                let edit_index = edits.len();
                edits.push(LightSectionEdit {
                    section: copy_light_section(section),
                    dirty: false,
                });
                sections.insert(
                    cached_section,
                    LightSectionEditEntry::Stored {
                        target: StoredLightSectionEdit {
                            chunk_slot,
                            section_index,
                        },
                        edit_index,
                    },
                );
            }
        }

        sections
    }

    fn commit_stored_section(&mut self, target: StoredLightSectionEdit, edit_index: usize) -> bool {
        let Some(edit) = self.edits.get_mut(edit_index) else {
            return false;
        };
        let edited = mem::replace(&mut edit.section, LightSection::missing());
        let layer = self.layer;
        let Some(light_data) = self.chunks.get_mut_slot(target.chunk_slot) else {
            return false;
        };
        let Some(target_section) = Self::layer_storage_mut(light_data, layer)
            .sections_mut()
            .get_mut(target.section_index)
        else {
            return false;
        };

        if *target_section == edited {
            return false;
        }

        *target_section = edited;
        true
    }

    fn set_section_slot_non_missing(&mut self, section_slot: usize) -> bool {
        let Some(section) = self.section_edit_mut(section_slot) else {
            return false;
        };

        let was_missing = matches!(section.section, LightSection::Missing);
        set_section_non_missing(&mut section.section);
        section.dirty |= was_missing;
        was_missing
    }

    fn section_edit(&self, section_slot: usize) -> Option<&LightSectionEdit> {
        let entry = self.sections.get_slot(section_slot)?;
        match entry {
            LightSectionEditEntry::Stored { edit_index, .. }
            | LightSectionEditEntry::Transient { edit_index } => self.edits.get(*edit_index),
        }
    }

    fn section_edit_mut(&mut self, section_slot: usize) -> Option<&mut LightSectionEdit> {
        let entry = self.sections.get_slot(section_slot)?;
        match entry {
            LightSectionEditEntry::Stored { edit_index, .. }
            | LightSectionEditEntry::Transient { edit_index } => self.edits.get_mut(*edit_index),
        }
    }

    const fn layer_storage(
        light_data: &ChunkLightData,
        layer: LightLayer,
    ) -> &ChunkLightLayerStorage {
        match layer {
            LightLayer::Sky => &light_data.sky,
            LightLayer::Block => &light_data.block,
        }
    }

    const fn layer_storage_mut(
        light_data: &mut ChunkLightData,
        layer: LightLayer,
    ) -> &mut ChunkLightLayerStorage {
        match layer {
            LightLayer::Sky => &mut light_data.sky,
            LightLayer::Block => &mut light_data.block,
        }
    }
}

fn copy_light_section(section: &LightSection) -> LightSection {
    match section {
        LightSection::Missing => LightSection::missing(),
        LightSection::Visible(data) => LightSection::visible(copy_light_section_data(data)),
        LightSection::Internal(data) => LightSection::internal(copy_light_section_data(data)),
    }
}

fn copy_light_section_data(data: &LightSectionData) -> LightSectionData {
    match data {
        LightSectionData::Homogeneous(value) => LightSectionData::homogeneous(*value),
        LightSectionData::Packed(data) => LightSectionData::Packed(Box::new(**data)),
    }
}

fn set_section_non_missing(section: &mut LightSection) {
    match section {
        LightSection::Missing => {
            *section = LightSection::visible(LightSectionData::homogeneous(0));
        }
        LightSection::Visible(_) => {}
        LightSection::Internal(data) => {
            *section = LightSection::visible(mem::replace(data, LightSectionData::homogeneous(0)));
        }
    }
}

fn take_internal_section(section: &mut LightSection) -> LightSection {
    match mem::replace(section, LightSection::missing()) {
        LightSection::Missing | LightSection::Visible(LightSectionData::Homogeneous(0)) => {
            LightSection::missing()
        }
        LightSection::Visible(data) | LightSection::Internal(data) => LightSection::internal(data),
    }
}

fn fill_section(section: &mut LightSection, value: u8) {
    match section {
        LightSection::Missing => {
            *section = LightSection::visible(LightSectionData::homogeneous(value));
        }
        LightSection::Visible(data) | LightSection::Internal(data) => data.fill(value),
    }
}

fn get_section_value(section: &LightSection, local_index: usize) -> u8 {
    let data = match section {
        LightSection::Missing => return 0,
        LightSection::Visible(data) | LightSection::Internal(data) => data,
    };
    let (local_x, local_y, local_z) = local_block_coords(local_index);
    data.get(local_x, local_y, local_z)
}

fn set_section_value(section: &mut LightSection, local_index: usize, level: u8) {
    let data = match section {
        LightSection::Missing => return,
        LightSection::Visible(data) | LightSection::Internal(data) => data,
    };
    let (local_x, local_y, local_z) = local_block_coords(local_index);
    data.set(local_x, local_y, local_z, level);
}

const fn section_has_light_data(section: &LightSection) -> bool {
    match section {
        LightSection::Missing | LightSection::Visible(LightSectionData::Homogeneous(0)) => false,
        LightSection::Visible(_) | LightSection::Internal(_) => true,
    }
}

fn lower_row(section: &LightSection) -> Option<[u8; 16 * 16]> {
    let data = match section {
        LightSection::Missing => return None,
        LightSection::Visible(data) | LightSection::Internal(data) => data,
    };

    if let LightSectionData::Homogeneous(0) = data {
        return None;
    }

    let mut row = [0; 16 * 16];
    for z in 0..16 {
        for x in 0..16 {
            row[z * 16 + x] = data.get(x, 0, z);
        }
    }
    Some(row)
}

fn extrude_lower_row(section: &mut LightSection, row: Option<&[u8; 16 * 16]>) {
    let Some(row) = row else {
        *section = LightSection::visible(LightSectionData::homogeneous(0));
        return;
    };

    if matches!(section, LightSection::Missing) {
        *section = LightSection::visible(LightSectionData::homogeneous(0));
    }

    for y in 0..16 {
        for z in 0..16 {
            for x in 0..16 {
                set_section_value(section, x | (z << 4) | (y << 8), row[z * 16 + x]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use steel_registry::{test_support::init_test_registry, vanilla_blocks};
    use steel_utils::{BlockPos, SectionPos};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::chunk::{
        chunk_access::ChunkAccess,
        chunk_ticket_manager::ChunkTicketLevel,
        proto_chunk::ProtoChunk,
        section::{ChunkSection, Sections},
    };

    fn init_tests() {
        init_test_registry();
        init_behaviors();
    }

    fn range() -> super::super::LightSectionRange {
        let Ok(range) = super::super::LightSectionRange::from_world_height(0, 16) else {
            panic!("test height should create a valid light range");
        };
        range
    }

    fn holder_with_section(pos: ChunkPos, section: ChunkSection) -> Arc<ChunkHolder> {
        let sections = Sections::from_owned(vec![section].into_boxed_slice());
        let proto = ProtoChunk::new(sections, pos, 0, 16, Weak::new());
        let holder = Arc::new(ChunkHolder::new(
            pos,
            ChunkTicketLevel::FULL_CHUNK,
            Some(ChunkTicketLevel::FULL_CHUNK),
            0,
            16,
        ));
        holder.insert_chunk(ChunkAccess::Proto(proto), ChunkStatus::Light);
        holder
    }

    fn set_light_section(
        holder: &ChunkHolder,
        layer: LightLayer,
        section_y: i32,
        section: LightSection,
    ) {
        let Some(chunk) = holder.try_chunk(ChunkStatus::Empty) else {
            panic!("test chunk should be available");
        };
        let mut light = chunk.light_mut();
        let storage = match layer {
            LightLayer::Sky => &mut light.sky,
            LightLayer::Block => &mut light.block,
        };
        let Some(target) = storage.section_mut(section_y) else {
            panic!("test section should be inside light range");
        };
        *target = section;
    }

    #[test]
    fn workset_pins_cached_chunk_holder_until_dropped() {
        init_tests();
        let center = ChunkPos::new(0, 0);
        let holder = holder_with_section(center, ChunkSection::new_empty());
        let layout = LightCacheLayout::new(center, range());

        let Ok(workset) = LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Full,
            true,
            |pos| (pos == center).then(|| Arc::clone(&holder)),
            |_| true,
        ) else {
            panic!("relaxed setup should accept missing optional chunks");
        };

        let Some(cached_center) = layout.cached_chunk(center) else {
            panic!("center chunk should be inside the cache");
        };
        assert_eq!(workset.layout(), layout);
        assert!(workset.chunk_holder(cached_center).is_some());
        assert!(workset.can_read_sections(cached_center));
        assert!(workset.can_write_light(cached_center));
        assert_eq!(Arc::strong_count(&holder), 2);

        drop(workset);
        assert_eq!(Arc::strong_count(&holder), 1);
    }

    #[test]
    fn workset_reports_missing_required_inner_chunk() {
        init_tests();
        let layout = LightCacheLayout::new(ChunkPos::new(0, 0), range());

        let result = LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Inner,
            false,
            |_| None,
            |_| true,
        );

        assert_eq!(
            result.err(),
            Some(LightWorksetSetupError::MissingRequiredChunk {
                chunk_pos: ChunkPos::new(-1, -1),
            })
        );
    }

    #[test]
    fn chunk_read_cache_exposes_admitted_chunks() {
        init_tests();
        let center = ChunkPos::new(0, 0);
        let holder = holder_with_section(center, ChunkSection::new_empty());
        let layout = LightCacheLayout::new(center, range());
        let Ok(workset) = LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Inner,
            true,
            |pos| (pos == center).then(|| Arc::clone(&holder)),
            |_| true,
        ) else {
            panic!("relaxed setup should accept missing neighbors");
        };
        let Some(cached_center) = layout.cached_chunk(center) else {
            panic!("center chunk should be inside the cache");
        };

        workset.with_chunk_read_cache(|chunk_cache| {
            assert_eq!(chunk_cache.layout(), layout);
            assert!(chunk_cache.chunk(cached_center).is_some());
        });
    }

    #[test]
    fn section_read_cache_uses_scalable_lux_local_indices() {
        init_tests();
        let center = ChunkPos::new(0, 0);
        let mut section = ChunkSection::new_empty();
        let stone = vanilla_blocks::STONE.default_state();
        section.set_block_state(1, 2, 3, stone);
        let holder = holder_with_section(center, section);
        let layout = LightCacheLayout::new(center, range());
        let Ok(workset) = LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Inner,
            true,
            |pos| (pos == center).then(|| Arc::clone(&holder)),
            |_| true,
        ) else {
            panic!("relaxed setup should accept missing neighbors");
        };

        let Some(cached_block) = layout.cached_block(BlockPos::new(1, 2, 3)) else {
            panic!("test block should be inside light cache");
        };
        let read_state = workset.with_chunk_read_cache(|chunk_cache| {
            chunk_cache.with_section_read_cache(|section_cache| {
                assert_eq!(section_cache.layout(), layout);
                section_cache.get_block_state(cached_block)
            })
        });

        assert_eq!(read_state, stone);
    }

    #[test]
    fn section_read_cache_reports_non_empty_sections() {
        init_tests();
        let center = ChunkPos::new(0, 0);
        let mut section = ChunkSection::new_empty();
        section.set_block_state(1, 2, 3, vanilla_blocks::STONE.default_state());
        let holder = holder_with_section(center, section);
        let layout = LightCacheLayout::new(center, range());
        let Ok(workset) = LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Inner,
            true,
            |pos| (pos == center).then(|| Arc::clone(&holder)),
            |_| true,
        ) else {
            panic!("relaxed setup should accept missing neighbors");
        };

        workset.with_chunk_read_cache(|chunk_cache| {
            chunk_cache.with_section_read_cache(|section_cache| {
                assert!(section_cache.has_cached_section(SectionPos::new(0, 0, 0)));
                assert!(section_cache.has_non_empty_section(SectionPos::new(0, 0, 0)));
                assert!(!section_cache.has_non_empty_section(SectionPos::new(0, 1, 0)));
                assert!(!section_cache.has_non_empty_section(SectionPos::new(1, 0, 0)));
            });
        });
    }

    #[test]
    fn section_read_cache_reports_outer_chunk_emptiness_maps() {
        init_tests();
        let center = ChunkPos::new(0, 0);
        let outer = ChunkPos::new(2, 0);
        let center_holder = holder_with_section(center, ChunkSection::new_empty());
        let mut outer_section = ChunkSection::new_empty();
        outer_section.set_block_state(1, 2, 3, vanilla_blocks::STONE.default_state());
        let outer_holder = holder_with_section(outer, outer_section);
        let layout = LightCacheLayout::new(center, range());
        let Ok(workset) = LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Full,
            true,
            |pos| {
                if pos == center {
                    Some(Arc::clone(&center_holder))
                } else if pos == outer {
                    Some(Arc::clone(&outer_holder))
                } else {
                    None
                }
            },
            |_| true,
        ) else {
            panic!("relaxed setup should accept cached test chunks");
        };

        workset.with_chunk_read_cache(|chunk_cache| {
            chunk_cache.with_section_read_cache(|section_cache| {
                assert_eq!(
                    section_cache.section_empty(SectionPos::new(outer.0.x, 0, outer.0.y)),
                    Some(false)
                );
                assert!(
                    !section_cache.has_non_empty_section(SectionPos::new(outer.0.x, 0, outer.0.y))
                );
            });
        });
    }

    #[test]
    fn workset_can_read_sections_without_writable_light_scope() {
        init_tests();
        let center = ChunkPos::new(0, 0);
        let east = ChunkPos::new(1, 0);
        let center_holder = holder_with_section(center, ChunkSection::new_empty());
        let mut east_section = ChunkSection::new_empty();
        east_section.set_block_state(0, 0, 0, vanilla_blocks::STONE.default_state());
        let east_holder = holder_with_section(east, east_section);
        let layout = LightCacheLayout::new(center, range());

        let Ok(workset) = LightWorkset::setup_with_scopes(
            layout,
            LightCacheSetupRadius::Inner,
            true,
            |pos| {
                if pos == center {
                    Some(Arc::clone(&center_holder))
                } else if pos == east {
                    Some(Arc::clone(&east_holder))
                } else {
                    None
                }
            },
            |cached_chunk, _, _| (true, cached_chunk.chunk_pos == center),
        ) else {
            panic!("relaxed setup should accept missing neighbors");
        };

        let Some(cached_center) = layout.cached_chunk(center) else {
            panic!("center chunk should be cached");
        };
        let Some(cached_east) = layout.cached_chunk(east) else {
            panic!("east chunk should be cached");
        };
        assert!(workset.can_read_sections(cached_center));
        assert!(workset.can_write_light(cached_center));
        assert!(workset.can_read_sections(cached_east));
        assert!(!workset.can_write_light(cached_east));

        workset.with_chunk_read_cache(|chunk_cache| {
            chunk_cache.with_section_read_cache(|section_cache| {
                assert!(section_cache.has_non_empty_section(SectionPos::new(1, 0, 0)));
            });
        });
    }

    #[test]
    fn light_edit_reads_writes_and_commits_sections() {
        init_tests();
        let center = ChunkPos::new(0, 0);
        let holder = holder_with_section(center, ChunkSection::new_empty());
        let layout = LightCacheLayout::new(center, range());
        let Ok(workset) = LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Inner,
            true,
            |pos| (pos == center).then(|| Arc::clone(&holder)),
            |_| true,
        ) else {
            panic!("relaxed setup should accept missing neighbors");
        };
        let Some(cached_block) = layout.cached_block(BlockPos::new(1, 2, 3)) else {
            panic!("test block should be inside light cache");
        };

        let updated = workset.with_chunk_read_cache(|chunk_cache| {
            chunk_cache.with_light_edit(LightLayer::Block, |mut light_edit| {
                assert_eq!(light_edit.layout(), layout);
                assert_eq!(light_edit.layer(), LightLayer::Block);
                assert_eq!(light_edit.get(cached_block), 0);
                assert!(!light_edit.set(cached_block, 12));
                assert!(light_edit.is_section_missing(SectionPos::new(0, 0, 0)));

                assert!(light_edit.set_section_non_missing(SectionPos::new(0, 0, 0)));
                assert!(light_edit.has_non_missing_section(SectionPos::new(0, 0, 0)));
                assert!(!light_edit.is_section_missing(SectionPos::new(0, 0, 0)));
                assert!(light_edit.has_non_missing(cached_block));
                assert!(!light_edit.has_light_data_section(SectionPos::new(0, 0, 0)));
                assert!(light_edit.set(cached_block, 12));
                assert_eq!(light_edit.get(cached_block), 12);
                assert!(light_edit.has_light_data_section(SectionPos::new(0, 0, 0)));

                let mut updated = Vec::new();
                assert_eq!(
                    light_edit.commit(None, |section_pos| updated.push(section_pos)),
                    1
                );
                updated
            })
        });

        assert_eq!(updated, vec![SectionPos::new(0, 0, 0)]);
        let Some(chunk) = holder.try_chunk(ChunkStatus::Empty) else {
            panic!("test chunk should still be available");
        };
        let light = chunk.light();
        assert_eq!(
            light.get_light_value(LightLayer::Block, BlockPos::new(1, 2, 3)),
            12
        );
    }

    #[test]
    fn light_edit_drops_without_commit() {
        init_tests();
        let center = ChunkPos::new(0, 0);
        let holder = holder_with_section(center, ChunkSection::new_empty());
        let layout = LightCacheLayout::new(center, range());
        let Ok(workset) = LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Inner,
            true,
            |pos| (pos == center).then(|| Arc::clone(&holder)),
            |_| true,
        ) else {
            panic!("relaxed setup should accept missing neighbors");
        };
        let Some(cached_block) = layout.cached_block(BlockPos::new(1, 2, 3)) else {
            panic!("test block should be inside light cache");
        };

        workset.with_chunk_read_cache(|chunk_cache| {
            chunk_cache.with_light_edit(LightLayer::Block, |mut light_edit| {
                assert!(light_edit.set_section_non_missing(SectionPos::new(0, 0, 0)));
                assert!(light_edit.set(cached_block, 12));
            });
        });

        let Some(chunk) = holder.try_chunk(ChunkStatus::Empty) else {
            panic!("test chunk should still be available");
        };
        let light = chunk.light();
        assert_eq!(
            light.get_light_value(LightLayer::Block, BlockPos::new(1, 2, 3)),
            0
        );
    }

    #[test]
    fn light_edit_publishes_explicit_notifications() {
        init_tests();
        let center = ChunkPos::new(0, 0);
        let holder = holder_with_section(center, ChunkSection::new_empty());
        set_light_section(
            &holder,
            LightLayer::Block,
            0,
            LightSection::visible(LightSectionData::homogeneous(0)),
        );
        let layout = LightCacheLayout::new(center, range());
        let Ok(workset) = LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Inner,
            true,
            |pos| (pos == center).then(|| Arc::clone(&holder)),
            |_| true,
        ) else {
            panic!("relaxed setup should accept missing neighbors");
        };
        let mut notifications = LightUpdateNotificationCache::new(layout);
        assert!(notifications.mark_section(SectionPos::new(0, 0, 0)));

        let updated = workset.with_chunk_read_cache(|chunk_cache| {
            chunk_cache.with_light_edit(LightLayer::Block, |light_edit| {
                let mut updated = Vec::new();
                assert_eq!(
                    light_edit.commit(Some(&notifications), |section_pos| {
                        updated.push(section_pos);
                    }),
                    1
                );
                updated
            })
        });

        assert_eq!(updated, vec![SectionPos::new(0, 0, 0)]);
    }

    #[test]
    fn sky_edit_materializes_removed_missing_sections_transiently() {
        init_tests();
        let center = ChunkPos::new(0, 0);
        let holder = holder_with_section(center, ChunkSection::new_empty());
        let layout = LightCacheLayout::new(center, range());
        let Ok(workset) = LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Inner,
            true,
            |pos| (pos == center).then(|| Arc::clone(&holder)),
            |_| true,
        ) else {
            panic!("relaxed setup should accept missing neighbors");
        };
        let section_pos = SectionPos::new(0, 0, 0);
        let Some(cached_block) = layout.cached_block(BlockPos::new(1, 2, 3)) else {
            panic!("test block should be inside light cache");
        };

        let updated = workset.with_chunk_read_cache(|chunk_cache| {
            chunk_cache.with_light_edit(LightLayer::Sky, |mut light_edit| {
                light_edit.rewrite_missing_sections_for_skylight();
                assert!(!light_edit.has_cached_section(section_pos));
                assert!(!light_edit.set(cached_block, 12));

                assert!(light_edit.materialize_removed_missing_section(section_pos));
                assert!(light_edit.has_cached_section(section_pos));
                assert!(light_edit.set_section_non_missing(section_pos));
                assert!(light_edit.set(cached_block, 12));

                let mut updated = Vec::new();
                assert_eq!(
                    light_edit.commit(None, |section_pos| updated.push(section_pos)),
                    1
                );
                updated
            })
        });

        assert_eq!(updated, vec![section_pos]);
        let Some(chunk) = holder.try_chunk(ChunkStatus::Empty) else {
            panic!("test chunk should still be available");
        };
        let light = chunk.light();
        assert_eq!(light.sky.section(0), Some(&LightSection::missing()));
    }

    #[test]
    fn light_edit_extrudes_lower_row_from_source_above() {
        init_tests();
        let center = ChunkPos::new(0, 0);
        let holder = holder_with_section(center, ChunkSection::new_empty());
        set_light_section(
            &holder,
            LightLayer::Sky,
            1,
            LightSection::visible(LightSectionData::homogeneous(9)),
        );
        let layout = LightCacheLayout::new(center, range());
        let Ok(workset) = LightWorkset::setup(
            layout,
            LightCacheSetupRadius::Inner,
            true,
            |pos| (pos == center).then(|| Arc::clone(&holder)),
            |_| true,
        ) else {
            panic!("relaxed setup should accept missing neighbors");
        };
        let Some(cached_block) = layout.cached_block(BlockPos::new(1, 15, 3)) else {
            panic!("test block should be inside light cache");
        };

        workset.with_chunk_read_cache(|chunk_cache| {
            chunk_cache.with_light_edit(LightLayer::Sky, |mut light_edit| {
                assert!(
                    light_edit.extrude_lower_from_first_section_above(SectionPos::new(0, 0, 0))
                );
                assert_eq!(light_edit.get(cached_block), 9);
                assert_eq!(light_edit.commit(None, |_| {}), 1);
            });
        });

        let Some(chunk) = holder.try_chunk(ChunkStatus::Empty) else {
            panic!("test chunk should still be available");
        };
        let light = chunk.light();
        assert_eq!(
            light.get_light_value(LightLayer::Sky, BlockPos::new(1, 15, 3)),
            9
        );
    }
}
