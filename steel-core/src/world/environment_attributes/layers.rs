//! Layers vanilla registers for every server level.

use glam::DVec3;
use steel_registry::environment_attribute::{
    AttributeValue, EnvironmentAttribute, ErasedAttributeModifierEntry,
    ErasedEnvironmentAttributeRef,
};
use steel_registry::timeline::{ErasedAttributeTrack, TimelineRef};
use steel_utils::{BlockPos, Downcast as _, ErasedType};

use super::{ConstantLayer, EnvironmentSampleContext, PositionalLayer, TimeBasedLayer};

/// Applies one entry of the dimension type's attribute map.
pub struct DimensionLayer {
    modifier: &'static dyn ErasedAttributeModifierEntry,
}

impl DimensionLayer {
    /// Wraps one dimension attribute map entry.
    #[must_use]
    pub const fn new(modifier: &'static dyn ErasedAttributeModifierEntry) -> Self {
        Self { modifier }
    }
}

impl ConstantLayer for DimensionLayer {
    fn apply_constant(&self, value: &mut dyn ErasedType) {
        self.modifier.apply_erased(value);
    }
}

/// Applies the attribute map of the biome at the sampled position.
pub struct BiomeLayer {
    attribute: ErasedEnvironmentAttributeRef,
}

impl BiomeLayer {
    /// Creates the biome layer of `attribute`.
    #[must_use]
    pub const fn new(attribute: ErasedEnvironmentAttributeRef) -> Self {
        Self { attribute }
    }
}

impl PositionalLayer for BiomeLayer {
    fn apply_positional(
        &self,
        value: &mut dyn ErasedType,
        pos: DVec3,
        context: &EnvironmentSampleContext<'_>,
    ) {
        let block_pos = BlockPos::containing(pos.x, pos.y, pos.z);
        let biome = if self.attribute.flags().full_resolution_biomes {
            context.biomes.biome(block_pos)
        } else {
            context
                .biomes
                .noise_biome(block_pos.x() >> 2, block_pos.y() >> 2, block_pos.z() >> 2)
        };
        let Some(modifier) = biome.and_then(|biome| biome.attributes.get(self.attribute)) else {
            return;
        };
        modifier.apply_erased(value);
    }
}

/// Applies a timeline track sampled at the timeline's clock, as vanilla's
/// `AttributeTrackSampler`.
pub struct TimelineTrackLayer {
    timeline: TimelineRef,
    track: &'static dyn ErasedAttributeTrack,
}

impl TimelineTrackLayer {
    /// Samples `track` on `timeline`'s clock.
    #[must_use]
    pub const fn new(timeline: TimelineRef, track: &'static dyn ErasedAttributeTrack) -> Self {
        Self { timeline, track }
    }
}

impl TimeBasedLayer for TimelineTrackLayer {
    fn apply_time_based(&self, value: &mut dyn ErasedType, context: &EnvironmentSampleContext<'_>) {
        let Some(total_ticks) = context.clock_total_ticks(self.timeline.clock) else {
            return;
        };
        self.track
            .apply_erased(value, self.timeline.period_ticks, total_ticks);
    }
}

/// Blends rain and thunder modifiers in by the current weather levels, as
/// the layer vanilla's `WeatherAttributes.addBuiltinLayers` registers.
pub struct WeatherLayer<V: AttributeValue> {
    attribute: &'static EnvironmentAttribute<V>,
    rain: Option<&'static dyn ErasedAttributeModifierEntry>,
    thunder: Option<&'static dyn ErasedAttributeModifierEntry>,
}

impl<V: AttributeValue> WeatherLayer<V> {
    /// Blends `attribute` toward its rain and thunder entries.
    #[must_use]
    pub const fn new(
        attribute: &'static EnvironmentAttribute<V>,
        rain: Option<&'static dyn ErasedAttributeModifierEntry>,
        thunder: Option<&'static dyn ErasedAttributeModifierEntry>,
    ) -> Self {
        Self {
            attribute,
            rain,
            thunder,
        }
    }

    fn apply(&self, mut result: V, context: &EnvironmentSampleContext<'_>) -> V {
        let lerp = self.attribute.attribute_type().state_change_lerp();
        let thunder_level = context.thunder_level;
        let rain_level = context.rain_level - thunder_level;
        if let Some(rain) = self.rain
            && rain_level > 0.0
        {
            let rain_value = rain.apply_typed(result.clone());
            result = lerp(rain_level, &result, &rain_value);
        }
        if let Some(thunder) = self.thunder
            && thunder_level > 0.0
        {
            let thunder_value = thunder.apply_typed(result.clone());
            result = lerp(thunder_level, &result, &thunder_value);
        }
        result
    }
}

impl<V: AttributeValue> TimeBasedLayer for WeatherLayer<V> {
    fn apply_time_based(&self, value: &mut dyn ErasedType, context: &EnvironmentSampleContext<'_>) {
        let Some(value) = value.downcast_mut::<V>() else {
            return;
        };
        *value = self.apply(value.clone(), context);
    }
}
