//! Vanilla `Mth` helpers shared by gameplay systems.

/// Returns vanilla `Mth.ceillog2`: the number of bits needed to represent
/// every id in `0..n`.
#[must_use]
pub const fn ceil_log2(n: usize) -> u8 {
    if n <= 1 { 0 } else { (n - 1).bit_width() as u8 }
}

#[cfg(test)]
mod tests {
    use super::ceil_log2;

    #[test]
    fn ceil_log2_matches_vanilla_worked_examples() {
        assert_eq!(ceil_log2(0), 0);
        assert_eq!(ceil_log2(1), 0);
        assert_eq!(ceil_log2(64), 6);
        assert_eq!(ceil_log2(65), 7);
        assert_eq!(ceil_log2(32_366), 15);
        assert_eq!(ceil_log2(35_723), 16);
    }
}
