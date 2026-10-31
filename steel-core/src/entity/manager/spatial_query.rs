//! ordered hits from already sorted cells

use std::{slice, vec::IntoIter};

use rustc_hash::FxHashSet;
use smallvec::SmallVec;
use steel_utils::WorldAabb;

use super::{EntityEntry, EntityQueryOrder, EntitySlot, ManagerState};

const MAX_MERGED_QUERY_CELLS: usize = 8;

#[expect(
    clippy::large_enum_variant,
    reason = "Common query cursors stay inline to avoid an allocation per spatial lookup"
)]
pub(super) enum EntityQueryEntries<'a> {
    Single {
        state: &'a ManagerState,
        aabb: Option<WorldAabb>,
        remaining: slice::Iter<'a, EntitySlot>,
    },
    Merged {
        state: &'a ManagerState,
        aabb: Option<WorldAabb>,
        cursors: SmallVec<[EntityQueryCursor<'a>; MAX_MERGED_QUERY_CELLS]>,
    },
    Sorted(IntoIter<&'a EntityEntry>),
}

pub(super) struct EntityQueryCursor<'a> {
    entry: &'a EntityEntry,
    order: EntityQueryOrder,
    remaining: slice::Iter<'a, EntitySlot>,
}

impl<'a> EntityQueryCursor<'a> {
    fn new(
        state: &'a ManagerState,
        aabb: Option<&WorldAabb>,
        cell: &'a [EntitySlot],
    ) -> Option<Self> {
        let mut remaining = cell.iter();
        let entry = next_candidate(state, aabb, &mut remaining)?;

        Some(Self {
            entry,
            order: entry.query_order,
            remaining,
        })
    }

    fn advance(&mut self, state: &'a ManagerState, aabb: Option<&WorldAabb>) -> bool {
        let Some(entry) = next_candidate(state, aabb, &mut self.remaining) else {
            return false;
        };

        self.entry = entry;
        self.order = entry.query_order;

        true
    }
}

fn next_candidate<'a>(
    state: &'a ManagerState,
    aabb: Option<&WorldAabb>,
    remaining: &mut slice::Iter<'a, EntitySlot>,
) -> Option<&'a EntityEntry> {
    remaining.find_map(|slot| {
        let entry = state.live_by_id.entry(*slot);

        aabb.is_none_or(|aabb| entry.bounding_box.intersects(*aabb))
            .then_some(entry)
    })
}

impl<'a> EntityQueryEntries<'a> {
    pub(super) fn new(
        state: &'a ManagerState,
        aabb: Option<&WorldAabb>,
        cells: SmallVec<[&'a [EntitySlot]; MAX_MERGED_QUERY_CELLS]>,
    ) -> Self {
        if let [cell] = cells.as_slice() {
            return Self::Single {
                state,
                aabb: aabb.copied(),
                remaining: cell.iter(),
            };
        }

        if cells.len() <= MAX_MERGED_QUERY_CELLS {
            return Self::Merged {
                state,
                aabb: aabb.copied(),
                cursors: cells
                    .into_iter()
                    .filter_map(|cell| EntityQueryCursor::new(state, aabb, cell))
                    .collect(),
            };
        }

        let mut entity_ids = FxHashSet::default();

        for cell in cells {
            entity_ids.extend(cell.iter().copied());
        }

        let mut entries = entity_ids
            .into_iter()
            .map(|slot| state.live_by_id.entry(slot))
            .filter(|entry| aabb.is_none_or(|aabb| entry.bounding_box.intersects(*aabb)))
            .collect::<Vec<_>>();

        entries.sort_unstable_by_key(|entry| entry.query_order);
        Self::Sorted(entries.into_iter())
    }
}

impl<'a> Iterator for EntityQueryEntries<'a> {
    type Item = &'a EntityEntry;

    fn fold<B, F>(self, initial: B, mut visit: F) -> B
    where
        F: FnMut(B, Self::Item) -> B,
    {
        match self {
            Self::Single {
                state,
                aabb,
                remaining,
            } => remaining
                .map(|slot| state.live_by_id.entry(*slot))
                .filter(|entry| aabb.is_none_or(|aabb| entry.bounding_box.intersects(aabb)))
                .fold(initial, visit),
            Self::Sorted(entries) => entries.fold(initial, visit),
            entries @ Self::Merged { .. } => {
                let mut result = initial;

                for entry in entries {
                    result = visit(result, entry);
                }

                result
            }
        }
    }

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Single {
                state,
                aabb,
                remaining,
            } => next_candidate(state, aabb.as_ref(), remaining),
            Self::Merged {
                state,
                aabb,
                cursors,
            } => {
                let cursor = cursors.iter().min_by_key(|cursor| cursor.order)?;
                let entry = cursor.entry;
                let order = cursor.order;

                let mut index = 0;

                while index < cursors.len() {
                    if cursors[index].order != order || cursors[index].advance(state, aabb.as_ref())
                    {
                        index += 1;
                    } else {
                        cursors.swap_remove(index);
                    }
                }

                Some(entry)
            }
            Self::Sorted(entries) => entries.next(),
        }
    }
}
