//! Ore vein generation during terrain fill.
//!
//! Matches vanilla's `OreVeinifier`. Evaluates the three vein density functions
//! (`vein_toggle`, `vein_ridged`, `vein_gap`) per solid block to decide whether
//! to replace stone with copper/iron ore, raw ore blocks, or filler (granite/tuff).

use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::BlockStateId;
use steel_utils::density::{ColumnCache, DimensionNoises};
use steel_utils::math::map_clamped;
use steel_utils::random::name_hash::NameHash;
use steel_utils::random::{PositionalRandom, Random, RandomSplitter};

/// Veininess magnitude must exceed this (after edge roundoff) to place any vein block.
const VEININESS_THRESHOLD: f64 = 0.4;
/// Within this many blocks of the vein type's Y boundary, the threshold tightens.
const EDGE_ROUNDOFF_BEGIN: f64 = 20.0;
/// Maximum tightening applied at the very edge of the Y range.
const MAX_EDGE_ROUNDOFF: f64 = -0.2;
/// Probability of NOT skipping a vein block (nextFloat must be <= this).
const VEIN_SOLIDNESS: f32 = 0.7;
/// Minimum richness (at veininess = 0.4).
const MIN_RICHNESS: f64 = 0.1;
/// Maximum richness (at veininess >= 0.6).
const MAX_RICHNESS: f64 = 0.3;
/// Probability of placing a raw ore block instead of ore.
const CHANCE_OF_RAW_ORE_BLOCK: f32 = 0.02;
/// Vein gap noise must be above this to place ore (otherwise filler).
const SKIP_ORE_IF_GAP_BELOW: f64 = -0.3;

/// A vein type with its Y range and block variants.
struct VeinType {
    ore: BlockStateId,
    raw_ore_block: BlockStateId,
    filler: BlockStateId,
    min_y: i32,
    max_y: i32,
}

/// Ore vein generator. Holds cached block state IDs and the positional random
/// splitter used for per-block randomness.
pub struct OreVeinifier {
    ore_splitter: RandomSplitter,
    copper: VeinType,
    iron: VeinType,
}

impl OreVeinifier {
    /// Create a new ore veinifier from the seed's positional splitter.
    ///
    /// The `splitter` should be the same one used to create `OverworldNoises`
    /// (i.e. from `Xoroshiro::from_seed(seed).next_positional()`).
    #[must_use]
    pub fn new(splitter: &RandomSplitter) -> Self {
        const ORE_HASH: NameHash = NameHash::new("minecraft:ore");
        let mut ore_rng = splitter.with_hash_of(&ORE_HASH);
        let ore_splitter = ore_rng.next_positional();

        let copper = VeinType {
            ore: REGISTRY
                .blocks
                .get_default_state_id(vanilla_blocks::COPPER_ORE),
            raw_ore_block: REGISTRY
                .blocks
                .get_default_state_id(vanilla_blocks::RAW_COPPER_BLOCK),
            filler: REGISTRY
                .blocks
                .get_default_state_id(vanilla_blocks::GRANITE),
            min_y: 0,
            max_y: 50,
        };

        let iron = VeinType {
            ore: REGISTRY
                .blocks
                .get_default_state_id(vanilla_blocks::DEEPSLATE_IRON_ORE),
            raw_ore_block: REGISTRY
                .blocks
                .get_default_state_id(vanilla_blocks::RAW_IRON_BLOCK),
            filler: REGISTRY.blocks.get_default_state_id(vanilla_blocks::TUFF),
            min_y: -60,
            max_y: -8,
        };

        Self {
            ore_splitter,
            copper,
            iron,
        }
    }

    /// Check if this solid block should be replaced with an ore vein block,
    /// using trilinearly interpolated vein density values.
    ///
    /// `interpolated` contains all interpolated channel values from the noise
    /// chunk. `combine_vein_toggle` and `combine_vein_ridged` extract the
    /// vein-specific channels and apply outer operations.
    ///
    /// `vein_gap` has no `Interpolated` marker so is evaluated directly.
    pub fn compute_interpolated<N: DimensionNoises>(
        &self,
        noises: &N,
        cache: &mut N::ColumnCache,
        interpolated: &[f64],
        world_x: i32,
        world_y: i32,
        world_z: i32,
    ) -> Option<BlockStateId> {
        let vein_toggle = if N::vein_interp_enabled() {
            noises.combine_vein_toggle(cache, interpolated, 0, world_y, 0)
        } else {
            cache.ensure(world_x, world_z, noises);
            noises.router_vein_toggle(cache, world_x, world_y, world_z)
        };

        // Select vein type based on sign of vein_toggle
        let vein_type = if vein_toggle > 0.0 {
            &self.copper
        } else {
            &self.iron
        };

        let veininess = vein_toggle.abs();

        // Check Y range
        let dist_from_top = vein_type.max_y - world_y;
        let dist_from_bottom = world_y - vein_type.min_y;
        if dist_from_bottom < 0 || dist_from_top < 0 {
            return None;
        }

        // Edge roundoff: tighten threshold near Y boundaries
        let dist_from_edge = dist_from_top.min(dist_from_bottom);
        let edge_roundoff = map_clamped(
            f64::from(dist_from_edge),
            0.0,
            EDGE_ROUNDOFF_BEGIN,
            MAX_EDGE_ROUNDOFF,
            0.0,
        );

        if veininess + edge_roundoff < VEININESS_THRESHOLD {
            return None;
        }

        // Per-block positional random
        let mut rng = self.ore_splitter.at(world_x, world_y, world_z);

        // Random solidness skip (30% chance to skip)
        if rng.next_f32() > VEIN_SOLIDNESS {
            return None;
        }

        // Ridged noise: uses interpolation if available
        let vein_ridged = if N::vein_interp_enabled() {
            noises.combine_vein_ridged(cache, interpolated, 0, world_y, 0)
        } else {
            cache.ensure(world_x, world_z, noises);
            noises.router_vein_ridged(cache, world_x, world_y, world_z)
        };
        if vein_ridged >= 0.0 {
            return None;
        }

        // Compute richness from veininess
        let richness = map_clamped(
            veininess,
            VEININESS_THRESHOLD,
            0.6,
            MIN_RICHNESS,
            MAX_RICHNESS,
        );

        if (f64::from(rng.next_f32())) < richness {
            // vein_gap has no Interpolated marker — evaluate directly
            cache.ensure(world_x, world_z, noises);
            let vein_gap = noises.router_vein_gap(cache, world_x, world_y, world_z);
            if vein_gap > SKIP_ORE_IF_GAP_BELOW {
                // Place ore (2% chance of raw ore block)
                if rng.next_f32() < CHANCE_OF_RAW_ORE_BLOCK {
                    Some(vein_type.raw_ore_block)
                } else {
                    Some(vein_type.ore)
                }
            } else {
                // Below gap threshold: filler block
                Some(vein_type.filler)
            }
        } else {
            // Below richness threshold: filler block
            Some(vein_type.filler)
        }
    }
}
