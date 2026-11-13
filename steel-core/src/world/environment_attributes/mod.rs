//! Vanilla `EnvironmentAttributeSystem`: layered sampling of environment
//! attributes for one world.
//!
//! Each attribute's value starts from its default and passes through its
//! layers in registration order: the dimension's constant layer, biome
//! positional layers, timeline tracks, then weather.
//!
//! Differences from vanilla:
//! - Layers read world state from an [`EnvironmentSampleContext`] snapshot
//!   passed per read instead of capturing the level.
//! - Samplers of every value type share one map, so each read downcasts its
//!   attribute's [`ValueSampler`] once (see `steel_registry`'s
//!   `environment_attribute::sampler`).
//! - Leading constant layers are not baked into a base value and nothing is
//!   cached per tick; every read recomputes from the layers, so it always
//!   sees the current clocks and weather.
//!
//!   Vanilla caches each sampled value until `invalidateTickCache`, which runs
//!   at the start of `ServerLevel.tick` and on every `ServerClockManager`
//!   clock change. A read therefore sees the state at the first read of the
//!   tick. The results match only because nothing a layer depends on changes
//!   between the first attribute read of a tick and the end of it:
//!   - weather levels change only in `World::tick_weather`, which runs before
//!     any read of the tick, as `advanceWeatherCycle` does in vanilla;
//!   - clocks change only in `World::tick_time`, which also runs before any
//!     read, or through clock commands and time markers, after which vanilla
//!     invalidates too.
//!
//!   Code that adds a read before `tick_weather`/`tick_time` in
//!   `World::tick_game`, or that changes rain or thunder levels mid-tick,
//!   breaks this equivalence.
//! - Spatial biome interpolation (`SpatialAttributeInterpolator`) is
//!   client-side and not implemented; the server never passes one.

mod weather_attributes;

#[cfg(test)]
mod tests;

use glam::DVec3;
use rustc_hash::FxHashMap;
use steel_math::keyframe_track::lerp_float;
use steel_registry::biome::BiomeRef;
use steel_registry::dimension_type::DimensionTypeRef;
use steel_registry::environment_attribute::{
    AttributeValue, BiomeSource, ClockSource, EnvironmentAttribute, EnvironmentAttributeLayer,
    EnvironmentAttributeMap, EnvironmentSampleContext, ErasedEnvironmentAttribute,
    ErasedEnvironmentAttributeRef, ErasedValueSampler, LayerValueTypeMismatch, ValueSampler,
};
use steel_registry::world_clock::WorldClockRef;
use steel_registry::{REGISTRY, RegistryExt as _};
use steel_utils::{BlockPos, Downcast as _, ErasedType, Identifier};

use super::World;
use super::clock::WorldClockManager;

pub use weather_attributes::WeatherLayer;

/// Samples every layered environment attribute of a world.
pub struct EnvironmentAttributeSystem {
    samplers: FxHashMap<Identifier, Box<dyn ErasedValueSampler>>,
}

impl EnvironmentAttributeSystem {
    /// Starts a system with no layers.
    #[must_use]
    pub fn builder() -> EnvironmentAttributeSystemBuilder {
        EnvironmentAttributeSystemBuilder::default()
    }

    /// Builds the layers vanilla gives a server level of `dimension_type`.
    #[must_use]
    pub(super) fn for_level(dimension_type: DimensionTypeRef) -> Self {
        Self::builder()
            .add_default_layers(
                dimension_type,
                World::dimension_can_have_weather(dimension_type),
            )
            .build()
    }

    /// Returns whether any layer modifies `attribute`.
    #[must_use]
    pub fn has_layers(&self, attribute: &(impl ErasedEnvironmentAttribute + ?Sized)) -> bool {
        self.samplers.contains_key(attribute.key())
    }

    /// Samples `attribute` ignoring positional layers.
    ///
    /// Vanilla rejects positional attributes here in development builds;
    /// Steel asserts the same in debug builds.
    #[must_use]
    pub fn get_dimension_value<V: AttributeValue>(
        &self,
        attribute: &EnvironmentAttribute<V>,
        context: &EnvironmentSampleContext<'_>,
    ) -> V {
        debug_assert!(
            !attribute.is_positional(),
            "Position must always be provided for positional attribute {}",
            attribute.key()
        );
        self.sample(attribute, None, context)
    }

