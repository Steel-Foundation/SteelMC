//! Incremental Full-neighborhood tracking for chunk ticking readiness.

use std::{
    mem,
    sync::{Arc, Weak},
};

use rustc_hash::{FxHashMap, FxHashSet};
use steel_utils::{ChunkPos, locks::SyncMutex};

use super::chunk_holder::{ChunkHolder, ChunkSaveDependency};

const BLOCK_TICKING_FULL_COUNT: u8 = 9;
const ENTITY_TICKING_FULL_COUNT: u8 = 25;

/// Queue receiving notifications after a holder publishes `ChunkStatus::Full`.
///
/// `ChunkMap` owns the queue strongly. Holders keep only a weak sink so an
/// unloading holder cannot retain its map.
#[derive(Default)]
pub(crate) struct FullPublicationQueue {
    pending: SyncMutex<Vec<FullPublication>>,
}

impl FullPublicationQueue {
    pub(crate) fn publish(&self, holder: &Arc<ChunkHolder>) {
        self.pending.lock().push(FullPublication {
            pos: holder.get_pos(),
            holder: Arc::downgrade(holder),
        });
    }

    pub(crate) fn drain(&self) -> Vec<FullPublication> {
        mem::take(&mut *self.pending.lock())
    }
}

pub(crate) struct FullPublication {
    pub(crate) pos: ChunkPos,
    pub(crate) holder: Weak<ChunkHolder>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FullNeighborhoodCounts {
    pub(crate) block_ticking: u8,
    pub(crate) entity_ticking: u8,
}

impl FullNeighborhoodCounts {
    #[must_use]
    pub(crate) const fn block_ticking_ready(self) -> bool {
        self.block_ticking == BLOCK_TICKING_FULL_COUNT
    }

    #[must_use]
    pub(crate) const fn entity_ticking_ready(self) -> bool {
        self.entity_ticking == ENTITY_TICKING_FULL_COUNT
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FullNeighborhoodError {
    CounterOverflow {
        center: ChunkPos,
        radius: u8,
        count: u8,
    },
    CounterUnderflow {
        center: ChunkPos,
        radius: u8,
    },
    ContributorIdentityMismatch {
        pos: ChunkPos,
    },
}

struct PendingReadiness {
    holder: Weak<ChunkHolder>,
    _save_dependency: ChunkSaveDependency,
}

#[derive(Default)]
pub(crate) struct FullNeighborhoodIndex {
    contributors: FxHashMap<ChunkPos, Weak<ChunkHolder>>,
    counts: FxHashMap<ChunkPos, FullNeighborhoodCounts>,
    dirty_centers: FxHashSet<ChunkPos>,
    pending_readiness: FxHashMap<ChunkPos, PendingReadiness>,
}

impl FullNeighborhoodIndex {
    pub(crate) fn reconcile_contributor(
        &mut self,
        pos: ChunkPos,
        holder: Option<&Arc<ChunkHolder>>,
    ) -> Result<(), FullNeighborhoodError> {
        let unchanged = match (self.contributors.get(&pos), holder) {
            (Some(current), Some(holder)) => current.ptr_eq(&Arc::downgrade(holder)),
            (None, None) => true,
            (Some(_), None) | (None, Some(_)) => false,
        };
        if unchanged {
            return Ok(());
        }

        if self.contributors.contains_key(&pos) {
            self.remove_contributor(pos)?;
        }
        if let Some(holder) = holder {
            self.add_contributor(pos, Arc::downgrade(holder))?;
        }
        Ok(())
    }

    pub(crate) fn remove_contributor_if_matches(
        &mut self,
        pos: ChunkPos,
        holder: &Arc<ChunkHolder>,
    ) -> Result<(), FullNeighborhoodError> {
        let expected = Arc::downgrade(holder);
        match self.contributors.get(&pos) {
            None => Ok(()),
            Some(current) if current.ptr_eq(&expected) => self.remove_contributor(pos),
            Some(_) => Err(FullNeighborhoodError::ContributorIdentityMismatch { pos }),
        }
    }

    fn add_contributor(
        &mut self,
        pos: ChunkPos,
        holder: Weak<ChunkHolder>,
    ) -> Result<(), FullNeighborhoodError> {
        debug_assert!(!self.contributors.contains_key(&pos));
        self.validate_increment(pos, 1, BLOCK_TICKING_FULL_COUNT)?;
        self.validate_increment(pos, 2, ENTITY_TICKING_FULL_COUNT)?;

        self.contributors.insert(pos, holder);
        self.adjust_counts(pos, 1, true);
        self.adjust_counts(pos, 2, true);
        Ok(())
    }

    fn remove_contributor(&mut self, pos: ChunkPos) -> Result<(), FullNeighborhoodError> {
        debug_assert!(self.contributors.contains_key(&pos));
        self.validate_decrement(pos, 1)?;
        self.validate_decrement(pos, 2)?;

        self.contributors.remove(&pos);
        self.adjust_counts(pos, 1, false);
        self.adjust_counts(pos, 2, false);
        Self::for_each_center(pos, 2, |center| {
            if self
                .counts
                .get(&center)
                .is_some_and(|counts| counts.block_ticking == 0 && counts.entity_ticking == 0)
            {
                self.counts.remove(&center);
            }
        });
        Ok(())
    }

    fn validate_increment(
        &self,
        pos: ChunkPos,
        radius: u8,
        maximum: u8,
    ) -> Result<(), FullNeighborhoodError> {
        let mut result = Ok(());
        Self::for_each_center(pos, radius, |center| {
            if result.is_err() {
                return;
            }
            let counts = self.counts.get(&center).copied().unwrap_or_default();
            let count = if radius == 1 {
                counts.block_ticking
            } else {
                counts.entity_ticking
            };
            if count >= maximum {
                result = Err(FullNeighborhoodError::CounterOverflow {
                    center,
                    radius,
                    count,
                });
            }
        });
        result
    }

    fn validate_decrement(&self, pos: ChunkPos, radius: u8) -> Result<(), FullNeighborhoodError> {
        let mut result = Ok(());
        Self::for_each_center(pos, radius, |center| {
            if result.is_err() {
                return;
            }
            let counts = self.counts.get(&center).copied().unwrap_or_default();
            let count = if radius == 1 {
                counts.block_ticking
            } else {
                counts.entity_ticking
            };
            if count == 0 {
                result = Err(FullNeighborhoodError::CounterUnderflow { center, radius });
            }
        });
        result
    }

    fn adjust_counts(&mut self, pos: ChunkPos, radius: u8, increment: bool) {
        Self::for_each_center(pos, radius, |center| {
            let counts = self.counts.entry(center).or_default();
            let count = if radius == 1 {
                &mut counts.block_ticking
            } else {
                &mut counts.entity_ticking
            };
            if increment {
                *count += 1;
            } else {
                *count -= 1;
            }
            self.dirty_centers.insert(center);
        });
    }

    fn for_each_center(pos: ChunkPos, radius: u8, mut f: impl FnMut(ChunkPos)) {
        let radius = i32::from(radius);
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let Some(x) = pos.0.x.checked_add(dx) else {
                    continue;
                };
                let Some(z) = pos.0.y.checked_add(dz) else {
                    continue;
                };
                f(ChunkPos::new(x, z));
            }
        }
    }

