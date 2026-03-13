//! Surface rule context types used by both generated code and runtime.

use crate::BlockStateId;
use crate::random::name_hash::NameHash;

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
    /// Numeric biome ID at the current position.
    pub biome_id: u16,
    /// Whether the current biome is cold enough to snow at this position.
    pub cold_enough_to_snow: bool,
    /// Reference to the surface system for noise lookups and band generation.
    pub system: &'a dyn SurfaceNoiseProvider,
}

/// Trait for providing noise values and clay band data to surface rules.
///
/// Implemented by `SurfaceSystem` in steel-core. The transpiled code calls these
/// methods through the `SurfaceRuleContext.system` field.
pub trait SurfaceNoiseProvider {
    /// Sample a surface condition noise at (x, z). The noise is identified by
    /// its index in the dimension's `surface_noise_ids()` list.
    fn get_noise(&self, noise_index: usize, x: i32, z: i32) -> f64;

    /// Get the badlands clay band block at position (x, y, z).
    fn get_band(&self, x: i32, y: i32, z: i32) -> BlockStateId;

    /// Evaluate a vertical gradient condition using positional random.
    ///
    /// Returns true if the random value at `(block_x, block_y, block_z)` falls
    /// within the gradient between `true_at_and_below` and `false_at_and_above`.
    fn vertical_gradient(
        &self,
        random_name: &NameHash,
        block_x: i32,
        block_y: i32,
        block_z: i32,
        true_at_and_below: i32,
        false_at_and_above: i32,
    ) -> bool;
}
