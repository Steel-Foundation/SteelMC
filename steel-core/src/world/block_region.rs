use std::sync::Arc;

use parking_lot::RwLockReadGuard;
use smallvec::SmallVec;
use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::{BlockPos, BlockStateId, ChunkPos, SectionPos};

use crate::chunk::{chunk_holder::ChunkHolder, section::ChunkSection, status::ChunkStatus};

use super::World;

/// Maximum combined chunk-holder and section slots acquired by one bulk region read.
pub(crate) const MAX_BLOCK_REGION_WORKSET_SLOTS: usize = 64;

/// Inclusive block bounds for one scoped region read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BlockRegionBounds {
    min: BlockPos,
    max: BlockPos,
}

impl BlockRegionBounds {
    /// Creates bounds containing both corners regardless of their order.
    #[must_use]
    pub(crate) const fn from_corners(first: BlockPos, second: BlockPos) -> Self {
        Self {
            min: BlockPos::min(first, second),
            max: BlockPos::max(first, second),
        }
    }

    #[must_use]
    const fn contains(self, pos: BlockPos) -> bool {
        pos.x() >= self.min.x()
            && pos.y() >= self.min.y()
            && pos.z() >= self.min.z()
            && pos.x() <= self.max.x()
            && pos.y() <= self.max.y()
            && pos.z() <= self.max.z()
    }
}

struct BlockRegionWorkset {
    bounds: BlockRegionBounds,
    chunks: SmallVec<[Option<Arc<ChunkHolder>>; 4]>,
    min_chunk_x: i32,
    min_chunk_z: i32,
    chunk_z_count: usize,
    min_section_y: i32,
    section_y_count: usize,
    world_min_y: i32,
    world_max_y: i32,
}

impl BlockRegionWorkset {
    fn try_new(world: &World, bounds: BlockRegionBounds) -> Option<Self> {
        let min_chunk_x =
            SectionPos::block_to_section_coord(bounds.min.x()).max(-ChunkPos::MAX_COORDINATE_VALUE);
        let max_chunk_x =
            SectionPos::block_to_section_coord(bounds.max.x()).min(ChunkPos::MAX_COORDINATE_VALUE);
        let min_chunk_z =
            SectionPos::block_to_section_coord(bounds.min.z()).max(-ChunkPos::MAX_COORDINATE_VALUE);
        let max_chunk_z =
            SectionPos::block_to_section_coord(bounds.max.z()).min(ChunkPos::MAX_COORDINATE_VALUE);

        let chunk_width = inclusive_count(min_chunk_x, max_chunk_x);
        let chunk_depth = inclusive_count(min_chunk_z, max_chunk_z);
        let world_min_y = world.get_min_y();
        let world_max_y = world.get_max_y();
        let min_section_y = SectionPos::block_to_section_coord(bounds.min.y().max(world_min_y));
        let max_section_y = SectionPos::block_to_section_coord(bounds.max.y().min(world_max_y));
        let section_y_count = inclusive_count(min_section_y, max_section_y);

        let chunk_count = chunk_width.checked_mul(chunk_depth)?;
        let section_slot_count = chunk_count.checked_mul(section_y_count)?;
        let workset_slot_count = chunk_count.checked_add(section_slot_count)?;
        if workset_slot_count > MAX_BLOCK_REGION_WORKSET_SLOTS {
            return None;
        }

        let mut chunks = SmallVec::new();
        for chunk_x_offset in 0..chunk_width {
            let chunk_x = min_chunk_x + chunk_x_offset as i32;
            for chunk_z_offset in 0..chunk_depth {
                let chunk_z = min_chunk_z + chunk_z_offset as i32;
                chunks.push(
                    world
                        .chunk_map
                        .active_full_chunk_holder(ChunkPos::new(chunk_x, chunk_z)),
                );
            }
        }

        Some(Self {
            bounds,
            chunks,
            min_chunk_x,
            min_chunk_z,
            chunk_z_count: chunk_depth,
            min_section_y,
            section_y_count,
            world_min_y,
            world_max_y,
        })
    }

