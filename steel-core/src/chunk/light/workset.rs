use std::sync::Arc;

use parking_lot::RwLockReadGuard;
use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::{BlockStateId, ChunkPos, SectionPos};

use crate::chunk::{
    chunk_access::{ChunkAccess, ChunkStatus},
    chunk_holder::ChunkHolder,
    section::ChunkSection,
};

use super::{
    CachedLightBlock, CachedLightChunk, LightCacheChunkScope, LightCacheLayout,
    LightCacheSetupRadius, LightChunkSlotArray, LightSectionSlotArray,
};

/// Error returned when a scoped light workset cannot acquire required chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightWorksetSetupError {
    /// A chunk inside ScalableLux's required 1-radius cache was unavailable.
    MissingRequiredChunk {
        /// Missing chunk position.
        chunk_pos: ChunkPos,
    },
}

/// Scoped chunk admission for one light operation.
///
/// This keeps the ScalableLux cache-window admission rules without storing
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
    /// Creates a scoped cache window by scanning chunks in ScalableLux setup order.
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

        for chunk_slot in 0..self.chunks.slot_count() {
            let Some(workset_chunk) = self.chunks.get_slot(chunk_slot) else {
                continue;
            };
            if !workset_chunk.section_readable {
                continue;
            }
            let Some(chunk) = workset_chunk.holder.try_chunk(ChunkStatus::Empty) else {
                continue;
            };
            chunks.insert_slot(chunk_slot, chunk);
        }

        let cache = LightChunkReadCache {
            layout: self.layout,
            chunks,
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
}
