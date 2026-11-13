//! Vanilla `net.minecraft.util.EasingType` and `net.minecraft.util.Ease`.
//!
//! Float/double promotion follows the Java source so eased values stay
//! bit-compatible with vanilla.

use std::f64::consts::PI;

use crate::trig::{cos, sin};

const BACK_C1: f32 = 1.701_58;
const BACK_C3: f32 = 2.701_58;
const BACK_C2: f32 = 2.594_909_4;
const ELASTIC_C4: f32 = (PI * 2.0 / 3.0) as f32;
const BOUNCE_N1: f32 = 7.5625;
const HALF_PI: f32 = (PI / 2.0) as f32;
const PI_F32: f32 = PI as f32;

/// Easing curve applied to the interpolation alpha of a keyframe segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EasingType {
    /// Always 0: holds the segment start value.
    Constant,
    /// Identity: linear interpolation.
    Linear,
    /// Vanilla `Ease.inBack`.
    InBack,
    /// Vanilla `Ease.inBounce`.
    InBounce,
    /// Vanilla `Ease.inCirc`.
    InCirc,
    /// Vanilla `Ease.inCubic`.
    InCubic,
    /// Vanilla `Ease.inElastic`.
    InElastic,
    /// Vanilla `Ease.inExpo`.
    InExpo,
    /// Vanilla `Ease.inQuad`.
    InQuad,
    /// Vanilla `Ease.inQuart`.
    InQuart,
    /// Vanilla `Ease.inQuint`.
    InQuint,
    /// Vanilla `Ease.inSine`.
    InSine,
    /// Vanilla `Ease.inOutBack`.
    InOutBack,
    /// Vanilla `Ease.inOutBounce`.
    InOutBounce,
    /// Vanilla `Ease.inOutCirc`.
    InOutCirc,
    /// Vanilla `Ease.inOutCubic`.
    InOutCubic,
    /// Vanilla `Ease.inOutElastic`.
    InOutElastic,
    /// Vanilla `Ease.inOutExpo`.
    InOutExpo,
    /// Vanilla `Ease.inOutQuad`.
    InOutQuad,
    /// Vanilla `Ease.inOutQuart`.
    InOutQuart,
    /// Vanilla `Ease.inOutQuint`.
    InOutQuint,
    /// Vanilla `Ease.inOutSine`.
    InOutSine,
    /// Vanilla `Ease.outBack`.
    OutBack,
    /// Vanilla `Ease.outBounce`.
    OutBounce,
    /// Vanilla `Ease.outCirc`.
    OutCirc,
    /// Vanilla `Ease.outCubic`.
    OutCubic,
    /// Vanilla `Ease.outElastic`.
    OutElastic,
    /// Vanilla `Ease.outExpo`.
    OutExpo,
    /// Vanilla `Ease.outQuad`.
    OutQuad,
    /// Vanilla `Ease.outQuart`.
    OutQuart,
    /// Vanilla `Ease.outQuint`.
    OutQuint,
    /// Vanilla `Ease.outSine`.
    OutSine,
    /// CSS-style cubic bezier curve through the given control points.
    CubicBezier(CubicBezierControls),
}

/// Control points of a cubic bezier easing curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CubicBezierControls {
    /// X of the first control point.
    pub x1: f32,
    /// Y of the first control point.
    pub y1: f32,
    /// X of the second control point.
    pub x2: f32,
    /// Y of the second control point.
    pub y2: f32,
}