    fn with_read<R>(&self, f: impl FnOnce(&BlockRegionRead<'_>) -> R) -> R {
        let mut sections = SmallVec::new();

        // This is the lock order for all block-region operations: chunk X, chunk Z,
        // then section Y. A future write-region API must use the same order.
        for holder in &self.chunks {
            let chunk = holder
                .as_ref()
                .and_then(|holder| holder.try_chunk(ChunkStatus::Full));
            for section_y_offset in 0..self.section_y_count {
                let guard = chunk.and_then(|chunk| {
                    let section_y = self.min_section_y + section_y_offset as i32;
                    let section_index = usize::try_from(
                        section_y - SectionPos::block_to_section_coord(self.world_min_y),
                    )
                    .ok()?;
                    chunk
                        .sections()
                        .sections
                        .get(section_index)
                        .map(|section| section.read())
                });
                sections.push(guard);
            }
        }

        let read = BlockRegionRead {
            bounds: self.bounds,
            sections,
            min_chunk_x: self.min_chunk_x,
            min_chunk_z: self.min_chunk_z,
            chunk_z_count: self.chunk_z_count,
            min_section_y: self.min_section_y,
            section_y_count: self.section_y_count,
            world_min_y: self.world_min_y,
            world_max_y: self.world_max_y,
            air: REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR),
            void_air: REGISTRY.blocks.get_base_state_id(&vanilla_blocks::VOID_AIR),
        };
        f(&read)
    }
}

/// A read-only view of one section already locked by a [`BlockRegionRead`].
pub(crate) struct BlockSectionRead<'a> {
    section_pos: SectionPos,
    section: &'a ChunkSection,
}

impl BlockSectionRead<'_> {
    /// Returns a state when `pos` belongs to this section.
    #[must_use]
    pub(crate) fn get_block_state(&self, pos: BlockPos) -> Option<BlockStateId> {
        if SectionPos::from_block_pos(pos) != self.section_pos {
            return None;
        }
        Some(self.section.states.get(
            (pos.x() & 15) as usize,
            (pos.y() & 15) as usize,
            (pos.z() & 15) as usize,
        ))
    }
}

/// Scoped block-state reads over a rectangular set of prelocked sections.
pub(crate) struct BlockRegionRead<'a> {
    bounds: BlockRegionBounds,
    sections: SmallVec<[Option<RwLockReadGuard<'a, ChunkSection>>; 8]>,
    min_chunk_x: i32,
    min_chunk_z: i32,
    chunk_z_count: usize,
    min_section_y: i32,
    section_y_count: usize,
    world_min_y: i32,
    world_max_y: i32,
    air: BlockStateId,
    void_air: BlockStateId,
}

impl BlockRegionRead<'_> {
    /// Returns the block state at `pos`, or `None` when it is outside this region.
    ///
    /// Positions outside world bounds are void air. Missing Full chunks are air,
    /// matching [`World::get_block_state`].
    #[must_use]
    pub(crate) fn get_block_state(&self, pos: BlockPos) -> Option<BlockStateId> {
        if !self.bounds.contains(pos) {
            return None;
        }
        if pos.y() < self.world_min_y
            || pos.y() > self.world_max_y
            || !ChunkPos::is_valid(
                SectionPos::block_to_section_coord(pos.x()),
                SectionPos::block_to_section_coord(pos.z()),
            )
        {
            return Some(self.void_air);
        }

        let section_pos = SectionPos::from_block_pos(pos);
        let Some(section) = self.section(section_pos) else {
            return Some(self.air);
        };
        section.get_block_state(pos)
    }

    /// Returns a prelocked section view, or `None` for an unloaded or uncached section.
    #[must_use]
    pub(crate) fn section(&self, section_pos: SectionPos) -> Option<BlockSectionRead<'_>> {
        let chunk_x_offset =
            usize::try_from(section_pos.x().checked_sub(self.min_chunk_x)?).ok()?;
        let chunk_z_offset =
            usize::try_from(section_pos.z().checked_sub(self.min_chunk_z)?).ok()?;
        let section_y_offset =
            usize::try_from(section_pos.y().checked_sub(self.min_section_y)?).ok()?;
        if chunk_z_offset >= self.chunk_z_count || section_y_offset >= self.section_y_count {
            return None;
        }

        let chunk_slot = chunk_x_offset
            .checked_mul(self.chunk_z_count)?
            .checked_add(chunk_z_offset)?;
        let section_slot = chunk_slot
            .checked_mul(self.section_y_count)?
            .checked_add(section_y_offset)?;
        let section = self.sections.get(section_slot)?.as_ref()?;
        Some(BlockSectionRead {
            section_pos,
            section,
        })
    }
}