    /// Samples `attribute` at `pos`.
    #[must_use]
    pub fn get_value<V: AttributeValue>(
        &self,
        attribute: &EnvironmentAttribute<V>,
        pos: DVec3,
        context: &EnvironmentSampleContext<'_>,
    ) -> V {
        self.sample(attribute, Some(pos), context)
    }

    fn sample<V: AttributeValue>(
        &self,
        attribute: &EnvironmentAttribute<V>,
        pos: Option<DVec3>,
        context: &EnvironmentSampleContext<'_>,
    ) -> V {
        let value = attribute.default_value().clone();
        let Some(sampler) = self.samplers.get(attribute.key()) else {
            return value;
        };
        let sampler: &dyn ErasedType = sampler.as_ref();
        let Some(sampler) = sampler.downcast_ref::<ValueSampler<V>>() else {
            // The builder keeps one value type per key, so only an
            // unregistered attribute reusing a registered key gets here.
            debug_assert!(
                false,
                "attribute {} is read with another value type than its layers",
                attribute.key()
            );
            return value;
        };
        attribute.sanitize_value(sampler.sample(value, pos, context))
    }
}

/// Collects layers per attribute, as vanilla's `EnvironmentAttributeSystem.Builder`.
///
/// Attribute keys are unique, so every layer added for one key must share its
/// value type. Adding a layer of another value type panics, like registering a
/// duplicate registry key.
#[derive(Default)]
pub struct EnvironmentAttributeSystemBuilder {
    samplers: FxHashMap<Identifier, Box<dyn ErasedValueSampler>>,
}

impl EnvironmentAttributeSystemBuilder {
    /// Adds the static and dynamic layers vanilla gives a server level.
    #[must_use]
    pub fn add_default_layers(
        self,
        dimension_type: DimensionTypeRef,
        can_have_weather: bool,
    ) -> Self {
        self.add_static_layers(dimension_type)
            .add_dynamic_layers(dimension_type, can_have_weather)
    }

    /// Adds the dimension's constant layer and a biome layer for every
    /// attribute any biome provides.
    #[must_use]
    pub fn add_static_layers(mut self, dimension_type: DimensionTypeRef) -> Self {
        self = self.add_constant_layer_map(dimension_type.attributes);

        let mut biome_attributes: Vec<ErasedEnvironmentAttributeRef> = Vec::new();
        for (_, biome) in REGISTRY.biomes.iter() {
            for attribute in biome.attributes.attributes() {
                if !biome_attributes
                    .iter()
                    .any(|known| known.key() == attribute.key())
                {
                    biome_attributes.push(attribute);
                }
            }
        }
        for attribute in biome_attributes {
            self.sampler_mut(attribute).add_biome_layer();
        }
        self
    }

    /// Adds the dimension's timeline tracks and, when the dimension has
    /// weather, the built-in weather layers.
    #[must_use]
    pub fn add_dynamic_layers(
        mut self,
        dimension_type: DimensionTypeRef,
        can_have_weather: bool,
    ) -> Self {
        for timeline in dimension_type.timelines() {
            for &track in timeline.attribute_tracks {
                let added = self
                    .sampler_mut(track.attribute())
                    .add_timeline_track_layer(timeline, track);
                Self::expect_value_type(added);
            }
        }
        if can_have_weather {
            self = weather_attributes::add_builtin_layers(self);
        }
        self
    }

    /// Adds one constant layer per entry of `map`.
    #[must_use]
    pub fn add_constant_layer_map(mut self, map: EnvironmentAttributeMap) -> Self {
        for entry in map.entries() {
            let added = self
                .sampler_mut(entry.attribute())
                .add_dimension_layer(entry.modifier());
            Self::expect_value_type(added);
        }
        self
    }

    /// Appends `layer` to `attribute`'s sampling chain.
    ///
    /// # Panics
    ///
    /// Panics if a layer of another value type was added under the same
    /// attribute key.
    #[must_use]
    pub fn add_layer<V: AttributeValue>(
        mut self,
        attribute: &'static EnvironmentAttribute<V>,
        layer: EnvironmentAttributeLayer<V>,
    ) -> Self {
        let sampler = self.sampler_mut(attribute);
        let found = sampler.attribute().value_type_key();
        let sampler: &mut dyn ErasedType = sampler;
        let Some(sampler) = sampler.downcast_mut::<ValueSampler<V>>() else {
            Self::expect_value_type(Err(LayerValueTypeMismatch {
                attribute: attribute.key().clone(),
                expected: found,
                found: attribute.value_type_key(),
            }));
            return self;
        };
        sampler.add_layer(layer);
        self
    }

