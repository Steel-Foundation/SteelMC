use std::cmp::{max, min};

use steel_utils::ChunkPos;

/// A view of chunks around a center chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerChunkView {
    /// The center of the view.
    pub center: ChunkPos,
    /// The view distance in chunks.
    pub view_distance: u8,
}

impl PlayerChunkView {
    /// Creates a new empty `ChunkTrackingView`.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            center: ChunkPos::new(0, 0),
            view_distance: 0,
        }
    }

    /// Creates a new `ChunkTrackingView` with the given center and view distance.
    #[must_use]
    pub const fn new(center: ChunkPos, view_distance: u8) -> Self {
        Self {
            center,
            view_distance,
        }
    }

    fn min_x(&self) -> i32 {
        self.center.0.x - i32::from(self.view_distance) - 1
    }

    fn max_x(&self) -> i32 {
        self.center.0.x + i32::from(self.view_distance) + 1
    }

    fn min_z(&self) -> i32 {
        self.center.0.y - i32::from(self.view_distance) - 1
    }

    fn max_z(&self) -> i32 {
        self.center.0.y + i32::from(self.view_distance) + 1
    }

    /// Checks if the given chunk position is within the view.
    #[must_use]
    pub fn contains(&self, pos: ChunkPos) -> bool {
        Self::is_within_distance(
            self.center.0.x,
            self.center.0.y,
            i32::from(self.view_distance),
            pos.0.x,
            pos.0.y,
            true,
        )
    }

    /// Checks if a chunk at `(chunk_x, chunk_z)` is within the view distance of `(center_x, center_z)`.
    #[must_use]
    pub fn is_within_distance(
        center_x: i32,
        center_z: i32,
        view_distance: i32,
        chunk_x: i32,
        chunk_z: i32,
        include_neighbors: bool,
    ) -> bool {
        let buffer_range = if include_neighbors { 2 } else { 1 };
        let delta_x = i64::from(max(0, (chunk_x - center_x).abs() - buffer_range));
        let delta_z = i64::from(max(0, (chunk_z - center_z).abs() - buffer_range));
        let distance_squared = delta_x * delta_x + delta_z * delta_z;
        let radius_squared = i64::from(view_distance) * i64::from(view_distance);
        distance_squared < radius_squared
    }

    /// Iterates over all chunks in the view.
    pub fn for_each<F>(&self, mut f: F)
    where
        F: FnMut(ChunkPos),
    {
        for x in self.min_x()..=self.max_x() {
            for z in self.min_z()..=self.max_z() {
                let pos = ChunkPos::new(x, z);
                if self.contains(pos) {
                    f(pos);
                }
            }
        }
    }

    fn square_intersects(&self, other: &Self) -> bool {
        self.min_x() <= other.max_x()
            && self.max_x() >= other.min_x()
            && self.min_z() <= other.max_z()
            && self.max_z() >= other.min_z()
    }

    /// Calculates the difference between two views, calling `on_added` for chunks in the new view but not the old,
    /// and `on_removed` for chunks in the old view but not the new.
    pub fn difference<T>(
        old: &Self,
        new: &Self,
        mut on_added: impl FnMut(ChunkPos, &mut T),
        mut on_removed: impl FnMut(ChunkPos, &mut T),
        data: &mut T,
    ) {
        if old == new {
            return;
        }

        if old.square_intersects(new) {
            let min_x = min(old.min_x(), new.min_x());
            let min_z = min(old.min_z(), new.min_z());
            let max_x = max(old.max_x(), new.max_x());
            let max_z = max(old.max_z(), new.max_z());

            for x in min_x..=max_x {
                for z in min_z..=max_z {
                    let pos = ChunkPos::new(x, z);
                    let saw = old.contains(pos);
                    let sees = new.contains(pos);
                    if saw != sees {
                        if sees {
                            on_added(pos, data);
                        } else {
                            on_removed(pos, data);
                        }
                    }
                }
            }
        } else {
            old.for_each(|pos| on_removed(pos, data));
            new.for_each(|pos| on_added(pos, data));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contains() {
        let view = PlayerChunkView::new(ChunkPos::new(0, 0), 2);

        // (0,0) is center, should be contained
        assert!(view.contains(ChunkPos::new(0, 0)));

        // view distance 2 + buffer 2 = 4 (actually logic is slightly different)
        // radius_squared = 2*2 = 4
        // delta = max(0, dist - 2)
        // if dist = 3, delta = 1. 1*1 = 1 < 4. True.
        // if dist = 4, delta = 2. 2*2 = 4 == 4. False.

        // Check neighbors logic
        assert!(view.contains(ChunkPos::new(3, 0))); // dist 3 -> delta 1 -> 1 < 4 -> ok
        assert!(!view.contains(ChunkPos::new(4, 0))); // dist 4 -> delta 2 -> 4 < 4 -> false
    }

    #[test]
    fn test_difference() {
        let mut added = Vec::new();
        let mut removed = Vec::new();

        let old_view = PlayerChunkView::new(ChunkPos::new(0, 0), 2);
        let new_view = PlayerChunkView::new(ChunkPos::new(1, 0), 2);

        PlayerChunkView::difference(
            &old_view,
            &new_view,
            |p, ()| added.push(p),
            |p, ()| removed.push(p),
            &mut (),
        );

        // Just verify something happened
        assert!(!added.is_empty());
        assert!(!removed.is_empty());

        // Verify intersection logic worked by ensuring we have balanced adds/removes for shift
        // (Rough check)
    }

    #[test]
    fn test_disjoint_difference() {
        let mut added = Vec::new();
        let mut removed = Vec::new();

        let old_view = PlayerChunkView::new(ChunkPos::new(0, 0), 2);
        let new_view = PlayerChunkView::new(ChunkPos::new(100, 100), 2); // far away

        PlayerChunkView::difference(
            &old_view,
            &new_view,
            |p, ()| added.push(p),
            |p, ()| removed.push(p),
            &mut (),
        );

        // Should be full remove and full add
        // Count for view distance 2:
        // iterate -2-1..2+1 = -3..3 range (7x7 square roughly), filtered by circle
        let mut count = 0;
        old_view.for_each(|_| count += 1);

        assert_eq!(removed.len(), count);
        assert_eq!(added.len(), count);
    }
}
