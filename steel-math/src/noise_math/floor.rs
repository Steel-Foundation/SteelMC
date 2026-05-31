/// Floor function that matches Java behavior.
///
/// In Java, `(int)v` truncates toward zero, but we need floor behavior.
/// For negative values, we need to subtract 1 if there's a fractional part.
///
/// Java reference: `Mth.floor(double)`
#[expect(clippy::inline_always, reason = "hot-path noise primitive")]
#[inline(always)]
#[must_use]
pub fn floor(v: f64) -> i32 {
    let i = v as i32;
    if v < f64::from(i) { i - 1 } else { i }
}

/// Long floor function matching Java behavior.
///
/// Java reference: `Mth.lfloor(double)`
#[inline]
#[must_use]
pub fn lfloor(v: f64) -> i64 {
    let i = v as i64;
    if v < i as f64 { i - 1 } else { i }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_floor() {
        assert_eq!(floor(1.5), 1);
        assert_eq!(floor(1.0), 1);
        assert_eq!(floor(0.5), 0);
        assert_eq!(floor(0.0), 0);
        assert_eq!(floor(-0.5), -1);
        assert_eq!(floor(-1.0), -1);
        assert_eq!(floor(-1.5), -2);
    }
}
