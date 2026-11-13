//! Vanilla `net.minecraft.world.attribute`: typed environment attribute
//! definitions, their value types, modifiers, and attribute maps.
//!
//! Attribute definitions come from the built-in `environment_attribute`
//! registry extracted by `SteelExtractor`. The typed layer chains
//! ([`ValueSampler`]) and the layers built from registry data live here; the
//! per-world system that assembles them, and the weather layers, live in
//! `steel-core`'s `world::environment_attributes`.
//!
//! Every vanilla attribute type is implemented except `activity`.
// TODO: Implement the `activity` attribute type (villager schedules) once Steel
// has the built-in activity registry.

mod attribute_type;
mod layer;
mod lazy_value;
mod map;
mod modifier;
mod range;
mod sampler;
mod values;

use std::fmt::Debug;

use rustc_hash::FxHashMap;
use steel_utils::{Downcast as _, DowncastType, DowncastTypeKey, ErasedType, Identifier};

pub use attribute_type::{
    AMBIENT_PARTICLES, AMBIENT_SOUNDS, ANGLE_DEGREES, ARGB_COLOR, AttributeType, BACKGROUND_MUSIC,
    BED_RULE, BOOLEAN, FLOAT, INTEGER, MOB_SPAWN_SETTINGS, MOON_PHASE, PARTICLE, RGB_COLOR,
    TRI_STATE,
};
pub use layer::{
    BiomeLayer, BiomeSource, ClockSource, ConstantLayer, DimensionLayer, EnvironmentAttributeLayer,
    EnvironmentSampleContext, PositionalLayer, TimeBasedLayer, TimelineTrackLayer,
};
pub use lazy_value::LazyValue;
pub use map::{EnvironmentAttributeMap, EnvironmentAttributeMapEntry};
pub use modifier::{
    AttributeModifier, AttributeModifierEntry, BlendToGray, BlendToGrayModifier, BooleanModifier,
    ColorAlphaBlendModifier, ColorMultiplyModifier, ErasedAttributeModifierEntry,
    FloatAlphaBlendModifier, FloatModifier, FloatWithAlpha, IntegerModifier, ListAppendModifier,
    MobSpawnSettingsOverlayModifier, OperationId, OverrideModifier, RgbModifier,
    TypedAttributeModifierEntry,
};
pub use range::AttributeRange;
pub use sampler::{ErasedValueSampler, LayerValueTypeMismatch, ValueSampler};
pub use values::{
    AmbientAdditionsSettings, AmbientMoodSettings, AmbientParticle, AmbientSounds, AttributeList,
    BackgroundMusic, BedRule, BedRuleValue, CategorySpawns, MobSpawnCost, MobSpawnSettings,
    MoonPhase, Music, SpawnerData, TriState, Weighted, argb, lerp_ambient_particles,
    lerp_list_cross_fade,
};

/// A value an environment attribute can hold.
///
/// Values are recovered from type-erased maps and layers through Steel's keyed
/// downcasting, so plugin value types only need a [`DowncastType`] key.
pub trait AttributeValue: DowncastType + Clone + Debug + Send + Sync {}

impl<T> AttributeValue for T where T: DowncastType + Clone + Debug + Send + Sync {}

/// Behavior flags of an environment attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the independent flags of vanilla's EnvironmentAttribute"
)]
pub struct EnvironmentAttributeFlags {
    /// Whether the attribute is sent to clients in registry data.
    pub syncable: bool,
    /// Whether the attribute may vary by position (biome layers).
    pub positional: bool,
    /// Whether clients blend the attribute across nearby biomes.
    pub spatially_interpolated: bool,
    /// Whether the biome layer samples the zoomed (fuzzed) biome instead of
    /// the noise biome at the position's quart.
    pub full_resolution_biomes: bool,
}

impl EnvironmentAttributeFlags {
    /// Vanilla builder defaults: positional only.
    pub const DEFAULT: Self = Self {
        syncable: false,
        positional: true,
        spatially_interpolated: false,
        full_resolution_biomes: false,
    };
}

impl Default for EnvironmentAttributeFlags {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A typed environment attribute definition.
#[derive(Debug)]
pub struct EnvironmentAttribute<V: AttributeValue> {
    key: Identifier,
    attribute_type: &'static AttributeType<V>,
    default_value: LazyValue<V>,
    value_range: AttributeRange<V>,
    flags: EnvironmentAttributeFlags,
}

impl<V: AttributeValue> EnvironmentAttribute<V> {
    /// Creates an attribute definition.
    #[must_use]
    pub const fn new(
        key: Identifier,
        attribute_type: &'static AttributeType<V>,
        default_value: LazyValue<V>,
        value_range: AttributeRange<V>,
        flags: EnvironmentAttributeFlags,
    ) -> Self {
        Self {
            key,
            attribute_type,
            default_value,
            value_range,
            flags,
        }
    }

