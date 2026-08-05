//! Configured carver registry.
//!
//! Mirrors vanilla's `ConfiguredWorldCarver` — each entry is a
//! `CarverType` (the carver kind — cave / `nether_cave` / canyon) paired with
//! its configuration. Configured carvers are referenced by biomes via the
//! `carvers` field on [`Biome`](crate::biome::Biome) and sampled during the
//! `CARVERS` chunk generation stage.

use std::sync::OnceLock;

use rustc_hash::FxHashMap;
use steel_utils::Identifier;
use steel_utils::random::{Random, legacy_random::LegacyRandom};
use steel_utils::value_providers::{FloatProvider, HeightProvider, VerticalAnchor};

/// Shared per-carver configuration fields present on every carver type.
///
/// Mirrors vanilla's `CarverConfiguration`. The `replaceable` block set is
/// stored as a tag identifier (e.g. `minecraft:overworld_carver_replaceables`)
/// and resolved against `BlockRegistry::is_in_tag` at carve time.
#[derive(Debug, Clone)]
pub struct CarverConfiguration {
    /// Per-chunk start probability (in `[0, 1]`).
    pub probability: f32,
    /// Carver origin Y coordinate provider.
    pub y: HeightProvider,
    /// Vertical-stretch multiplier for carved ellipsoids.
    pub y_scale: FloatProvider,
    /// Any block carved at or below this Y becomes lava instead of air.
    pub lava_level: VerticalAnchor,
    /// Tag of blocks the carver is allowed to replace.
    pub replaceable_tag: Identifier,
    // TODO: debug_settings parsed but ignored — only active when
    // `SharedConstants.DEBUG_CARVERS` is true, which is never the case in
    // a production build. Wire through if we ever want the cave visualiser.
}

/// Cave/nether-cave configuration.
///
/// Mirrors vanilla's `CaveCarverConfiguration`.
#[derive(Debug, Clone)]
pub struct CaveCarverConfiguration {
    /// Base configuration.
    pub base: CarverConfiguration,
    /// Per-tunnel horizontal-radius stretch factor.
    pub horizontal_radius_multiplier: FloatProvider,
    /// Per-tunnel vertical-radius stretch factor.
    pub vertical_radius_multiplier: FloatProvider,
    /// Shapes the cave floor — values in `[-1, 1]`. Blocks where the normalized
    /// vertical offset `yd` is below this value are skipped.
    pub floor_level: FloatProvider,
}

/// Canyon shape parameters — controls tunnel shape and width-per-height
/// variation.
///
/// Mirrors vanilla's `CanyonCarverConfiguration.CanyonShapeConfiguration`.
#[derive(Debug, Clone)]
pub struct CanyonShapeConfiguration {
    /// Fraction of the max carving distance used as the actual tunnel length.
    pub distance_factor: FloatProvider,
    /// Overall tunnel thickness.
    pub thickness: FloatProvider,
    /// Lower values = fresher width noise each step; higher = smoother.
    pub width_smoothness: i32,
    /// Per-step horizontal-radius multiplier.
    pub horizontal_radius_factor: FloatProvider,
    /// Baseline vertical-radius multiplier applied along the whole tunnel.
    pub vertical_radius_default_factor: f32,
    /// Extra vertical-radius multiplier that peaks at the tunnel midpoint.
    pub vertical_radius_center_factor: f32,
}

impl CanyonShapeConfiguration {
    /// Mirrors vanilla `CanyonWorldCarver.initWidthFactors` — fresh squared
    /// width factor at every `width_smoothness`-th Y level, otherwise
    /// repeating the previous value.
    #[must_use]
    pub fn init_width_factors(&self, gen_depth: i32, random: &mut LegacyRandom) -> Vec<f32> {
        let depth = gen_depth as usize;
        let mut factors = vec![0.0_f32; depth];
        let mut current = 1.0_f32;
        for (y_index, slot) in factors.iter_mut().enumerate() {
            if y_index == 0 || random.next_i32_bounded(self.width_smoothness) == 0 {
                current = 1.0 + random.next_f32() * random.next_f32();
            }
            *slot = current * current;
        }
        factors
    }

