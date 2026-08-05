use std::str::FromStr as _;

use steel_registry::dimension_type::DimensionTypeRef;
use steel_registry::timeline::{Ease, KeyframeValue, TimelineRef, Track};
use steel_registry::{REGISTRY, RegistryExt as _, TaggedRegistryExt as _};
use steel_utils::Identifier;

use super::clock::WorldClockManager;

const SKY_LIGHT_LEVEL_ATTRIBUTE: &str = "minecraft:gameplay/sky_light_level";
const SUN_ANGLE_ATTRIBUTE: &str = "minecraft:visual/sun_angle";
const DEFAULT_SKY_LIGHT_LEVEL: f32 = 15.0;
const DEFAULT_SUN_ANGLE: f32 = 0.0;
const MIN_SKY_LIGHT_LEVEL: f32 = 0.0;
const MAX_SKY_LIGHT_LEVEL: f32 = 15.0;
const RAIN_SKY_LIGHT_TARGET: f32 = 4.0;
const RAIN_SKY_LIGHT_ALPHA: f32 = 0.3125;
const THUNDER_SKY_LIGHT_TARGET: f32 = 4.0;
const THUNDER_SKY_LIGHT_ALPHA: f32 = 0.527_343_75;

#[must_use]
pub(super) fn sky_light_level(
    dimension_type: DimensionTypeRef,
    clock_manager: &WorldClockManager,
    rain_level: f32,
    thunder_level: f32,
    can_have_weather: bool,
) -> f32 {
    let mut value = dimension_type
        .sky_light_level
        .unwrap_or(DEFAULT_SKY_LIGHT_LEVEL);
    value = apply_timeline_float_attribute(
        value,
        dimension_type,
        clock_manager,
        SKY_LIGHT_LEVEL_ATTRIBUTE,
    );
    if can_have_weather {
        value = apply_weather_sky_light_level(value, rain_level, thunder_level);
    }
    value.clamp(MIN_SKY_LIGHT_LEVEL, MAX_SKY_LIGHT_LEVEL)
}

#[must_use]
pub(super) fn sun_angle_degrees(
    dimension_type: DimensionTypeRef,
    clock_manager: &WorldClockManager,
) -> f32 {
    apply_timeline_float_attribute(
        DEFAULT_SUN_ANGLE,
        dimension_type,
        clock_manager,
        SUN_ANGLE_ATTRIBUTE,
    )
}

#[must_use]
pub(super) fn sky_darkening(sky_light_level: f32) -> u8 {
    (MAX_SKY_LIGHT_LEVEL - sky_light_level.clamp(MIN_SKY_LIGHT_LEVEL, MAX_SKY_LIGHT_LEVEL)) as u8
}

fn apply_timeline_float_attribute(
    mut value: f32,
    dimension_type: DimensionTypeRef,
    clock_manager: &WorldClockManager,
    attribute: &str,
) -> f32 {
    let Some(timelines) = dimension_type.timelines else {
        return value;
    };
    if let Some(tag) = timelines.strip_prefix('#') {
        let Ok(tag) = Identifier::from_str(tag) else {
            return value;
        };
        for timeline in REGISTRY.timelines.iter_tag(&tag) {
            value = apply_timeline_float_track(value, timeline, clock_manager, attribute);
        }
        return value;
    }

    let Ok(key) = Identifier::from_str(timelines) else {
        return value;
    };
    REGISTRY.timelines.by_key(&key).map_or(value, |timeline| {
        apply_timeline_float_track(value, timeline, clock_manager, attribute)
    })
}

fn apply_timeline_float_track(
    value: f32,
    timeline: TimelineRef,
    clock_manager: &WorldClockManager,
    attribute: &str,
) -> f32 {
    let Some(track) = timeline.tracks.iter().find(|track| track.name == attribute) else {
        return value;
    };
    let Some(total_ticks) = clock_manager.total_ticks(timeline.clock) else {
        return value;
    };
    let Some(sample) = sample_float_track(track, timeline.period_ticks.map(i64::from), total_ticks)
    else {
        return value;
    };
    match track.modifier {
        Some("multiply") => value * sample,
        None => sample,
        _ => value,
    }
}

