//! Vanilla `EnvironmentAttributeLayer`: one step of an attribute's sampling
//! chain, and the layers built from registry data.
//!
//! Differences from vanilla: layers read world state from an
//! [`EnvironmentSampleContext`] passed per read instead of capturing the level,
//! so the registry stays independent of the world implementation.

use glam::DVec3;
use steel_utils::BlockPos;

use super::{AttributeValue, EnvironmentAttribute, TypedAttributeModifierEntry};
use crate::biome::BiomeRef;
use crate::timeline::{TimelineRef, TypedAttributeTrack};
use crate::world_clock::WorldClockRef;

/// Looks up the biomes positional layers sample.
pub trait BiomeSource {
    /// Biome stored for the given quart (4x4x4 block cell), as vanilla's
    /// `BiomeManager.getNoiseBiomeAtQuart`.
    fn noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> Option<BiomeRef>;

    /// Zoomed (fuzzed) biome at a block, as vanilla's `BiomeManager.getBiome`.
    fn biome(&self, pos: BlockPos) -> Option<BiomeRef>;
}

/// Looks up the world clocks time-based layers sample.
pub trait ClockSource {
    /// Total ticks elapsed on `clock`, if the world tracks it.
    fn total_ticks(&self, clock: WorldClockRef) -> Option<i64>;
}

/// World state layers read while sampling.
pub struct EnvironmentSampleContext<'a> {
    /// Clock lookup for timeline layers.
    pub clocks: &'a dyn ClockSource,
    /// Vanilla `Level.getRainLevel(1.0F)`.
    pub rain_level: f32,
    /// Vanilla `Level.getThunderLevel(1.0F)`, already scaled by the rain level.
    pub thunder_level: f32,
    /// Biome lookup for positional layers.
    pub biomes: &'a dyn BiomeSource,
}

/// A layer whose result only depends on its input value.
pub trait ConstantLayer<V: AttributeValue>: Send + Sync {
    fn apply_constant(&self, value: V) -> V;
}

/// A layer that varies with clocks or weather.
pub trait TimeBasedLayer<V: AttributeValue>: Send + Sync {
    fn apply_time_based(&self, value: V, context: &EnvironmentSampleContext<'_>) -> V;
}

/// A layer that varies with position.
pub trait PositionalLayer<V: AttributeValue>: Send + Sync {
    fn apply_positional(&self, value: V, pos: DVec3, context: &EnvironmentSampleContext<'_>) -> V;
}

/// One step of an attribute's sampling chain, as vanilla's
/// `EnvironmentAttributeLayer`.
pub enum EnvironmentAttributeLayer<V: AttributeValue> {
    /// Depends only on its input value.
    Constant(Box<dyn ConstantLayer<V>>),
    /// Varies with clocks or weather.
    TimeBased(Box<dyn TimeBasedLayer<V>>),
    /// Varies with the sampled position; skipped by dimension reads.
    Positional(Box<dyn PositionalLayer<V>>),
}

/// Applies one entry of the dimension type's attribute map.
pub struct DimensionLayer<V: AttributeValue> {
    modifier: &'static dyn TypedAttributeModifierEntry<V>,
}

impl<V: AttributeValue> DimensionLayer<V> {
    /// Wraps one dimension attribute map entry.
    #[must_use]
    pub const fn new(modifier: &'static dyn TypedAttributeModifierEntry<V>) -> Self {
        Self { modifier }
    }
}

impl<V: AttributeValue> ConstantLayer<V> for DimensionLayer<V> {
    fn apply_constant(&self, value: V) -> V {
        self.modifier.apply(value)
    }
}

/// Applies the attribute map of the biome at the sampled position.
pub struct BiomeLayer<V: AttributeValue> {
    attribute: &'static EnvironmentAttribute<V>,
}

impl<V: AttributeValue> BiomeLayer<V> {
    /// Creates the biome layer of `attribute`.
    #[must_use]
    pub const fn new(attribute: &'static EnvironmentAttribute<V>) -> Self {
        Self { attribute }
    }
}

impl<V: AttributeValue> PositionalLayer<V> for BiomeLayer<V> {
    fn apply_positional(&self, value: V, pos: DVec3, context: &EnvironmentSampleContext<'_>) -> V {
        let block_pos = BlockPos::containing(pos.x, pos.y, pos.z);
        let biome = if self.attribute.flags().full_resolution_biomes {
            context.biomes.biome(block_pos)
        } else {
            context
                .biomes
                .noise_biome(block_pos.x() >> 2, block_pos.y() >> 2, block_pos.z() >> 2)
        };
        let Some(biome) = biome else {
            return value;
        };
        biome.attributes.apply_modifier(self.attribute, value)
    }
}

/// Applies a timeline track sampled at the timeline's clock, as vanilla's
/// `AttributeTrackSampler`.
pub struct TimelineTrackLayer<V: AttributeValue> {
    timeline: TimelineRef,
    track: &'static dyn TypedAttributeTrack<V>,
}

impl<V: AttributeValue> TimelineTrackLayer<V> {
    /// Samples `track` on `timeline`'s clock.
    #[must_use]
    pub const fn new(timeline: TimelineRef, track: &'static dyn TypedAttributeTrack<V>) -> Self {
        Self { timeline, track }
    }
}

impl<V: AttributeValue> TimeBasedLayer<V> for TimelineTrackLayer<V> {
    fn apply_time_based(&self, value: V, context: &EnvironmentSampleContext<'_>) -> V {
        let Some(total_ticks) = context.clocks.total_ticks(self.timeline.clock) else {
            return value;
        };
        self.track
            .apply(value, self.timeline.period_ticks, total_ticks)
    }
}
