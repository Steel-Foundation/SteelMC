//! Surface rule context types used by both generated code and runtime.

use std::cell::Cell;

use crate::BlockStateId;

/// Context data passed to transpiled surface rule functions.
///
/// This is a flat struct holding all the values a surface rule condition might need.
/// The `SurfaceContext` in steel-core populates this and passes it to the generated
/// `try_apply_surface_rule()` function.
pub struct SurfaceRuleContext<'a> {
    /// World X coordinate.
    pub block_x: i32,
    /// World Z coordinate.
    pub block_z: i32,
    /// Noise-based surface layer thickness (typically 3-6 blocks).
    pub surface_depth: i32,
    /// Surface secondary noise value for depth variation.
    pub surface_secondary: f64,
    /// Minimum surface level from preliminary surface interpolation.
    pub min_surface_level: i32,
    /// Whether this column has a steep slope.
    pub steep: bool,
    /// World Y coordinate.
    pub block_y: i32,
    /// How many solid blocks above the current position.
    pub stone_depth_above: i32,
    /// How many solid blocks below until the next cavity.
    pub stone_depth_below: i32,
    /// Y of water surface above this block, or `i32::MIN` if no water.
    pub water_height: i32,
    /// Cached numeric biome ID at the current position, if already known.
    biome_id: Option<u16>,
    /// Lazy biome lookup for rules that need the biome at this Y position.
    biome_provider: Option<&'a mut dyn SurfaceBiomeProvider>,
    /// Reference to the surface system for noise lookups and band generation.
    pub system: &'a dyn SurfaceNoiseProvider,
    /// Lazily populated column cache for surface condition noise values.
    condition_noises: &'a SurfaceConditionNoiseCache<'a>,
    /// Pre-resolved block states returned by generated surface rules.
    block_states: &'a [BlockStateId],
    /// Lazily computed temperature condition value.
    cold_enough_to_snow: Option<bool>,
}

impl<'a> SurfaceRuleContext<'a> {
    /// Creates a surface rule context for one block position.
    #[expect(
        clippy::too_many_arguments,
        reason = "surface rule context mirrors vanilla's flat condition input"
    )]
    pub fn new(
        block_x: i32,
        block_z: i32,
        surface_depth: i32,
        surface_secondary: f64,
        min_surface_level: i32,
        steep: bool,
        block_y: i32,
        stone_depth_above: i32,
        stone_depth_below: i32,
        water_height: i32,
        biome_id: Option<u16>,
        biome_provider: Option<&'a mut dyn SurfaceBiomeProvider>,
        system: &'a dyn SurfaceNoiseProvider,
        condition_noises: &'a SurfaceConditionNoiseCache<'a>,
        block_states: &'a [BlockStateId],
    ) -> Self {
        Self {
            block_x,
            block_z,
            surface_depth,
            surface_secondary,
            min_surface_level,
            steep,
            block_y,
            stone_depth_above,
            stone_depth_below,
            water_height,
            biome_id,
            biome_provider,
            system,
            condition_noises,
            block_states,
            cold_enough_to_snow: None,
        }
    }

    /// Returns a column-cached surface condition noise value.
    #[must_use]
    pub fn condition_noise(&self, noise_index: usize) -> f64 {
        self.condition_noises
            .get(noise_index, self.system, self.block_x, self.block_z)
    }

    /// Returns a pre-resolved block state emitted by the generated surface rule.
    #[must_use]
    pub const fn block_state(&self, block_state_index: usize) -> BlockStateId {
        self.block_states[block_state_index]
    }

    /// Returns a biome ID already supplied by the caller.
    #[must_use]
    pub const fn known_biome_id(&self) -> Option<u16> {
        self.biome_id
    }

    /// Returns the current biome ID if this context was built with one.
    #[must_use]
    pub fn biome_id(&mut self) -> Option<u16> {
        if self.biome_id.is_some() {
            return self.biome_id;
        }

        let provider = self.biome_provider.as_mut()?;
        let biome_id = provider.biome_id(self.block_y);
        self.biome_id = Some(biome_id);
        Some(biome_id)
    }

    /// Lazily evaluates the vanilla temperature surface condition.
    #[must_use]
    pub fn cold_enough_to_snow(&mut self) -> bool {
        if let Some(value) = self.cold_enough_to_snow {
            return value;
        }

        let Some(biome_id) = self.biome_id() else {
            return false;
        };

        let value =
            self.system
                .cold_enough_to_snow(biome_id, self.block_x, self.block_y, self.block_z);
        self.cold_enough_to_snow = Some(value);
        value
    }
}

/// Supplies vanilla-fuzzed biome IDs to generated surface rules on demand.
pub trait SurfaceBiomeProvider {
    /// Returns the biome ID for the given block Y in the current X/Z column.
    fn biome_id(&mut self, block_y: i32) -> u16;
}

/// Lazily caches x/z-only surface condition noise values for one column.
pub struct SurfaceConditionNoiseCache<'a> {
    values: &'a [Cell<f64>],
    initialized: &'a [Cell<bool>],
}

impl<'a> SurfaceConditionNoiseCache<'a> {
    /// Creates a cache backed by caller-owned reusable storage.
    #[must_use]
    pub fn new(values: &'a [Cell<f64>], initialized: &'a [Cell<bool>]) -> Self {
        debug_assert_eq!(values.len(), initialized.len());
        Self {
            values,
            initialized,
        }
    }

    /// Clears populated markers before reusing the cache for another column.
    pub fn reset(&self) {
        for initialized in self.initialized {
            initialized.set(false);
        }
    }

    /// Returns a cached noise value, computing it if this column has not used it yet.
    #[must_use]
    pub fn get(
        &self,
        noise_index: usize,
        system: &dyn SurfaceNoiseProvider,
        x: i32,
        z: i32,
    ) -> f64 {
        if self.initialized[noise_index].get() {
            return self.values[noise_index].get();
        }

        let value = system.condition_noise(noise_index, x, z);
        self.values[noise_index].set(value);
        self.initialized[noise_index].set(true);
        value
    }
}

/// Trait for providing noise values and clay band data to surface rules.
///
/// Implemented by `SurfaceSystem` in steel-core. The transpiled code calls these
/// methods through the `SurfaceRuleContext.system` field.
pub trait SurfaceNoiseProvider {
    /// Sample a surface condition noise at (x, z). The noise is identified by
    /// its index in the dimension's `surface_noise_ids()` list.
    fn condition_noise(&self, noise_index: usize, x: i32, z: i32) -> f64;

    /// Get the badlands clay band block at position (x, y, z).
    fn get_band(&self, x: i32, y: i32, z: i32) -> BlockStateId;

    /// Evaluates whether the biome temperature is cold enough for snow.
    fn cold_enough_to_snow(&self, biome_id: u16, block_x: i32, block_y: i32, block_z: i32) -> bool;

    /// Evaluate a vertical gradient condition using positional random.
    ///
    /// Returns true if the random value at `(block_x, block_y, block_z)` falls
    /// within the gradient between `true_at_and_below` and `false_at_and_above`.
    fn vertical_gradient(
        &self,
        gradient_index: usize,
        block_x: i32,
        block_y: i32,
        block_z: i32,
        true_at_and_below: i32,
        false_at_and_above: i32,
    ) -> bool;
}