impl EasingType {
    /// Every named easing, paired with its serialized name.
    pub const NAMED: [(&'static str, Self); 32] = [
        ("constant", Self::Constant),
        ("linear", Self::Linear),
        ("in_back", Self::InBack),
        ("in_bounce", Self::InBounce),
        ("in_circ", Self::InCirc),
        ("in_cubic", Self::InCubic),
        ("in_elastic", Self::InElastic),
        ("in_expo", Self::InExpo),
        ("in_quad", Self::InQuad),
        ("in_quart", Self::InQuart),
        ("in_quint", Self::InQuint),
        ("in_sine", Self::InSine),
        ("in_out_back", Self::InOutBack),
        ("in_out_bounce", Self::InOutBounce),
        ("in_out_circ", Self::InOutCirc),
        ("in_out_cubic", Self::InOutCubic),
        ("in_out_elastic", Self::InOutElastic),
        ("in_out_expo", Self::InOutExpo),
        ("in_out_quad", Self::InOutQuad),
        ("in_out_quart", Self::InOutQuart),
        ("in_out_quint", Self::InOutQuint),
        ("in_out_sine", Self::InOutSine),
        ("out_back", Self::OutBack),
        ("out_bounce", Self::OutBounce),
        ("out_circ", Self::OutCirc),
        ("out_cubic", Self::OutCubic),
        ("out_elastic", Self::OutElastic),
        ("out_expo", Self::OutExpo),
        ("out_quad", Self::OutQuad),
        ("out_quart", Self::OutQuart),
        ("out_quint", Self::OutQuint),
        ("out_sine", Self::OutSine),
    ];

    /// Looks up a named easing by its serialized name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::NAMED
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, easing)| *easing)
    }

    /// Returns the serialized name, or `None` for a cubic bezier.
    #[must_use]
    pub fn name(self) -> Option<&'static str> {
        Self::NAMED
            .iter()
            .find(|(_, easing)| *easing == self)
            .map(|(name, _)| *name)
    }

    /// Eases `x`, the linear progress through a segment in `[0, 1]`.
    #[must_use]
    #[expect(
        clippy::float_cmp,
        reason = "vanilla special-cases the exact segment endpoints"
    )]
    #[expect(
        clippy::manual_midpoint,
        reason = "evaluation order mirrors vanilla Ease float arithmetic"
    )]
    pub fn apply(self, x: f32) -> f32 {
        match self {
            Self::Constant => 0.0,
            Self::Linear => x,
            Self::InBack => square(x) * (BACK_C3 * x - BACK_C1),
            Self::InBounce => 1.0 - out_bounce(1.0 - x),
            Self::InCirc => (-f64::from(1.0 - x * x).sqrt()) as f32 + 1.0,
            Self::InCubic => cube(x),
            Self::InElastic => in_elastic(x),
            Self::InExpo => {
                if x == 0.0 {
                    0.0
                } else {
                    2.0_f64.powf(10.0 * f64::from(x) - 10.0) as f32
                }
            }
            Self::InQuad => x * x,
            Self::InQuart => square(square(x)),
            Self::InQuint => square(square(x)) * x,
            Self::InSine => 1.0 - cos(x * HALF_PI),
            Self::InOutBack => in_out_back(x),
            Self::InOutBounce => {
                if x < 0.5 {
                    (1.0 - out_bounce(1.0 - 2.0 * x)) / 2.0
                } else {
                    (1.0 + out_bounce(2.0 * x - 1.0)) / 2.0
                }
            }
            Self::InOutCirc => in_out_circ(x),
            Self::InOutCubic => {
                if x < 0.5 {
                    4.0 * cube(x)
                } else {
                    (1.0 - (-2.0 * f64::from(x) + 2.0).powf(3.0) / 2.0) as f32
                }
            }
            Self::InOutElastic => in_out_elastic(x),
            Self::InOutExpo => in_out_expo(x),
            Self::InOutQuad => {
                if x < 0.5 {
                    2.0 * square(x)
                } else {
                    (1.0 - (-2.0 * f64::from(x) + 2.0).powf(2.0) / 2.0) as f32
                }
            }
            Self::InOutQuart => {
                if x < 0.5 {
                    8.0 * square(square(x))
                } else {
                    (1.0 - (-2.0 * f64::from(x) + 2.0).powf(4.0) / 2.0) as f32
                }
            }
            Self::InOutQuint => {
                if f64::from(x) < 0.5 {
                    16.0 * x * x * x * x * x
                } else {
                    (1.0 - (-2.0 * f64::from(x) + 2.0).powf(5.0) / 2.0) as f32
                }
            }
            Self::InOutSine => -(cos(PI_F32 * x) - 1.0) / 2.0,
            Self::OutBack => 1.0 + BACK_C3 * cube(x - 1.0) + BACK_C1 * square(x - 1.0),
            Self::OutBounce => out_bounce(x),
            Self::OutCirc => f64::from(1.0 - square(x - 1.0)).sqrt() as f32,
            Self::OutCubic => 1.0 - cube(1.0 - x),
            Self::OutElastic => out_elastic(x),
            Self::OutExpo => {
                if x == 1.0 {
                    1.0
                } else {
                    1.0 - 2.0_f64.powf(-10.0 * f64::from(x)) as f32
                }
            }
            Self::OutQuad => 1.0 - square(1.0 - x),
            Self::OutQuart => 1.0 - square(square(1.0 - x)),
            Self::OutQuint => 1.0 - (1.0 - f64::from(x)).powf(5.0) as f32,
            Self::OutSine => sin(x * HALF_PI),
            Self::CubicBezier(controls) => controls.apply(x),
        }
    }
}