    #[must_use]
    pub const fn key(&self) -> &Identifier {
        &self.key
    }

    #[must_use]
    pub const fn attribute_type(&self) -> &'static AttributeType<V> {
        self.attribute_type
    }

    #[must_use]
    pub fn default_value(&self) -> &V {
        self.default_value.get()
    }

    #[must_use]
    pub const fn flags(&self) -> EnvironmentAttributeFlags {
        self.flags
    }

    #[must_use]
    pub const fn is_positional(&self) -> bool {
        self.flags.positional
    }

    /// Clamps `value` into this attribute's valid range.
    #[must_use]
    pub fn sanitize_value(&self, value: V) -> V {
        self.value_range.sanitize(value)
    }
}

impl<V: AttributeValue> PartialEq for EnvironmentAttribute<V> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<V: AttributeValue> Eq for EnvironmentAttribute<V> {}

// SAFETY: The key includes `V`'s key, so every instantiation of this
// Steel-owned type has its own key.
unsafe impl<V: AttributeValue> DowncastType for EnvironmentAttribute<V> {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::generic(
        "steel:environment_attribute/environment_attribute",
        &[V::TYPE_KEY],
    );
}

/// Object-safe access to an environment attribute of any value type.
///
/// Recover the typed attribute with [`downcast`](#method.downcast).
pub trait ErasedEnvironmentAttribute: ErasedType + Debug + Send + Sync {
    fn key(&self) -> &Identifier;
    fn flags(&self) -> EnvironmentAttributeFlags;
    /// Downcast key of the attribute's value type.
    fn value_type_key(&self) -> DowncastTypeKey;

    /// Creates an empty sampler of this attribute's value type, so layers
    /// built from erased registry data can be typed.
    fn new_value_sampler(&'static self) -> Box<dyn ErasedValueSampler>;
}

impl<V: AttributeValue> ErasedEnvironmentAttribute for EnvironmentAttribute<V> {
    fn key(&self) -> &Identifier {
        self.key()
    }

    fn flags(&self) -> EnvironmentAttributeFlags {
        self.flags()
    }

    fn value_type_key(&self) -> DowncastTypeKey {
        V::TYPE_KEY
    }

    fn new_value_sampler(&'static self) -> Box<dyn ErasedValueSampler> {
        Box::new(ValueSampler::new(self))
    }
}

impl dyn ErasedEnvironmentAttribute {
    /// Returns this attribute typed by its value type, or `None` when its
    /// value type is not `V`.
    #[must_use]
    pub fn downcast<V: AttributeValue>(&self) -> Option<&EnvironmentAttribute<V>> {
        let erased: &dyn ErasedType = self;
        erased.downcast_ref()
    }
}

pub type EnvironmentAttributeRef<V> = &'static EnvironmentAttribute<V>;
pub type ErasedEnvironmentAttributeRef = &'static dyn ErasedEnvironmentAttribute;

/// Registry of every environment attribute definition.
pub struct EnvironmentAttributeRegistry {
    attributes_by_id: Vec<ErasedEnvironmentAttributeRef>,
    attributes_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl EnvironmentAttributeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            attributes_by_id: Vec::new(),
            attributes_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    /// Registers an attribute and returns its id.
    ///
    /// # Panics
    ///
    /// Panics if the registry is frozen or the key is already registered.
    pub fn register<V: AttributeValue>(&mut self, attribute: EnvironmentAttributeRef<V>) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register environment attributes after registry has been frozen"
        );
        assert!(
            !self.attributes_by_key.contains_key(attribute.key()),
            "Cannot register duplicate environment attribute {}",
            attribute.key()
        );

        let id = self.attributes_by_id.len();
        self.attributes_by_key.insert(attribute.key().clone(), id);
        self.attributes_by_id.push(attribute);
        id
    }

    pub const fn freeze(&mut self) {
        self.allows_registering = false;
    }

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<ErasedEnvironmentAttributeRef> {
        self.attributes_by_id.get(id).copied()
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<ErasedEnvironmentAttributeRef> {
        self.id_from_key(key).and_then(|id| self.by_id(id))
    }

    #[must_use]
    pub fn id_from_key(&self, key: &Identifier) -> Option<usize> {
        self.attributes_by_key.get(key).copied()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.attributes_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.attributes_by_id.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, ErasedEnvironmentAttributeRef)> + '_ {
        self.attributes_by_id.iter().copied().enumerate()
    }
}

impl Default for EnvironmentAttributeRegistry {
    fn default() -> Self {
        Self::new()
    }
}