    /// Mirrors vanilla `CanyonWorldCarver.updateVerticalRadius` — applies the
    /// shape's default/center factors plus a `Mth.randomBetween(0.75, 1.0)`
    /// jitter.
    #[must_use]
    pub fn update_vertical_radius(
        &self,
        random: &mut LegacyRandom,
        vertical_radius: f64,
        distance: f32,
        current_step: f32,
    ) -> f64 {
        // Vanilla: `Mth.abs(0.5F - currentStep/distance)` — float arithmetic.
        let vertical_multiplier = 1.0_f32 - (0.5 - current_step / distance).abs() * 2.0;
        let factor = self.vertical_radius_default_factor
            + self.vertical_radius_center_factor * vertical_multiplier;
        // `Mth.randomBetween(random, 0.75F, 1.0F)` = 0.75 + nextFloat()*0.25.
        let jitter = 0.75 + random.next_f32() * 0.25;
        f64::from(factor) * vertical_radius * f64::from(jitter)
    }
}

/// Canyon (ravine) configuration.
///
/// Mirrors vanilla's `CanyonCarverConfiguration`.
#[derive(Debug, Clone)]
pub struct CanyonCarverConfiguration {
    /// Base configuration.
    pub base: CarverConfiguration,
    /// Per-tunnel vertical rotation amount.
    pub vertical_rotation: FloatProvider,
    /// Shape configuration.
    pub shape: CanyonShapeConfiguration,
}

/// Which carver algorithm a [`ConfiguredCarver`] uses, along with its
/// fully-resolved configuration.
#[derive(Debug, Clone)]
pub enum ConfiguredCarverKind {
    /// Branching cave tunnels (`CaveWorldCarver`).
    Cave(CaveCarverConfiguration),
    /// Nether variant of caves (`NetherWorldCarver`) — no aquifer lookups,
    /// and a fixed lava-or-cave-air substance decision.
    NetherCave(CaveCarverConfiguration),
    /// Long narrow ravines (`CanyonWorldCarver`).
    Canyon(CanyonCarverConfiguration),
}

/// A fully-configured carver, as referenced by biomes via their
/// `carvers` field.
///
/// Mirrors vanilla's `ConfiguredWorldCarver<?>`.
#[derive(Debug)]
pub struct ConfiguredCarver {
    /// Registry key (e.g. `minecraft:cave`, `minecraft:canyon`).
    pub key: Identifier,
    /// Which carver algorithm this is + its configuration.
    pub kind: ConfiguredCarverKind,
    /// Cached registry ID, set during registration for O(1) lookup on hot
    /// paths.
    pub id: OnceLock<usize>,
}

impl ConfiguredCarver {
    /// The base (shared) configuration for this carver.
    #[must_use]
    pub const fn base(&self) -> &CarverConfiguration {
        match &self.kind {
            ConfiguredCarverKind::Cave(c) | ConfiguredCarverKind::NetherCave(c) => &c.base,
            ConfiguredCarverKind::Canyon(c) => &c.base,
        }
    }
}

/// Read-only reference to a registered [`ConfiguredCarver`].
pub type ConfiguredCarverRef = &'static ConfiguredCarver;

/// Registry of configured carvers, keyed by namespaced identifier.
pub struct ConfiguredCarverRegistry {
    carvers_by_id: Vec<ConfiguredCarverRef>,
    carvers_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl ConfiguredCarverRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            carvers_by_id: Vec::new(),
            carvers_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    /// Registers a carver and returns its numeric ID.
    pub fn register(&mut self, entry: ConfiguredCarverRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register ConfiguredCarver after registry has been frozen"
        );
        let id = self.carvers_by_id.len();
        let cached = entry.id.get_or_init(|| id);
        assert_eq!(*cached, id, "carver registered with conflicting id");
        self.carvers_by_id.push(entry);
        self.carvers_by_key.insert(entry.key.clone(), id);
        id
    }

    /// Iterates over all registered carvers with their IDs.
    pub fn iter(&self) -> impl Iterator<Item = (usize, ConfiguredCarverRef)> + '_ {
        self.carvers_by_id
            .iter()
            .enumerate()
            .map(|(id, &entry)| (id, entry))
    }
}

impl Default for ConfiguredCarverRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry_ext!(
    ConfiguredCarverRegistry,
    ConfiguredCarver,
    carvers_by_id,
    carvers_by_key
);

crate::impl_registry_entry_eq!(ConfiguredCarver);

impl crate::RegistryEntry for ConfiguredCarver {
    fn key(&self) -> &Identifier {
        &self.key
    }

    fn try_id(&self) -> Option<usize> {
        self.id.get().copied()
    }
}