    pub(crate) fn mark_dirty(&mut self, pos: ChunkPos) {
        self.dirty_centers.insert(pos);
    }

    pub(crate) fn dirty_counts_snapshot(&self) -> Vec<(ChunkPos, FullNeighborhoodCounts)> {
        self.dirty_centers
            .iter()
            .copied()
            .map(|pos| (pos, self.counts.get(&pos).copied().unwrap_or_default()))
            .collect()
    }

    pub(crate) fn take_dirty_counts(&mut self) -> Vec<(ChunkPos, FullNeighborhoodCounts)> {
        self.dirty_centers
            .drain()
            .map(|pos| (pos, self.counts.get(&pos).copied().unwrap_or_default()))
            .collect()
    }

    pub(crate) fn ensure_pending_readiness(&mut self, pos: ChunkPos, holder: &Arc<ChunkHolder>) {
        let holder_weak = Arc::downgrade(holder);
        if self
            .pending_readiness
            .get(&pos)
            .is_some_and(|pending| pending.holder.ptr_eq(&holder_weak))
        {
            return;
        }

        self.pending_readiness.insert(
            pos,
            PendingReadiness {
                holder: holder_weak,
                _save_dependency: holder.add_save_dependency(),
            },
        );
    }

    pub(crate) fn clear_pending_readiness(&mut self, pos: ChunkPos) {
        self.pending_readiness.remove(&pos);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_square_counts_track_block_and_entity_neighborhoods() {
        let mut index = FullNeighborhoodIndex::default();
        for z in -2..=2 {
            for x in -2..=2 {
                index
                    .add_contributor(ChunkPos::new(x, z), Weak::new())
                    .expect("unique contributors stay within neighborhood bounds");
            }
        }

        assert_eq!(
            index.counts.get(&ChunkPos::new(0, 0)),
            Some(&FullNeighborhoodCounts {
                block_ticking: BLOCK_TICKING_FULL_COUNT,
                entity_ticking: ENTITY_TICKING_FULL_COUNT,
            })
        );

        index
            .remove_contributor(ChunkPos::new(-2, -2))
            .expect("existing contributor has matching counters");
        assert_eq!(
            index.counts.get(&ChunkPos::new(0, 0)),
            Some(&FullNeighborhoodCounts {
                block_ticking: BLOCK_TICKING_FULL_COUNT,
                entity_ticking: ENTITY_TICKING_FULL_COUNT - 1,
            })
        );
    }

    #[test]
    fn invalid_extreme_coordinates_do_not_overflow() {
        let mut index = FullNeighborhoodIndex::default();
        let pos = ChunkPos::new(i32::MAX, i32::MIN);

        index
            .add_contributor(pos, Weak::new())
            .expect("representable neighboring centers should be counted");
        index
            .remove_contributor(pos)
            .expect("the same representable centers should be decremented");

        assert!(index.counts.is_empty());
    }
}
