//! Vanilla `EnvironmentAttributeSystem.ValueSampler`: the typed layer chain of
//! one attribute.
//!
//! Differences from vanilla: Steel stores samplers of every value type in one
//! map, so each sampler sits behind [`ErasedValueSampler`] and is downcast to
//! its value type once per read. Layers built from registry data, whose value
//! type is only known through an erased attribute, are added through the
//! erased sampler, which recovers their typed entries at build time.

use std::fmt::{self, Display, Formatter};

use glam::DVec3;
use steel_utils::{DowncastType, DowncastTypeKey, ErasedType, Identifier};

use super::{
    AttributeValue, BiomeLayer, DimensionLayer, EnvironmentAttribute, EnvironmentAttributeLayer,
    EnvironmentSampleContext, ErasedAttributeModifierEntry, ErasedEnvironmentAttribute,
    TimelineTrackLayer,
};
use crate::timeline::{ErasedAttributeTrack, TimelineRef};

/// The layers of one attribute, applied in registration order.
pub struct ValueSampler<V: AttributeValue> {
    attribute: &'static EnvironmentAttribute<V>,
    layers: Vec<EnvironmentAttributeLayer<V>>,
    is_affected_by_position: bool,
}

impl<V: AttributeValue> ValueSampler<V> {
    /// Creates a sampler of `attribute` with no layers.
    #[must_use]
    pub const fn new(attribute: &'static EnvironmentAttribute<V>) -> Self {
        Self {
            attribute,
            layers: Vec::new(),
            is_affected_by_position: false,
        }
    }

    /// Appends `layer` to the chain.
    pub fn add_layer(&mut self, layer: EnvironmentAttributeLayer<V>) {
        self.is_affected_by_position |= matches!(layer, EnvironmentAttributeLayer::Positional(_));
        self.layers.push(layer);
    }

    /// Whether any layer varies with position.
    #[must_use]
    pub const fn is_affected_by_position(&self) -> bool {
        self.is_affected_by_position
    }

    /// Runs every layer over `value`; positional layers only run with a `pos`.
    #[must_use]
    pub fn sample(
        &self,
        value: V,
        pos: Option<DVec3>,
        context: &EnvironmentSampleContext<'_>,
    ) -> V {
        let pos = pos.filter(|_| self.is_affected_by_position);
        self.layers
            .iter()
            .fold(value, |value, layer| match (layer, pos) {
                (EnvironmentAttributeLayer::Constant(layer), _) => layer.apply_constant(value),
                (EnvironmentAttributeLayer::TimeBased(layer), _) => {
                    layer.apply_time_based(value, context)
                }
                (EnvironmentAttributeLayer::Positional(layer), Some(pos)) => {
                    layer.apply_positional(value, pos, context)
                }
                (EnvironmentAttributeLayer::Positional(_), None) => value,
            })
    }
}

// SAFETY: The key includes `V`'s key, so every instantiation of this
// Steel-owned type has its own key.
unsafe impl<V: AttributeValue> DowncastType for ValueSampler<V> {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::generic("steel:environment_attribute/value_sampler", &[V::TYPE_KEY]);
}

/// A data layer whose value type differs from the sampler it was added to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerValueTypeMismatch {
    /// Attribute the sampler belongs to.
    pub attribute: Identifier,
    /// Value type of the sampler.
    pub expected: DowncastTypeKey,
    /// Value type of the rejected layer.
    pub found: DowncastTypeKey,
}

impl Display for LayerValueTypeMismatch {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "layer of value type {} cannot be added to attribute {} of value type {}",
            self.found, self.attribute, self.expected
        )
    }
}

impl std::error::Error for LayerValueTypeMismatch {}

/// A [`ValueSampler`] of any value type.
///
/// Recover the typed sampler with [`Downcast`](steel_utils::Downcast) after
/// upcasting to [`ErasedType`].
pub trait ErasedValueSampler: ErasedType + Send + Sync {
    /// The attribute this sampler belongs to.
    fn attribute(&self) -> &'static dyn ErasedEnvironmentAttribute;

    /// Appends a constant layer applying a dimension map entry.
    ///
    /// # Errors
    ///
    /// Returns an error when the entry's value type is not the sampler's.
    fn add_dimension_layer(
        &mut self,
        modifier: &'static dyn ErasedAttributeModifierEntry,
    ) -> Result<(), LayerValueTypeMismatch>;

    /// Appends a positional layer applying the sampled biome's map.
    fn add_biome_layer(&mut self);

    /// Appends a time-based layer applying a timeline track.
    ///
    /// # Errors
    ///
    /// Returns an error when the track's value type is not the sampler's.
    fn add_timeline_track_layer(
        &mut self,
        timeline: TimelineRef,
        track: &'static dyn ErasedAttributeTrack,
    ) -> Result<(), LayerValueTypeMismatch>;
}

impl<V: AttributeValue> ValueSampler<V> {
    fn mismatch(&self, found: DowncastTypeKey) -> LayerValueTypeMismatch {
        LayerValueTypeMismatch {
            attribute: self.attribute.key().clone(),
            expected: V::TYPE_KEY,
            found,
        }
    }
}

impl<V: AttributeValue> ErasedValueSampler for ValueSampler<V> {
    fn attribute(&self) -> &'static dyn ErasedEnvironmentAttribute {
        self.attribute
    }

    fn add_dimension_layer(
        &mut self,
        modifier: &'static dyn ErasedAttributeModifierEntry,
    ) -> Result<(), LayerValueTypeMismatch> {
        let Some(modifier) = modifier.downcast::<V>() else {
            return Err(self.mismatch(modifier.value_type_key()));
        };
        self.add_layer(EnvironmentAttributeLayer::Constant(Box::new(
            DimensionLayer::new(modifier),
        )));
        Ok(())
    }

    fn add_biome_layer(&mut self) {
        self.add_layer(EnvironmentAttributeLayer::Positional(Box::new(
            BiomeLayer::new(self.attribute),
        )));
    }

    fn add_timeline_track_layer(
        &mut self,
        timeline: TimelineRef,
        track: &'static dyn ErasedAttributeTrack,
    ) -> Result<(), LayerValueTypeMismatch> {
        let Some(typed_track) = track.downcast::<V>() else {
            return Err(self.mismatch(track.attribute().value_type_key()));
        };
        self.add_layer(EnvironmentAttributeLayer::TimeBased(Box::new(
            TimelineTrackLayer::new(timeline, typed_track),
        )));
        Ok(())
    }
}
