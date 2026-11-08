//! Vanilla `WeatherAttributes`: modifiers blended in while it rains or thunders.

use glam::{Vec3, Vec4};
use steel_registry::environment_attribute::{
    AttributeModifierEntry, AttributeValue, BlendToGray, BlendToGrayModifier,
    ColorAlphaBlendModifier, ColorMultiplyModifier, EnvironmentAttribute, EnvironmentAttributeMap,
    EnvironmentAttributeMapEntry, FloatAlphaBlendModifier, FloatWithAlpha, OverrideModifier, argb,
};
use steel_registry::vanilla_environment_attributes::{
    BEES_STAY_IN_HIVE, CLOUD_COLOR, FOG_COLOR, SKY_COLOR, SKY_LIGHT_COLOR, SKY_LIGHT_FACTOR,
    SKY_LIGHT_LEVEL, STAR_BRIGHTNESS, SUNRISE_SUNSET_COLOR,
};

use super::{EnvironmentAttributeLayer, EnvironmentAttributeSystemBuilder, WeatherLayer};

/// Vanilla `Timelines.NIGHT_SKY_LIGHT_COLOR`.
const NIGHT_SKY_LIGHT_COLOR: u32 = argb::color_from_float(1.0, 0.48, 0.48, 1.0);

const RAIN_ALPHA: f32 = 0.3125;
const THUNDER_ALPHA: f32 = 0.527_343_75;

// Entries own lazily built arguments, so they have destructors and cannot be
// `'static` temporaries; each is a named static instead.

static RAIN_SKY_COLOR: AttributeModifierEntry<Vec3, BlendToGrayModifier> =
    AttributeModifierEntry::new(
        BlendToGrayModifier,
        BlendToGray {
            brightness: 0.6,
            factor: 0.75,
        },
    );
static RAIN_FOG_COLOR: AttributeModifierEntry<Vec3, ColorMultiplyModifier> =
    AttributeModifierEntry::new(
        ColorMultiplyModifier,
        argb::vector3f_from_rgb24(argb::color_from_float(1.0, 0.5, 0.5, 0.6)),
    );
static RAIN_CLOUD_COLOR: AttributeModifierEntry<Vec4, BlendToGrayModifier> =
    AttributeModifierEntry::new(
        BlendToGrayModifier,
        BlendToGray {
            brightness: 0.24,
            factor: 0.5,
        },
    );
static RAIN_SKY_LIGHT_LEVEL: AttributeModifierEntry<f32, FloatAlphaBlendModifier> =
    AttributeModifierEntry::new(
        FloatAlphaBlendModifier,
        FloatWithAlpha {
            value: 4.0,
            alpha: RAIN_ALPHA,
        },
    );
static RAIN_SKY_LIGHT_COLOR: AttributeModifierEntry<Vec3, ColorAlphaBlendModifier> =
    AttributeModifierEntry::new(
        ColorAlphaBlendModifier,
        argb::vector4f_from_argb32(argb::color_with_alpha(RAIN_ALPHA, NIGHT_SKY_LIGHT_COLOR)),
    );
static RAIN_SKY_LIGHT_FACTOR: AttributeModifierEntry<f32, FloatAlphaBlendModifier> =
    AttributeModifierEntry::new(
        FloatAlphaBlendModifier,
        FloatWithAlpha {
            value: 0.24,
            alpha: RAIN_ALPHA,
        },
    );
static RAIN_SUNRISE_SUNSET_COLOR: AttributeModifierEntry<Vec4, ColorMultiplyModifier> =
    AttributeModifierEntry::new(
        ColorMultiplyModifier,
        argb::vector4f_from_argb32(argb::color_from_float(1.0, 0.5, 0.5, 0.6)),
    );

static THUNDER_SKY_COLOR: AttributeModifierEntry<Vec3, BlendToGrayModifier> =
    AttributeModifierEntry::new(
        BlendToGrayModifier,
        BlendToGray {
            brightness: 0.24,
            factor: 0.94,
        },
    );
static THUNDER_FOG_COLOR: AttributeModifierEntry<Vec3, ColorMultiplyModifier> =
    AttributeModifierEntry::new(
        ColorMultiplyModifier,
        argb::vector3f_from_rgb24(argb::color_from_float(1.0, 0.25, 0.25, 0.3)),
    );
static THUNDER_CLOUD_COLOR: AttributeModifierEntry<Vec4, BlendToGrayModifier> =
    AttributeModifierEntry::new(
        BlendToGrayModifier,
        BlendToGray {
            brightness: 0.095,
            factor: 0.94,
        },
    );
static THUNDER_SKY_LIGHT_LEVEL: AttributeModifierEntry<f32, FloatAlphaBlendModifier> =
    AttributeModifierEntry::new(
        FloatAlphaBlendModifier,
        FloatWithAlpha {
            value: 4.0,
            alpha: THUNDER_ALPHA,
        },
    );
static THUNDER_SKY_LIGHT_COLOR: AttributeModifierEntry<Vec3, ColorAlphaBlendModifier> =
    AttributeModifierEntry::new(
        ColorAlphaBlendModifier,
        argb::vector4f_from_argb32(argb::color_with_alpha(THUNDER_ALPHA, NIGHT_SKY_LIGHT_COLOR)),
    );
static THUNDER_SKY_LIGHT_FACTOR: AttributeModifierEntry<f32, FloatAlphaBlendModifier> =
    AttributeModifierEntry::new(
        FloatAlphaBlendModifier,
        FloatWithAlpha {
            value: 0.24,
            alpha: THUNDER_ALPHA,
        },
    );
