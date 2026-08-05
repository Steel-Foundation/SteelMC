//! Vanilla `BlockUtil` helpers shared by gameplay systems.

use crate::{BlockPos, axis::Axis};

/// Rectangle found around a center block while scanning along two axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoundRectangle {
    /// Minimum corner of the rectangle.
    pub min_corner: BlockPos,
    /// Size along the first scan axis.
    pub axis1_size: i32,
    /// Size along the second scan axis.
    pub axis2_size: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IntBounds {
    min: i32,
    max: i32,
}

/// Returns vanilla `BlockUtil.getLargestRectangleAround`.
///
/// # Panics
///
/// Panics if either scan limit is negative.
#[must_use]
pub fn get_largest_rectangle_around(
    center: BlockPos,
    axis1: Axis,
    limit1: i32,
    axis2: Axis,
    limit2: i32,
    test: impl Fn(BlockPos) -> bool,
) -> FoundRectangle {
    assert!(
        limit1 >= 0 && limit2 >= 0,
        "rectangle scan limits must be non-negative"
    );

    let negative_delta1 = get_limit(&test, center, axis1, -1, limit1);
    let positive_delta1 = get_limit(&test, center, axis1, 1, limit1);
    let center_index1 = negative_delta1;
    let mut bounds_by_axis1 =
        vec![IntBounds { min: 0, max: 0 }; (center_index1 + 1 + positive_delta1) as usize];
    bounds_by_axis1[center_index1 as usize] = IntBounds {
        min: get_limit(&test, center, axis2, -1, limit2),
        max: get_limit(&test, center, axis2, 1, limit2),
    };
    let center_index2 = bounds_by_axis1[center_index1 as usize].min;

    for i in 1..=negative_delta1 {
        let last_bounds = bounds_by_axis1[(center_index1 - (i - 1)) as usize];
        let origin = center.relative_axis(axis1, -i);
        bounds_by_axis1[(center_index1 - i) as usize] = IntBounds {
            min: get_limit(&test, origin, axis2, -1, last_bounds.min),
            max: get_limit(&test, origin, axis2, 1, last_bounds.max),
        };
    }

    for i in 1..=positive_delta1 {
        let last_bounds = bounds_by_axis1[(center_index1 + i - 1) as usize];
        let origin = center.relative_axis(axis1, i);
        bounds_by_axis1[(center_index1 + i) as usize] = IntBounds {
            min: get_limit(&test, origin, axis2, -1, last_bounds.min),
            max: get_limit(&test, origin, axis2, 1, last_bounds.max),
        };
    }

    let mut min_axis1 = 0;
    let mut min_axis2 = 0;
    let mut size_axis1 = 0;
    let mut size_axis2 = 0;
    let mut columns = vec![0; bounds_by_axis1.len()];

    for i2 in (0..=center_index2).rev() {
        for (i1, bounds2) in bounds_by_axis1.iter().enumerate() {
            let min2 = center_index2 - bounds2.min;
            let max2 = center_index2 + bounds2.max;
            columns[i1] = if i2 >= min2 && i2 <= max2 {
                max2 + 1 - i2
            } else {
                0
            };
        }

        let (bounds_axis1, new_size_axis2) = get_max_rectangle_location(&columns);
        let new_size_axis1 = 1 + bounds_axis1.max - bounds_axis1.min;
        if new_size_axis1 * new_size_axis2 > size_axis1 * size_axis2 {
            min_axis1 = bounds_axis1.min;
            min_axis2 = i2;
            size_axis1 = new_size_axis1;
            size_axis2 = new_size_axis2;
        }
    }

    FoundRectangle {
        min_corner: center
            .relative_axis(axis1, min_axis1 - center_index1)
            .relative_axis(axis2, min_axis2 - center_index2),
        axis1_size: size_axis1,
        axis2_size: size_axis2,
    }
}

fn get_limit(
    test: &impl Fn(BlockPos) -> bool,
    start: BlockPos,
    axis: Axis,
    step: i32,
    limit: i32,
) -> i32 {
    let mut max = 0;
    let mut pos = start;
    while max < limit {
        pos = pos.relative_axis(axis, step);
        if !test(pos) {
            break;
        }
        max += 1;
    }
    max
}

fn get_max_rectangle_location(columns: &[i32]) -> (IntBounds, i32) {
    let mut max_start = 0;
    let mut max_end = 0;
    let mut max_height = 0;
    let mut stack = vec![0_usize];

    for column in 1..=columns.len() {
        let height = if column == columns.len() {
            0
        } else {
            columns[column]
        };

        while let Some(&top) = stack.last() {
            let stack_height = columns[top];
            if height >= stack_height {
                stack.push(column);
                break;
            }

            stack.pop();
            let start = stack.last().map_or(0, |top| top + 1);
            if stack_height * (column - start) as i32 > max_height * (max_end - max_start) as i32 {
                max_end = column;
                max_start = start;
                max_height = stack_height;
            }
        }

        if stack.is_empty() {
            stack.push(column);
        }
    }

    (
        IntBounds {
            min: max_start as i32,
            max: max_end as i32 - 1,
        },
        max_height,
    )
}

#[cfg(test)]
mod tests {
    use super::{IntBounds, get_largest_rectangle_around, get_max_rectangle_location};
    use crate::{BlockPos, axis::Axis};

    #[test]
    fn max_rectangle_location_matches_vanilla_histogram_scan() {
        let (bounds, height) = get_max_rectangle_location(&[2, 1, 5, 6, 2, 3]);

        assert_eq!(bounds, IntBounds { min: 2, max: 3 });
        assert_eq!(height, 5);
    }

    #[test]
    fn largest_rectangle_around_scans_both_axes_from_center() {
        let center = BlockPos::ZERO;
        let rectangle = get_largest_rectangle_around(center, Axis::X, 3, Axis::Y, 3, |pos| {
            (-1..=2).contains(&pos.x()) && (-1..=1).contains(&pos.y()) && pos.z() == 0
        });

        assert_eq!(rectangle.min_corner, BlockPos::new(-1, -1, 0));
        assert_eq!(rectangle.axis1_size, 4);
        assert_eq!(rectangle.axis2_size, 3);
    }
}