impl CubicBezierControls {
    const NEWTON_RAPHSON_ITERATIONS: usize = 4;
    const MAX_STEP: f32 = 0.25;
    const ERROR_EPSILON: f32 = 1.0E-5;

    fn apply(self, x: f32) -> f32 {
        let x_curve = CubicCurve::from_controls(self.x1, self.x2);
        let y_curve = CubicCurve::from_controls(self.y1, self.y2);
        y_curve.sample(Self::solve_t(x_curve, x))
    }

    fn solve_t(x_curve: CubicCurve, x: f32) -> f32 {
        let mut t = x;
        for _ in 0..Self::NEWTON_RAPHSON_ITERATIONS {
            let error = x_curve.sample(t) - x;
            if error.abs() < Self::ERROR_EPSILON {
                return t;
            }
            let gradient = x_curve.sample_gradient(t);
            if gradient < Self::ERROR_EPSILON {
                break;
            }
            t -= (error / gradient).clamp(-Self::MAX_STEP, Self::MAX_STEP);
        }
        Self::solve_t_bisect(x_curve, x, t)
    }

    #[expect(
        clippy::manual_midpoint,
        reason = "evaluation order mirrors vanilla CubicBezier.solveTBisect float arithmetic"
    )]
    fn solve_t_bisect(x_curve: CubicCurve, x: f32, initial_t: f32) -> f32 {
        let mut lower = 0.0;
        let mut upper = 1.0;
        let mut t = initial_t;
        while lower < upper {
            let error = x_curve.sample(t) - x;
            if error.abs() < Self::ERROR_EPSILON {
                return t;
            }
            if error < 0.0 {
                lower = t;
            } else {
                upper = t;
            }
            t = (upper + lower) / 2.0;
        }
        t
    }
}

#[derive(Clone, Copy)]
struct CubicCurve {
    a: f32,
    b: f32,
    c: f32,
}

impl CubicCurve {
    fn from_controls(first: f32, second: f32) -> Self {
        Self {
            a: 3.0 * first - 3.0 * second + 1.0,
            b: -6.0 * first + 3.0 * second,
            c: 3.0 * first,
        }
    }

    fn sample(self, t: f32) -> f32 {
        ((self.a * t + self.b) * t + self.c) * t
    }

    fn sample_gradient(self, t: f32) -> f32 {
        (3.0 * self.a * t + 2.0 * self.b) * t + self.c
    }
}

fn square(x: f32) -> f32 {
    x * x
}

fn cube(x: f32) -> f32 {
    x * x * x
}

fn out_bounce(x: f32) -> f32 {
    if x < 0.363_636_37 {
        BOUNCE_N1 * square(x)
    } else if x < 0.727_272_75 {
        BOUNCE_N1 * square(x - 0.545_454_56) + 0.75
    } else if f64::from(x) < 0.909_090_909_090_909_1 {
        BOUNCE_N1 * square(x - 0.818_181_8) + 0.9375
    } else {
        BOUNCE_N1 * square(x - 0.954_545_44) + 0.984_375
    }
}

#[expect(
    clippy::float_cmp,
    reason = "vanilla special-cases the exact segment endpoints"
)]
fn in_elastic(x: f32) -> f32 {
    if x == 0.0 {
        return 0.0;
    }
    if x == 1.0 {
        return 1.0;
    }
    let x = f64::from(x);
    (-(2.0_f64.powf(10.0 * x - 10.0)) * ((x * 10.0 - 10.75) * f64::from(ELASTIC_C4)).sin()) as f32
}

