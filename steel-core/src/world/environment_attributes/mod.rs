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

mod layers;
mod weather_attributes;

#[cfg(test)]
mod tests;

use glam::DVec3;
use rustc_hash::FxHashMap;
use steel_math::keyframe_track::lerp_float;
use steel_registry::biome::BiomeRef;
use steel_registry::dimension_type::DimensionTypeRef;
use steel_registry::environment_attribute::{
    AttributeValue, EnvironmentAttribute, EnvironmentAttributeMap, ErasedEnvironmentAttribute,
    ErasedEnvironmentAttributeRef,
};
use steel_registry::world_clock::WorldClockRef;
use steel_registry::{REGISTRY, RegistryExt as _};
use steel_utils::{BlockPos, ErasedType, Identifier};

use super::World;
use super::clock::WorldClockManager;

pub use layers::{BiomeLayer, DimensionLayer, TimelineTrackLayer, WeatherLayer};

/// Looks up the biomes a world's positional layers sample.
pub trait BiomeSource {
    /// Biome stored for the given quart (4x4x4 block cell), as vanilla's
    /// `BiomeManager.getNoiseBiomeAtQuart`.
    fn noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> Option<BiomeRef>;

    /// Zoomed (fuzzed) biome at a block, as vanilla's `BiomeManager.getBiome`.
    fn biome(&self, pos: BlockPos) -> Option<BiomeRef>;
}

/// World state layers read while sampling.
pub struct EnvironmentSampleContext<'a> {
    pub(crate) clocks: &'a WorldClockManager,
    /// Vanilla `Level.getRainLevel(1.0F)`.
    pub rain_level: f32,
    /// Vanilla `Level.getThunderLevel(1.0F)`, already scaled by the rain level.
    pub thunder_level: f32,
    /// Biome lookup for positional layers.
    pub biomes: &'a dyn BiomeSource,
}

impl EnvironmentSampleContext<'_> {
    /// Total ticks elapsed on `clock`, if the world tracks it.
    #[must_use]
    pub fn clock_total_ticks(&self, clock: WorldClockRef) -> Option<i64> {
        self.clocks.total_ticks(clock)
    }
}

/// A layer whose result only depends on its input value.
pub trait ConstantLayer: Send + Sync {
    /// Modifies `value` in place; ignores values of another type.
    fn apply_constant(&self, value: &mut dyn ErasedType);
}

/// A layer that varies with clocks or weather.
pub trait TimeBasedLayer: Send + Sync {
    /// Modifies `value` in place; ignores values of another type.
    fn apply_time_based(&self, value: &mut dyn ErasedType, context: &EnvironmentSampleContext<'_>);
}

/// A layer that varies with position.
pub trait PositionalLayer: Send + Sync {
    /// Modifies `value` in place; ignores values of another type.
    fn apply_positional(
        &self,
        value: &mut dyn ErasedType,
        pos: DVec3,
        context: &EnvironmentSampleContext<'_>,
    );
}

/// One step of an attribute's sampling chain, as vanilla's
/// `EnvironmentAttributeLayer`.
pub enum EnvironmentAttributeLayer {
    /// Depends only on its input value.
    Constant(Box<dyn ConstantLayer>),
    /// Varies with clocks or weather.
    TimeBased(Box<dyn TimeBasedLayer>),
    /// Varies with the sampled position; skipped by dimension reads.
    Positional(Box<dyn PositionalLayer>),
}

struct ValueSampler {
    layers: Vec<EnvironmentAttributeLayer>,
    is_affected_by_position: bool,
}

impl ValueSampler {
    fn new(layers: Vec<EnvironmentAttributeLayer>) -> Self {
        let is_affected_by_position = layers
            .iter()
            .any(|layer| matches!(layer, EnvironmentAttributeLayer::Positional(_)));
        Self {
            layers,
            is_affected_by_position,
        }
    }

    /// Runs every layer over `value`; positional layers only run with a `pos`.
    fn sample(
        &self,
        value: &mut dyn ErasedType,
        pos: Option<DVec3>,
        context: &EnvironmentSampleContext<'_>,
    ) {
        for layer in &self.layers {
            match layer {
                EnvironmentAttributeLayer::Constant(layer) => layer.apply_constant(value),
                EnvironmentAttributeLayer::TimeBased(layer) => {
                    layer.apply_time_based(value, context);
                }
                EnvironmentAttributeLayer::Positional(layer) => {
                    if let Some(pos) = pos {
                        layer.apply_positional(value, pos, context);
                    }
                }
            }
        }
    }
}

/// Samples every layered environment attribute of a world.
pub struct EnvironmentAttributeSystem {
    samplers: FxHashMap<Identifier, ValueSampler>,
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
        let mut value = attribute.default_value().clone();
        let Some(sampler) = self.samplers.get(attribute.key()) else {
            return value;
        };
        let pos = pos.filter(|_| sampler.is_affected_by_position);
        sampler.sample(&mut value, pos, context);
        attribute.sanitize_value(value)
    }
}

/// Collects layers per attribute, as vanilla's `EnvironmentAttributeSystem.Builder`.
#[derive(Default)]
pub struct EnvironmentAttributeSystemBuilder {
    layers_by_attribute: FxHashMap<Identifier, Vec<EnvironmentAttributeLayer>>,
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
            self = self.add_layer(
                attribute,
                EnvironmentAttributeLayer::Positional(Box::new(BiomeLayer::new(attribute))),
            );
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
                self = self.add_layer(
                    track.attribute(),
                    EnvironmentAttributeLayer::TimeBased(Box::new(TimelineTrackLayer::new(
                        timeline, track,
                    ))),
                );
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
            self = self.add_layer(
                entry.attribute(),
                EnvironmentAttributeLayer::Constant(Box::new(DimensionLayer::new(
                    entry.modifier(),
                ))),
            );
        }
        self
    }

    /// Appends `layer` to `attribute`'s sampling chain.
    #[must_use]
    pub fn add_layer(
        mut self,
        attribute: ErasedEnvironmentAttributeRef,
        layer: EnvironmentAttributeLayer,
    ) -> Self {
        self.layers_by_attribute
            .entry(attribute.key().clone())
            .or_default()
            .push(layer);
        self
    }

    /// Finishes the system.
    #[must_use]
    pub fn build(self) -> EnvironmentAttributeSystem {
        EnvironmentAttributeSystem {
            samplers: self
                .layers_by_attribute
                .into_iter()
                .map(|(key, layers)| (key, ValueSampler::new(layers)))
                .collect(),
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

impl BiomeSource for World {
    fn noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> Option<BiomeRef> {
        let biome_id = self.noise_biome_id(quart_x, quart_y, quart_z)?;
        REGISTRY.biomes.by_id(usize::from(biome_id))
    }

    fn biome(&self, pos: BlockPos) -> Option<BiomeRef> {
        self.biome_at(pos)
    }
}