fn sample_float_track(track: &Track, period_ticks: Option<i64>, ticks: i64) -> Option<f32> {
    let keyframes = track.keyframes;
    match keyframes.len() {
        0 => return None,
        1 => return keyframe_float_value(&keyframes[0].value),
        _ => {}
    }

    let sample_ticks = period_ticks.map_or(ticks, |period| ticks.rem_euclid(period));
    let first = &keyframes[0];
    let last = &keyframes[keyframes.len() - 1];

    if let Some(period) = period_ticks
        && sample_ticks < first.ticks
    {
        return interpolate_float_segment(
            track,
            last.ticks - period,
            &last.value,
            first.ticks,
            &first.value,
            sample_ticks,
        );
    }

    for segment in keyframes.windows(2) {
        let from = &segment[0];
        let to = &segment[1];
        if sample_ticks < to.ticks {
            return interpolate_float_segment(
                track,
                from.ticks,
                &from.value,
                to.ticks,
                &to.value,
                sample_ticks,
            );
        }
    }

    if let Some(period) = period_ticks {
        return interpolate_float_segment(
            track,
            last.ticks,
            &last.value,
            first.ticks + period,
            &first.value,
            sample_ticks,
        );
    }

    keyframe_float_value(&last.value)
}

fn interpolate_float_segment(
    track: &Track,
    from_ticks: i64,
    from_value: &KeyframeValue,
    to_ticks: i64,
    to_value: &KeyframeValue,
    sample_ticks: i64,
) -> Option<f32> {
    let from = keyframe_float_value(from_value)?;
    let to = keyframe_float_value(to_value)?;
    if sample_ticks <= from_ticks {
        return Some(from);
    }
    if sample_ticks >= to_ticks {
        return Some(to);
    }

    let alpha = (sample_ticks - from_ticks) as f32 / (to_ticks - from_ticks) as f32;
    let eased_alpha = apply_easing(track.ease.as_ref(), alpha)?;
    Some(from + eased_alpha * (to - from))
}

fn apply_easing(ease: Option<&Ease>, alpha: f32) -> Option<f32> {
    match ease {
        None | Some(Ease::Named("linear")) => Some(alpha),
        Some(Ease::Named("constant")) => Some(0.0),
        Some(Ease::CubicBezier([x1, y1, x2, y2])) => Some(cubic_bezier(alpha, *x1, *y1, *x2, *y2)),
        Some(Ease::Named(_)) => None,
    }
}

fn cubic_bezier(x: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let x_curve = CubicCurve::from_controls(x1, x2);
    let y_curve = CubicCurve::from_controls(y1, y2);
    y_curve.sample(x_curve.solve_t(x))
}

#[derive(Clone, Copy)]
struct CubicCurve {
    a: f32,
    b: f32,
    c: f32,
}

impl CubicCurve {
    const ERROR_EPSILON: f32 = 1.0E-5;

