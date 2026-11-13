/// Valid value range of an environment attribute, as vanilla's `AttributeRange`.
#[derive(Debug, Clone, Copy)]
pub enum AttributeRange<V> {
    /// Every value is valid.
    Any,
    /// Values are clamped into `[min, max]` by `clamp`.
    Bounded {
        min: V,
        max: V,
        clamp: fn(V, &V, &V) -> V,
    },
}

impl<V> AttributeRange<V> {
    /// Clamps `value` into the range.
    #[must_use]
    pub fn sanitize(&self, value: V) -> V {
        match self {
            Self::Any => value,
            Self::Bounded { min, max, clamp } => clamp(value, min, max),
        }
    }
}

impl AttributeRange<f32> {
    /// A float range, as vanilla's `AttributeRange.ofFloat`.
    #[must_use]
    pub const fn of_float(min: f32, max: f32) -> Self {
        Self::Bounded {
            min,
            max,
            clamp: clamp_float,
        }
    }
}

/// Vanilla `ofFloat.sanitize`: in-range values pass through, others go through
/// `Mth.clamp`, so `NaN` stays `NaN`.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "signature is shared with non-Copy attribute value types"
)]
fn clamp_float(value: f32, min: &f32, max: &f32) -> f32 {
    if value >= *min && value <= *max {
        return value;
    }
    if value < *min {
        return *min;
    }
    if value.is_nan() {
        value
    } else {
        max.min(value)
    }
}

#[cfg(test)]
mod tests {
    use super::AttributeRange;

    #[test]
    fn float_range_clamps_out_of_range_values() {
        let range = AttributeRange::of_float(0.0, 15.0);
        assert_eq!(range.sanitize(7.5), 7.5);
        assert_eq!(range.sanitize(-1.0), 0.0);
        assert_eq!(range.sanitize(20.0), 15.0);
        assert!(range.sanitize(f32::NAN).is_nan());
    }
}