#[expect(
    clippy::float_cmp,
    reason = "vanilla special-cases the exact segment endpoints"
)]
fn out_elastic(x: f32) -> f32 {
    if x == 0.0 {
        return 0.0;
    }
    if x == 1.0 {
        return 1.0;
    }
    let x = f64::from(x);
    (2.0_f64.powf(-10.0 * x) * ((x * 10.0 - 0.75) * f64::from(ELASTIC_C4)).sin() + 1.0) as f32
}

#[expect(
    clippy::float_cmp,
    reason = "vanilla special-cases the exact segment endpoints"
)]
fn in_out_elastic(x: f32) -> f32 {
    if x == 0.0 {
        return 0.0;
    }
    if x == 1.0 {
        return 1.0;
    }
    // Vanilla evaluates `(20.0 * x - 11.125) * (float) Math.PI * 4.0F / 9.0F` left to right in
    // double precision after promoting the float factors.
    let x_double = f64::from(x);
    let sin = ((20.0 * x_double - 11.125) * f64::from(PI_F32) * 4.0 / 9.0).sin();
    if x < 0.5 {
        (-(2.0_f64.powf(20.0 * x_double - 10.0) * sin) / 2.0) as f32
    } else {
        (2.0_f64.powf(-20.0 * x_double + 10.0) * sin / 2.0 + 1.0) as f32
    }
}

#[expect(
    clippy::float_cmp,
    reason = "vanilla special-cases the exact segment endpoints"
)]
fn in_out_expo(x: f32) -> f32 {
    let x_double = f64::from(x);
    if x < 0.5 {
        if x == 0.0 {
            0.0
        } else {
            (2.0_f64.powf(20.0 * x_double - 10.0) / 2.0) as f32
        }
    } else if x == 1.0 {
        1.0
    } else {
        ((2.0 - 2.0_f64.powf(-20.0 * x_double + 10.0)) / 2.0) as f32
    }
}

#[expect(
    clippy::manual_midpoint,
    reason = "evaluation order mirrors vanilla Ease float arithmetic"
)]
fn in_out_circ(x: f32) -> f32 {
    let x_double = f64::from(x);
    if x < 0.5 {
        ((1.0 - (1.0 - (2.0 * x_double).powf(2.0)).sqrt()) / 2.0) as f32
    } else {
        (((1.0 - (-2.0 * x_double + 2.0).powf(2.0)).sqrt() + 1.0) / 2.0) as f32
    }
}

#[expect(
    clippy::manual_midpoint,
    reason = "evaluation order mirrors vanilla Ease float arithmetic"
)]
fn in_out_back(x: f32) -> f32 {
    if x < 0.5 {
        return 4.0 * x * x * (7.189_819 * x - BACK_C2) / 2.0;
    }
    let dt = 2.0 * x - 2.0;
    (dt * dt * (3.594_909_4 * dt + BACK_C2) + 2.0) / 2.0
}

#[cfg(test)]
mod tests {
    use super::{CubicBezierControls, EasingType};

    #[test]
    fn named_easings_round_trip() {
        for (name, easing) in EasingType::NAMED {
            assert_eq!(EasingType::from_name(name), Some(easing));
            assert_eq!(easing.name(), Some(name));
        }
        assert_eq!(EasingType::from_name("unknown"), None);
    }

    #[test]
    fn named_easings_hit_segment_endpoints() {
        for (name, easing) in EasingType::NAMED {
            if easing == EasingType::Constant {
                continue;
            }
            assert!(easing.apply(0.0).abs() < 1.0E-6, "{name} at 0");
            assert!((easing.apply(1.0) - 1.0).abs() < 1.0E-6, "{name} at 1");
        }
    }

    #[test]
    fn vanilla_sky_angle_bezier_is_symmetric() {
        let easing = EasingType::CubicBezier(CubicBezierControls {
            x1: 0.362,
            y1: 0.241,
            x2: 0.638,
            y2: 0.759,
        });
        assert!((easing.apply(0.5) - 0.5).abs() < 1.0E-4);
        let low = easing.apply(0.25);
        let high = easing.apply(0.75);
        assert!((low + high - 1.0).abs() < 1.0E-4);
    }
}