    fn from_controls(first: f32, second: f32) -> Self {
        Self {
            a: (3.0 * first - 3.0 * second) + 1.0,
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

    fn solve_t(self, x: f32) -> f32 {
        let mut t = x;
        for _ in 0..4 {
            let error = self.sample(t) - x;
            if error.abs() < Self::ERROR_EPSILON {
                return t;
            }
            let gradient = self.sample_gradient(t);
            if gradient < Self::ERROR_EPSILON {
                break;
            }
            t -= (error / gradient).clamp(-0.25, 0.25);
        }
        self.solve_t_bisect(x, t)
    }

    #[expect(
        clippy::manual_midpoint,
        reason = "evaluation order mirrors vanilla CubicBezier.solveTBisect float arithmetic"
    )]
    fn solve_t_bisect(self, x: f32, initial_t: f32) -> f32 {
        let mut lower = 0.0;
        let mut upper = 1.0;
        let mut t = initial_t;
        while lower < upper {
            let error = self.sample(t) - x;
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

const fn keyframe_float_value(value: &KeyframeValue) -> Option<f32> {
    match value {
        KeyframeValue::Float(value) => Some(*value),
        _ => None,
    }
}

fn apply_weather_sky_light_level(mut value: f32, rain_level: f32, thunder_level: f32) -> f32 {
    let thunder_level = thunder_level.clamp(0.0, 1.0);
    let rain_level = (rain_level - thunder_level).clamp(0.0, 1.0);
    if rain_level > 0.0 {
        let rain_value = lerp(RAIN_SKY_LIGHT_ALPHA, value, RAIN_SKY_LIGHT_TARGET);
        value = lerp(rain_level, value, rain_value);
    }
    if thunder_level > 0.0 {
        let thunder_value = lerp(THUNDER_SKY_LIGHT_ALPHA, value, THUNDER_SKY_LIGHT_TARGET);
        value = lerp(thunder_level, value, thunder_value);
    }
    value
}

fn lerp(alpha: f32, from: f32, to: f32) -> f32 {
    from + alpha * (to - from)
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_dimension_types::{OVERWORLD, THE_NETHER};
    use steel_registry::vanilla_world_clocks;

    use super::*;

    fn assert_f32_close(left: f32, right: f32) {
        assert!(
            (left - right).abs() < 0.000_001,
            "left={left}, right={right}"
        );
    }

    fn clock_manager_at(total_ticks: i64) -> WorldClockManager {
        let mut manager = WorldClockManager::new();
        assert_eq!(
            manager.set_total_ticks(&vanilla_world_clocks::OVERWORLD, total_ticks),
            Some(())
        );
        manager
    }

    #[test]
    fn overworld_sky_light_uses_generated_day_timeline() {
        init_test_registry();

        assert_f32_close(
            sky_light_level(&OVERWORLD, &clock_manager_at(6000), 0.0, 0.0, true),
            15.0,
        );
        assert_f32_close(
            sky_light_level(&OVERWORLD, &clock_manager_at(18000), 0.0, 0.0, true),
            4.0,
        );
    }

    #[test]
    fn overworld_sky_light_interpolates_sunset_from_generated_keyframes() {
        init_test_registry();

        assert_f32_close(
            sky_light_level(&OVERWORLD, &clock_manager_at(12_768), 0.0, 0.0, true),
            9.503_051,
        );
    }

    #[test]
    fn overworld_sun_angle_uses_vanilla_cubic_bezier_easing() {
        init_test_registry();

        assert_f32_close(
            sun_angle_degrees(&OVERWORLD, &clock_manager_at(0)),
            282.374_33,
        );
        assert_f32_close(
            sun_angle_degrees(&OVERWORLD, &clock_manager_at(12_000)),
            77.625_66,
        );
        assert_f32_close(
            sun_angle_degrees(&OVERWORLD, &clock_manager_at(18_000)),
            180.0,
        );
    }

    #[test]
    fn sky_light_level_applies_vanilla_weather_alpha_layers() {
        init_test_registry();

        assert_f32_close(
            sky_light_level(&OVERWORLD, &clock_manager_at(6000), 1.0, 0.0, true),
            11.5625,
        );
        assert_f32_close(
            sky_light_level(&OVERWORLD, &clock_manager_at(6000), 1.0, 1.0, true),
            9.199_219,
        );
    }

    #[test]
    fn fixed_nether_sky_light_uses_dimension_attribute() {
        init_test_registry();

        assert_f32_close(
            sky_light_level(&THE_NETHER, &clock_manager_at(6000), 0.0, 0.0, false),
            4.0,
        );
    }

    #[test]
    fn sky_darkening_matches_vanilla_integer_cast() {
        assert_eq!(sky_darkening(15.0), 0);
        assert_eq!(sky_darkening(11.5625), 3);
        assert_eq!(sky_darkening(4.0), 11);
    }
}
