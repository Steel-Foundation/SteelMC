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
        // 64 possible ids still fit in 6 bits (ids 0..=63).
        assert_eq!(ceil_log2(64), 6);
        // A 65th id needs a 7th bit, matching the issue's 66-biome regression.
        assert_eq!(ceil_log2(65), 7);
        // 26.2's block-state count needs 15 bits, matching the old hardcoded value.
        assert_eq!(ceil_log2(32_366), 15);
        // 26.3-rc-1's block-state count needs a 16th bit.
        assert_eq!(ceil_log2(35_723), 16);
    }
}
