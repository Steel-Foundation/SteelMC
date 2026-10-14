//! The standing-spot fallback for a search in which every candidate failed.

use std::sync::Arc;

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::Direction;
use steel_registry::blocks::shapes::is_offset_face_full;
use steel_utils::BlockPos;

use crate::chunk::heightmap::HeightmapType;
use crate::world::World;

use super::chunk_load::{PollBudget, ScanEnd, Visit};
use super::{LoadPoll, PlayerSpawnSearch};

/// Looks for a standing spot in the candidate columns, one chunk at a time.
#[derive(Default)]
pub(super) struct FallbackPass {
    /// Candidate indices whose column was already scanned.
    scanned: Vec<bool>,
    /// First candidate index that may still be unscanned.
    anchor: i32,
    /// Set once every candidate column was scanned without finding a spot.
    pub(super) exhausted: bool,
}

pub(super) enum FallbackPoll {
    Found(BlockPos),
    Waiting,
    Cancelled,
    Exhausted,
}

impl PlayerSpawnSearch {
    /// Scans the candidate columns for a standing spot, loading one chunk at a time.
    pub(super) fn poll_fallback_pass(
        &mut self,
        world: &Arc<World>,
        budget: &mut PollBudget,
    ) -> FallbackPoll {
        let sequence = self.sequence;
        if self.fallback.scanned.is_empty() {
            self.fallback.scanned = vec![false; self.results.len()];
        }

        loop {
            let (chunk, next_scan) = if let Some(load) = &self.loading {
                (load.chunk, load.next_scan)
            } else {
                let Some(anchor) = (self.fallback.anchor..sequence.count()).find(|&index| {
                    self.fallback
                        .scanned
                        .get(index as usize)
                        .is_some_and(|scanned| !scanned)
                }) else {
                    return FallbackPoll::Exhausted;
                };
                self.fallback.anchor = anchor;
                (sequence.chunk(anchor), anchor)
            };
            let mut load = match self.poll_loading(world, chunk, next_scan) {
                LoadPoll::Ready(load) => load,
                LoadPoll::Waiting => return FallbackPoll::Waiting,
                LoadPoll::Cancelled => return FallbackPoll::Cancelled,
            };

            let scanned = &mut self.fallback.scanned;
            let end = load.scan(&sequence, budget, |index, x, z| {
                let Some(scanned) = scanned.get_mut(index as usize) else {
                    return Visit::Skip;
                };
                if *scanned {
                    return Visit::Skip;
                }
                *scanned = true;
                world
                    .safe_spawn_in_column(x, z)
                    .map_or(Visit::Evaluated, Visit::Stop)
            });
            match end {
                ScanEnd::Stopped(pos) => return FallbackPoll::Found(pos),
                ScanEnd::OutOfBudget => {
                    self.loading = Some(load);
                    return FallbackPoll::Waiting;
                }
                ScanEnd::Finished => {}
            }
        }
    }
}

impl World {
    /// Returns the highest position in the column where a player can stand: on a block
    /// with a full top face that holds no fluid, with a free player box above it. In a
    /// dimension with a ceiling the scan starts below the roof, so the roof is never chosen.
    pub(super) fn safe_spawn_in_column(self: &Arc<Self>, x: i32, z: i32) -> Option<BlockPos> {
        let min_y = self.get_min_y();
        let top_y = if self.dimension_type.has_ceiling {
            min_y + self.dimension_type.logical_height - 1
        } else {
            self.vanilla_chunk_height_at(HeightmapType::MotionBlocking, x, z)? + 1
        };

        for y in ((min_y + 1)..=top_y).rev() {
            let floor_pos = BlockPos::new(x, y - 1, z);
            let floor = self.get_block_state(floor_pos);
            if floor.has_fluid()
                || !is_offset_face_full(floor.get_collision_shape_at(floor_pos), Direction::Up)
            {
                continue;
            }
            let pos = BlockPos::new(x, y, z);
            // Prunes the solid blocks below the surface before the much costlier box test. It
            // also rejects a head block whose shape stays above the player's box, which the box
            // test would allow.
            let head_pos = pos.above();
            let feet = self.get_block_state(pos);
            let head = self.get_block_state(head_pos);
            if feet.has_fluid()
                || head.has_fluid()
                || !feet.get_collision_shape_at(pos).is_empty()
                || !head.get_collision_shape_at(head_pos).is_empty()
            {
                continue;
            }
            if self.no_collision_no_liquid(pos) {
                return Some(pos);
            }
        }

        None
    }
}
