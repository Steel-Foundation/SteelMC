//! Vanilla `Mth` helpers shared by gameplay systems.

/// Returns vanilla `Mth.ceillog2`: the number of bits needed to represent
/// every id in `0..n`.
#[must_use]
pub const fn ceil_log2(n: usize) -> u8 {
    if n <= 1 { 0 } else { (n - 1).bit_width() as u8 }
}

/// Returns vanilla `Mth.outFromOrigin`: the values `origin`, `origin + step`,
/// `origin - step`, `origin + 2 * step`, ... clamped to `[lower_bound, upper_bound]`.
///
/// The origin itself is included first. Values are skipped once both sides would
/// leave the bounds, matching vanilla's `IntStream.iterate` termination.
///
/// # Panics
///
/// Panics if `lower_bound > upper_bound` or `step_size < 1`.
#[must_use]
pub fn out_from_origin(
    origin: i32,
    lower_bound: i32,
    upper_bound: i32,
    step_size: i32,
) -> Vec<i32> {
    assert!(
        lower_bound <= upper_bound,
        "upper bound must not be below lower bound"
    );
    assert!(step_size >= 1, "step size must be at least 1");

    let clamped_origin = origin.clamp(lower_bound, upper_bound);
    let mut values = Vec::new();
    let mut cursor = clamped_origin;

    loop {
        let distance = (clamped_origin - cursor).abs();
        if clamped_origin - distance < lower_bound && clamped_origin + distance > upper_bound {
            break;
        }

        values.push(cursor);

        let previous_was_negative = cursor <= clamped_origin;
        let can_move_positive = clamped_origin + distance + step_size <= upper_bound;
        cursor = if !previous_was_negative || !can_move_positive {
            let attempted =
                clamped_origin - distance - if previous_was_negative { step_size } else { 0 };
            if attempted >= lower_bound {
                attempted
            } else {
                clamped_origin + distance + step_size
            }
        } else {
            clamped_origin + distance + step_size
        };
    }

    values
}

#[cfg(test)]
mod tests {
    use super::{ceil_log2, out_from_origin};

    #[test]
    fn ceil_log2_matches_vanilla_worked_examples() {
        assert_eq!(ceil_log2(0), 0);
        assert_eq!(ceil_log2(1), 0);
        assert_eq!(ceil_log2(64), 6);
        assert_eq!(ceil_log2(65), 7);
        assert_eq!(ceil_log2(32_366), 15);
        assert_eq!(ceil_log2(35_723), 16);
    }

    #[test]
    fn out_from_origin_alternates_outward_and_respects_bounds() {
        assert_eq!(
            out_from_origin(64, -63, 320, 64),
            vec![64, 128, 0, 192, 256, 320]
        );
        assert_eq!(
            out_from_origin(0, 0, 100, 10),
            vec![0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100]
        );
        assert_eq!(out_from_origin(500, 0, 10, 4), vec![10, 6, 2]);
        assert_eq!(out_from_origin(-500, 0, 10, 4), vec![0, 4, 8]);
    }
}