static THUNDER_SUNRISE_SUNSET_COLOR: AttributeModifierEntry<Vec4, ColorMultiplyModifier> =
    AttributeModifierEntry::new(
        ColorMultiplyModifier,
        argb::vector4f_from_argb32(argb::color_from_float(1.0, 0.25, 0.25, 0.3)),
    );

/// Shared by both weather states.
static NO_STARS: AttributeModifierEntry<f32, OverrideModifier> =
    AttributeModifierEntry::new(OverrideModifier, 0.0);
/// Shared by both weather states.
static BEES_STAY: AttributeModifierEntry<bool, OverrideModifier> =
    AttributeModifierEntry::new(OverrideModifier, true);

pub(super) static RAIN: EnvironmentAttributeMap = EnvironmentAttributeMap::new(&[
    EnvironmentAttributeMapEntry::new(&SKY_COLOR, &RAIN_SKY_COLOR),
    EnvironmentAttributeMapEntry::new(&FOG_COLOR, &RAIN_FOG_COLOR),
    EnvironmentAttributeMapEntry::new(&CLOUD_COLOR, &RAIN_CLOUD_COLOR),
    EnvironmentAttributeMapEntry::new(&SKY_LIGHT_LEVEL, &RAIN_SKY_LIGHT_LEVEL),
    EnvironmentAttributeMapEntry::new(&SKY_LIGHT_COLOR, &RAIN_SKY_LIGHT_COLOR),
    EnvironmentAttributeMapEntry::new(&SKY_LIGHT_FACTOR, &RAIN_SKY_LIGHT_FACTOR),
    EnvironmentAttributeMapEntry::new(&STAR_BRIGHTNESS, &NO_STARS),
    EnvironmentAttributeMapEntry::new(&SUNRISE_SUNSET_COLOR, &RAIN_SUNRISE_SUNSET_COLOR),
    EnvironmentAttributeMapEntry::new(&BEES_STAY_IN_HIVE, &BEES_STAY),
]);

pub(super) static THUNDER: EnvironmentAttributeMap = EnvironmentAttributeMap::new(&[
    EnvironmentAttributeMapEntry::new(&SKY_COLOR, &THUNDER_SKY_COLOR),
    EnvironmentAttributeMapEntry::new(&FOG_COLOR, &THUNDER_FOG_COLOR),
    EnvironmentAttributeMapEntry::new(&CLOUD_COLOR, &THUNDER_CLOUD_COLOR),
    EnvironmentAttributeMapEntry::new(&SKY_LIGHT_LEVEL, &THUNDER_SKY_LIGHT_LEVEL),
    EnvironmentAttributeMapEntry::new(&SKY_LIGHT_COLOR, &THUNDER_SKY_LIGHT_COLOR),
    EnvironmentAttributeMapEntry::new(&SKY_LIGHT_FACTOR, &THUNDER_SKY_LIGHT_FACTOR),
    EnvironmentAttributeMapEntry::new(&STAR_BRIGHTNESS, &NO_STARS),
    EnvironmentAttributeMapEntry::new(&SUNRISE_SUNSET_COLOR, &THUNDER_SUNRISE_SUNSET_COLOR),
    EnvironmentAttributeMapEntry::new(&BEES_STAY_IN_HIVE, &BEES_STAY),
]);

/// Adds a weather layer for every attribute `RAIN` or `THUNDER` modifies.
///
/// Vanilla iterates the union of both maps' keys; the weather layer needs each
/// attribute's typed lerp, so Steel lists them explicitly.
pub(super) fn add_builtin_layers(
    builder: EnvironmentAttributeSystemBuilder,
) -> EnvironmentAttributeSystemBuilder {
    let builder = add_layer(builder, &SKY_COLOR);
    let builder = add_layer(builder, &FOG_COLOR);
    let builder = add_layer(builder, &CLOUD_COLOR);
    let builder = add_layer(builder, &SKY_LIGHT_LEVEL);
    let builder = add_layer(builder, &SKY_LIGHT_COLOR);
    let builder = add_layer(builder, &SKY_LIGHT_FACTOR);
    let builder = add_layer(builder, &STAR_BRIGHTNESS);
    let builder = add_layer(builder, &SUNRISE_SUNSET_COLOR);
    add_layer(builder, &BEES_STAY_IN_HIVE)
}

fn add_layer<V: AttributeValue>(
    builder: EnvironmentAttributeSystemBuilder,
    attribute: &'static EnvironmentAttribute<V>,
) -> EnvironmentAttributeSystemBuilder {
    let layer = WeatherLayer::new(attribute, RAIN.get(attribute), THUNDER.get(attribute));
    builder.add_layer(
        attribute,
        EnvironmentAttributeLayer::TimeBased(Box::new(layer)),
    )
}

#[cfg(test)]
mod tests {
    use super::{RAIN, THUNDER, add_builtin_layers};
    use crate::world::environment_attributes::EnvironmentAttributeSystem;

    #[test]
    fn builtin_layers_cover_every_weather_attribute() {
        let system = add_builtin_layers(EnvironmentAttributeSystem::builder()).build();
        for attribute in RAIN.attributes().chain(THUNDER.attributes()) {
            assert!(
                system.has_layers(attribute),
                "{} has no weather layer",
                attribute.key()
            );
        }
    }
}
