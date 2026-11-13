//! Vanilla `net.minecraft.util.KeyframeTrack` and `KeyframeTrackSampler`.
//!
//! Vanilla bakes a segment list once per sampler; Steel walks the same segment
//! sequence on demand so tracks can live in static generated data.

use crate::easing::EasingType;

/// Interpolates between two values, as vanilla's `LerpFunction`.
pub type LerpFunction<T> = fn(alpha: f32, from: &T, to: &T) -> T;

/// A value pinned to a tick of a keyframe track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keyframe<T> {
    /// Tick of the keyframe within its track.
    pub ticks: i32,
    /// Value reached at `ticks`.
    pub value: T,
}

/// Keyframes ordered by tick, eased with one curve between each pair.
#[derive(Debug, Clone, Copy)]
pub struct KeyframeTrack<T: 'static> {
    keyframes: &'static [Keyframe<T>],
    easing: EasingType,
}

struct Segment<'a, T> {
    easing: EasingType,
    from_value: &'a T,
    from_ticks: i64,
    to_value: &'a T,
    to_ticks: i64,
}

impl<T: Clone> KeyframeTrack<T> {
    /// Creates a track from keyframes ordered by tick.
    ///
    /// # Panics
    ///
    /// Panics if `keyframes` is empty, matching vanilla's constructor check.
    #[must_use]
    pub const fn new(keyframes: &'static [Keyframe<T>], easing: EasingType) -> Self {
        assert!(!keyframes.is_empty(), "Track has no keyframes");
        Self { keyframes, easing }
    }

    /// Returns the keyframes, ordered by tick.
    #[must_use]
    pub const fn keyframes(&self) -> &'static [Keyframe<T>] {
        self.keyframes
    }

    /// Returns the easing applied between each keyframe pair.
    #[must_use]
    pub const fn easing(&self) -> EasingType {
        self.easing
    }

    /// Samples the track at `ticks`, looping over `period_ticks` when present.
    #[must_use]
    pub fn sample(&self, period_ticks: Option<i32>, ticks: i64, lerp: LerpFunction<T>) -> T {
        let sample_ticks = period_ticks.map_or(ticks, |period| ticks.rem_euclid(i64::from(period)));
        let segment = self.segment_at(period_ticks, sample_ticks);
        if sample_ticks <= segment.from_ticks {
            return segment.from_value.clone();
        }
        if sample_ticks >= segment.to_ticks {
            return segment.to_value.clone();
        }

        let alpha = (sample_ticks - segment.from_ticks) as f32
            / (segment.to_ticks - segment.from_ticks) as f32;
        let eased_alpha = segment.easing.apply(alpha);
        lerp(eased_alpha, segment.from_value, segment.to_value)
    }

    fn segment_at(&self, period_ticks: Option<i32>, sample_ticks: i64) -> Segment<'_, T> {
        let keyframes = self.keyframes;
        let first = &keyframes[0];
        let last = &keyframes[keyframes.len() - 1];
        if keyframes.len() == 1 {
            return Segment {
                easing: EasingType::Constant,
                from_value: &first.value,
                from_ticks: 0,
                to_value: &first.value,
                to_ticks: 0,
            };
        }

        if let Some(period) = period_ticks {
            let wrap_in = self.segment(last, i64::from(last.ticks - period), first, first.ticks);
            if sample_ticks < wrap_in.to_ticks {
                return wrap_in;
            }
        }

        let last_pair_index = keyframes.len() - 2;
        for (index, pair) in keyframes.windows(2).enumerate() {
            let segment = self.segment(&pair[0], i64::from(pair[0].ticks), &pair[1], pair[1].ticks);
            if sample_ticks < segment.to_ticks
                || (period_ticks.is_none() && index == last_pair_index)
            {
                return segment;
            }
        }

        let Some(period) = period_ticks else {
            unreachable!("the final keyframe pair is returned for aperiodic tracks");
        };
        self.segment(last, i64::from(last.ticks), first, first.ticks + period)
    }

    const fn segment<'a>(
        &self,
        from: &'a Keyframe<T>,
        from_ticks: i64,
        to: &'a Keyframe<T>,
        to_ticks: i32,
    ) -> Segment<'a, T> {
        Segment {
            easing: self.easing,
            from_value: &from.value,
            from_ticks,
            to_value: &to.value,
            to_ticks: to_ticks as i64,
        }
    }
}

/// Linear float interpolation, as vanilla's `Mth.lerp(float, float, float)`.
#[must_use]
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "signature must match LerpFunction, which is shared with non-Copy values"
)]
pub fn lerp_float(alpha: f32, from: &f32, to: &f32) -> f32 {
    from + alpha * (to - from)
}

/// Integer interpolation, as vanilla's `Mth.lerpInt`.
#[must_use]
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "signature must match LerpFunction, which is shared with non-Copy values"
)]
pub fn lerp_int(alpha: f32, from: &i32, to: &i32) -> i32 {
    from.wrapping_add((alpha * to.wrapping_sub(*from) as f32).floor() as i32)
}

/// Holds `from` until `alpha` reaches 1, as vanilla's `LerpFunction.ofConstant`.
#[must_use]
pub fn lerp_constant<T: Clone>(alpha: f32, from: &T, to: &T) -> T {
    lerp_step(1.0, alpha, from, to)
}

/// Switches from `from` to `to` once `alpha` reaches `threshold`, as vanilla's
/// `LerpFunction.ofStep`.
#[must_use]
pub fn lerp_step<T: Clone>(threshold: f32, alpha: f32, from: &T, to: &T) -> T {
    if alpha >= threshold {
        to.clone()
    } else {
        from.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{Keyframe, KeyframeTrack, lerp_constant, lerp_float};
    use crate::easing::EasingType;

    static RAMP: [Keyframe<f32>; 2] = [
        Keyframe {
            ticks: 100,
            value: 0.0,
        },
        Keyframe {
            ticks: 300,
            value: 1.0,
        },
    ];

    #[test]
    fn aperiodic_track_clamps_outside_keyframes() {
        let track = KeyframeTrack::new(&RAMP, EasingType::Linear);
        assert_eq!(track.sample(None, 0, lerp_float), 0.0);
        assert_eq!(track.sample(None, 200, lerp_float), 0.5);
        assert_eq!(track.sample(None, 1_000, lerp_float), 1.0);
    }

    #[test]
    fn periodic_track_wraps_between_last_and_first_keyframe() {
        let track = KeyframeTrack::new(&RAMP, EasingType::Linear);
        // After the last keyframe the wrap-out segment spans 300..500 (value 1 to 0).
        assert_eq!(track.sample(Some(400), 350, lerp_float), 0.75);
        // Before the first keyframe the wrap-in segment spans -100..100.
        assert_eq!(track.sample(Some(400), 0, lerp_float), 0.5);
        assert_eq!(track.sample(Some(400), 400, lerp_float), 0.5);
    }

    #[test]
    fn constant_lerp_holds_segment_start() {
        static FLAGS: [Keyframe<bool>; 2] = [
            Keyframe {
                ticks: 0,
                value: false,
            },
            Keyframe {
                ticks: 10,
                value: true,
            },
        ];
        let track = KeyframeTrack::new(&FLAGS, EasingType::Linear);
        assert!(!track.sample(None, 9, lerp_constant));
        assert!(track.sample(None, 10, lerp_constant));
    }
}