    /// Finishes the system.
    #[must_use]
    pub fn build(self) -> EnvironmentAttributeSystem {
        EnvironmentAttributeSystem {
            samplers: self.samplers,
        }
    }

    /// Returns `attribute`'s sampler, creating it with the attribute's value
    /// type on first use.
    fn sampler_mut(
        &mut self,
        attribute: ErasedEnvironmentAttributeRef,
    ) -> &mut dyn ErasedValueSampler {
        self.samplers
            .entry(attribute.key().clone())
            .or_insert_with(|| attribute.new_value_sampler())
            .as_mut()
    }

    /// Panics on a layer whose value type differs from its attribute key's
    /// earlier layers; see the type docs.
    #[expect(
        clippy::panic,
        reason = "two value types under one attribute key is a fatal setup error, like a duplicate registry key"
    )]
    fn expect_value_type(added: Result<(), LayerValueTypeMismatch>) {
        if let Err(mismatch) = added {
            panic!("{mismatch}");
        }
    }
}

/// Reads a world's environment attributes, as vanilla's
/// `level.environmentAttributes()`.
///
/// Each read takes the world's `level_data` read lock and `weather` lock;
/// calling it while holding either deadlocks.
pub struct EnvironmentAttributeReader<'a> {
    world: &'a World,
}

impl EnvironmentAttributeReader<'_> {
    /// Samples a non-positional `attribute`.
    #[must_use]
    pub fn get_dimension_value<V: AttributeValue>(&self, attribute: &EnvironmentAttribute<V>) -> V {
        let system = &self.world.environment_attributes;
        if !system.has_layers(attribute) {
            return attribute.default_value().clone();
        }
        self.with_context(|context| system.get_dimension_value(attribute, context))
    }

    /// Samples `attribute` at the center of `pos`.
    #[must_use]
    pub fn get_value<V: AttributeValue>(
        &self,
        attribute: &EnvironmentAttribute<V>,
        pos: BlockPos,
    ) -> V {
        self.get_value_at(attribute, DVec3::from(pos.get_center()))
    }

    /// Samples `attribute` at `pos`.
    #[must_use]
    pub fn get_value_at<V: AttributeValue>(
        &self,
        attribute: &EnvironmentAttribute<V>,
        pos: DVec3,
    ) -> V {
        let system = &self.world.environment_attributes;
        if !system.has_layers(attribute) {
            return attribute.default_value().clone();
        }
        self.with_context(|context| system.get_value(attribute, pos, context))
    }

    fn with_context<R>(&self, sample: impl FnOnce(&EnvironmentSampleContext<'_>) -> R) -> R {
        let (rain_level, thunder_level) = {
            let weather = self.world.weather.lock();
            let rain_level = lerp_float(1.0, &weather.previous_rain_level, &weather.rain_level);
            let thunder_level =
                lerp_float(1.0, &weather.previous_thunder_level, &weather.thunder_level)
                    * rain_level;
            (rain_level, thunder_level)
        };
        let level_data = self.world.level_data.read();
        sample(&EnvironmentSampleContext {
            clocks: level_data.world_clocks(),
            rain_level,
            thunder_level,
            biomes: self.world,
        })
    }
}

impl World {
    /// Returns read access to this world's environment attributes.
    #[must_use]
    pub const fn environment_attributes(&self) -> EnvironmentAttributeReader<'_> {
        EnvironmentAttributeReader { world: self }
    }
}

impl ClockSource for WorldClockManager {
    fn total_ticks(&self, clock: WorldClockRef) -> Option<i64> {
        Self::total_ticks(self, clock)
    }
}

impl BiomeSource for World {
    fn noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> Option<BiomeRef> {
        let biome_id = self.noise_biome_id(quart_x, quart_y, quart_z)?;
        REGISTRY.biomes.by_id(usize::from(biome_id))
    }

    fn biome(&self, pos: BlockPos) -> Option<BiomeRef> {
        self.biome_at(pos)
    }
}
