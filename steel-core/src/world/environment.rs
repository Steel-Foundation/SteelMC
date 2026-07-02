use std::str::FromStr as _;

use steel_registry::dimension_type::DimensionTypeRef;
use steel_registry::timeline::{Ease, KeyframeValue, TimelineRef, Track};
use steel_registry::{REGISTRY, RegistryExt as _, TaggedRegistryExt as _};
use steel_utils::Identifier;

const SKY_LIGHT_LEVEL_ATTRIBUTE: &str = "minecraft:gameplay/sky_light_level";
const DEFAULT_SKY_LIGHT_LEVEL: f32 = 15.0;
const MIN_SKY_LIGHT_LEVEL: f32 = 0.0;
const MAX_SKY_LIGHT_LEVEL: f32 = 15.0;
const RAIN_SKY_LIGHT_TARGET: f32 = 4.0;
const RAIN_SKY_LIGHT_ALPHA: f32 = 0.3125;
const THUNDER_SKY_LIGHT_TARGET: f32 = 4.0;
const THUNDER_SKY_LIGHT_ALPHA: f32 = 0.527_343_75;

#[must_use]
pub(super) fn sky_light_level(
    dimension_type: DimensionTypeRef,
    day_time: i64,
    rain_level: f32,
    thunder_level: f32,
    can_have_weather: bool,
) -> f32 {
    let mut value = dimension_type
        .sky_light_level
        .unwrap_or(DEFAULT_SKY_LIGHT_LEVEL);
    value = apply_timeline_sky_light_level(value, dimension_type, day_time);
    if can_have_weather {
        value = apply_weather_sky_light_level(value, rain_level, thunder_level);
    }
    value.clamp(MIN_SKY_LIGHT_LEVEL, MAX_SKY_LIGHT_LEVEL)
}

#[must_use]
pub(super) fn sky_darkening(sky_light_level: f32) -> u8 {
    (MAX_SKY_LIGHT_LEVEL - sky_light_level.clamp(MIN_SKY_LIGHT_LEVEL, MAX_SKY_LIGHT_LEVEL)) as u8
}

fn apply_timeline_sky_light_level(
    mut value: f32,
    dimension_type: DimensionTypeRef,
    day_time: i64,
) -> f32 {
    let Some(timelines) = dimension_type.timelines else {
        return value;
    };
    if let Some(tag) = timelines.strip_prefix('#') {
        let Ok(tag) = Identifier::from_str(tag) else {
            return value;
        };
        for timeline in REGISTRY.timelines.iter_tag(&tag) {
            value = apply_timeline_sky_light_level_track(value, timeline, day_time);
        }
        return value;
    }

    let Ok(key) = Identifier::from_str(timelines) else {
        return value;
    };
    REGISTRY.timelines.by_key(&key).map_or(value, |timeline| {
        apply_timeline_sky_light_level_track(value, timeline, day_time)
    })
}

fn apply_timeline_sky_light_level_track(value: f32, timeline: TimelineRef, day_time: i64) -> f32 {
    let Some(track) = timeline
        .tracks
        .iter()
        .find(|track| track.name == SKY_LIGHT_LEVEL_ATTRIBUTE)
    else {
        return value;
    };
    let Some(sample) = sample_float_track(track, timeline.period_ticks, day_time) else {
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
    let eased_alpha = if matches!(track.ease, Some(Ease::Named("constant"))) {
        0.0
    } else {
        alpha
    };
    Some(from + eased_alpha * (to - from))
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

    use super::*;

    fn assert_f32_close(left: f32, right: f32) {
        assert!(
            (left - right).abs() < 0.000_001,
            "left={left}, right={right}"
        );
    }

    #[test]
    fn overworld_sky_light_uses_generated_day_timeline() {
        init_test_registry();

        assert_f32_close(sky_light_level(&OVERWORLD, 6000, 0.0, 0.0, true), 15.0);
        assert_f32_close(sky_light_level(&OVERWORLD, 18000, 0.0, 0.0, true), 4.0);
    }

    #[test]
    fn overworld_sky_light_interpolates_sunset_from_generated_keyframes() {
        init_test_registry();

        assert_f32_close(
            sky_light_level(&OVERWORLD, 12_768, 0.0, 0.0, true),
            9.503_051,
        );
    }

    #[test]
    fn sky_light_level_applies_vanilla_weather_alpha_layers() {
        init_test_registry();

        assert_f32_close(sky_light_level(&OVERWORLD, 6000, 1.0, 0.0, true), 11.5625);
        assert_f32_close(sky_light_level(&OVERWORLD, 6000, 1.0, 1.0, true), 9.199_219);
    }

    #[test]
    fn fixed_nether_sky_light_uses_dimension_attribute() {
        init_test_registry();

        assert_f32_close(sky_light_level(&THE_NETHER, 6000, 0.0, 0.0, false), 4.0);
    }

    #[test]
    fn sky_darkening_matches_vanilla_integer_cast() {
        assert_eq!(sky_darkening(15.0), 0);
        assert_eq!(sky_darkening(11.5625), 3);
        assert_eq!(sky_darkening(4.0), 11);
    }
}