impl World {
    /// Acquires every loaded section intersecting a small `bounds` once and exposes a scoped read
    /// view.
    ///
    /// Setup and lock count scale with every intersecting chunk and section, so this is an internal
    /// primitive for bounded synchronous gameplay queries rather than arbitrary or
    /// player-controlled regions.
    ///
    /// The callback must use [`BlockRegionRead`] for reads covered by `bounds` and must not perform
    /// world writes. Re-entering a world read for a covered section can deadlock if a writer is
    /// already waiting because the requested section read guards remain held until the callback
    /// returns.
    pub(crate) fn try_with_block_region<R>(
        &self,
        bounds: BlockRegionBounds,
        f: impl FnOnce(&BlockRegionRead<'_>) -> R,
    ) -> Option<R> {
        Some(BlockRegionWorkset::try_new(self, bounds)?.with_read(f))
    }
}

fn inclusive_count(min: i32, max: i32) -> usize {
    if min > max {
        return 0;
    }
    usize::try_from(i64::from(max) - i64::from(min) + 1).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use steel_registry::vanilla_blocks;
    use steel_utils::{BlockPos, ChunkPos, SectionPos, WorldAabb, types::UpdateFlags};

    use crate::{
        behavior::init_behaviors,
        test_support::{fresh_test_world, insert_ready_full_chunk},
    };

    use super::{BlockRegionBounds, BlockRegionWorkset};

    #[test]
    fn region_reuses_section_reads_across_chunk_and_section_boundaries() {
        let world = fresh_test_world("block_region_reads");
        init_behaviors();
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        insert_ready_full_chunk(&world, ChunkPos::new(1, 0));

        let first = BlockPos::new(15, 64, 0);
        let second = BlockPos::new(16, 80, 0);
        assert!(world.set_block(
            first,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        assert!(world.set_block(
            second,
            vanilla_blocks::DIRT.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        let below_world = BlockPos::new(15, world.get_min_y() - 1, 0);
        let missing_chunk = BlockPos::new(32, 64, 0);
        let bounds = BlockRegionBounds::from_corners(below_world, BlockPos::new(32, 80, 0));
        world
            .try_with_block_region(bounds, |region| {
                assert_eq!(
                    region.get_block_state(first),
                    Some(vanilla_blocks::STONE.default_state())
                );
                assert_eq!(
                    region.get_block_state(second),
                    Some(vanilla_blocks::DIRT.default_state())
                );
                assert_eq!(
                    region.get_block_state(missing_chunk),
                    Some(vanilla_blocks::AIR.default_state())
                );
                assert_eq!(
                    region.get_block_state(below_world),
                    Some(vanilla_blocks::VOID_AIR.default_state())
                );
                assert_eq!(region.get_block_state(first.west()), None);

                let Some(section) = region.section(SectionPos::from_block_pos(first)) else {
                    panic!("the first block's loaded section should be cached");
                };
                assert_eq!(
                    section.get_block_state(first),
                    Some(vanilla_blocks::STONE.default_state())
                );
                assert_eq!(section.get_block_state(second), None);
                assert!(
                    region
                        .section(SectionPos::new(i32::MAX, i32::MAX, i32::MAX))
                        .is_none()
                );
            })
            .expect("focused region should fit the bounded workset");

        assert!(
            !world.block_states_in_aabb_are_air(WorldAabb::new(15.0, 64.0, 0.0, 15.9, 64.9, 0.9,))
        );
        assert!(
            world.block_states_in_aabb_are_air(WorldAabb::new(32.0, 64.0, 0.0, 32.9, 64.9, 0.9,))
        );
    }

    #[test]
    fn oversized_region_uses_streaming_reads() {
        let world = fresh_test_world("oversized_block_region_reads");
        init_behaviors();
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

        let first = BlockPos::new(0, 64, 0);
        assert!(world.set_block(
            first,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        let oversized_bounds =
            BlockRegionBounds::from_corners(first, BlockPos::new(16 * 64, first.y(), first.z()));
        assert!(BlockRegionWorkset::try_new(&world, oversized_bounds).is_none());
        assert!(!world.block_states_in_aabb_are_air(WorldAabb::new(
            f64::from(first.x()),
            f64::from(first.y()),
            f64::from(first.z()),
            f64::from(16 * 64),
            f64::from(first.y()),
            f64::from(first.z()),
        )));
    }
}
