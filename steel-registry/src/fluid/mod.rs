//! Fluid registry for Minecraft fluids.

use crate::vanilla_fluids;
use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// A fluid type definition (e.g., water, lava, empty).
#[derive(Debug, Clone)]
pub struct Fluid {
    /// The identifier for this fluid (e.g., "minecraft:water").
    pub key: Identifier,
    /// Whether this fluid is empty (air).
    pub is_empty: bool,
    /// Whether this is a source fluid (vs flowing).
    pub is_source: bool,
    /// The block this fluid places.
    pub block: Identifier,
    /// The bucket item for this fluid.
    pub bucket_item: Identifier,
    /// The source fluid identifier (for flowing fluids).
    pub source_fluid: Option<Identifier>,
    /// The flowing fluid identifier (for source fluids).
    pub flowing_fluid: Option<Identifier>,
    /// Tick delay for fluid updates.
    pub tick_delay: u32,
    /// Explosion resistance.
    pub explosion_resistance: f32,
}

pub type FluidRef = &'static Fluid;

impl PartialEq for FluidRef {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for FluidRef {}

/// A fluid state instance with amount and falling properties.
///
/// This is computed on-demand from block states rather than stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FluidState {
    /// The fluid type (water, lava, empty).
    pub fluid_id: FluidRef,
    /// The fluid amount (1-8, where 8 is a full block/source).
    pub amount: u8,
    /// Whether the fluid is falling (flows downward faster).
    pub falling: bool,
}

impl FluidState {
    /// The empty fluid state.
    pub const EMPTY: Self = Self {
        fluid_id: &vanilla_fluids::EMPTY,
        amount: 0,
        falling: false,
    };

    /// Creates a new fluid state.
    #[must_use]
    pub const fn new(fluid: FluidRef, amount: u8, falling: bool) -> Self {
        Self {
            fluid_id: fluid,
            amount,
            falling,
        }
    }

    /// Creates a source fluid state (amount=8, not falling).
    #[must_use]
    pub const fn source(fluid: FluidRef) -> Self {
        Self {
            fluid_id: fluid,
            amount: 8,
            falling: false,
        }
    }

    /// Creates a flowing fluid state.
    #[must_use]
    pub const fn flowing(fluid: FluidRef, amount: u8, falling: bool) -> Self {
        Self {
            fluid_id: fluid,
            amount,
            falling,
        }
    }

    /// Returns true if this is the empty fluid.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.fluid_id.is_empty || self.amount == 0
    }

    /// Returns true if this is a source block (full fluid, not falling).
    ///
    /// Checks both the registry `fluid_id.is_source` flag (primary discriminator,
    /// equivalent to vanilla checking if the type is a `SourceFluid`) and the
    /// data invariant `amount == 8 && !falling`, guarding against malformed chunk data.
    #[must_use]
    pub const fn is_source(&self) -> bool {
        self.fluid_id.is_source && self.amount == 8 && !self.falling
    }

    /// Returns the fluid's own height (0.0 to ~0.89).
    #[must_use]
    pub fn own_height(&self) -> f32 {
        if self.is_empty() {
            0.0
        } else {
            self.amount as f32 / 9.0
        }
    }

    /// Decodes a fluid state from a liquid block's LEVEL property (0-15).
    ///
    /// - LEVEL 0 = source (amount=8, falling=false)
    /// - LEVEL 1-7 = flowing levels 7-1 (amount = 8 - level)
    /// - LEVEL 8-15 = falling fluid (amount=8, falling=true, but clamped)
    #[must_use]
    pub const fn from_block_level(fluid: FluidRef, level: u8) -> Self {
        if level == 0 {
            // Source block
            Self::source(fluid)
        } else if level <= 7 {
            // Flowing fluid: level 1 = amount 7, level 7 = amount 1
            Self::flowing(fluid, 8 - level, false)
        } else {
            // Falling fluid (level 8-15): vanilla encodes as 8 + (8 - amount)
            // so amount = 16 - level. In practice only level=8 (amount=8) is used.
            let amount = 16u8.saturating_sub(level).max(1);
            Self::flowing(fluid, amount, true)
        }
    }

    /// Encodes this fluid state to a liquid block's LEVEL property (0-15).
    #[must_use]
    pub const fn to_block_level(&self) -> u8 {
        if self.is_source() {
            0
        } else if self.falling {
            8
        } else {
            // amount 7 -> level 1, amount 1 -> level 7
            8 - self.amount
        }
    }
}

/// Registry for all fluids.
pub struct FluidRegistry {
    fluids_by_id: Vec<FluidRef>,
    fluids_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl Default for FluidRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FluidRegistry {
    /// Creates a new, empty fluid registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            fluids_by_id: Vec::new(),
            fluids_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }

    /// Registers a fluid and returns its ID.
    pub fn register(&mut self, fluid: FluidRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register fluids after the registry has been frozen"
        );

        let id = self.fluids_by_id.len();
        self.fluids_by_key.insert(fluid.key.clone(), id);
        self.fluids_by_id.push(fluid);
        id
    }

    /// Replaces a fluid at a given index.
    /// Returns true if the fluid was replaced and false if the fluid wasn't replaced
    #[must_use]
    pub fn replace(&mut self, item: FluidRef, id: usize) -> bool {
        if id >= self.fluids_by_id.len() {
            return false;
        }
        self.fluids_by_id[id] = item;
        true
    }

    /// Iterates over all fluids with their IDs.
    pub fn iter(&self) -> impl Iterator<Item = (usize, FluidRef)> + '_ {
        self.fluids_by_id
            .iter()
            .enumerate()
            .map(|(id, &fluid)| (id, fluid))
    }
}

crate::impl_registry!(FluidRegistry, Fluid, fluids_by_id, fluids_by_key, fluids);
crate::impl_tagged_registry!(FluidRegistry, fluids_by_key, "fluid");

// --- Fluid type checking helpers ---

use crate::{REGISTRY, TaggedRegistryExt, vanilla_fluid_tags};

/// Returns true if the given `FluidRef` is water (including flowing water).
#[must_use]
pub fn is_water_fluid(fluid: FluidRef) -> bool {
    !fluid.is_empty
        && REGISTRY
            .fluids
            .is_in_tag(fluid, &vanilla_fluid_tags::WATER_TAG)
}

/// Returns true if the given `FluidRef` is lava (including flowing lava).
#[must_use]
pub fn is_lava_fluid(fluid: FluidRef) -> bool {
    !fluid.is_empty
        && REGISTRY
            .fluids
            .is_in_tag(fluid, &vanilla_fluid_tags::LAVA_TAG)
}

/// Extension trait for `FluidState` type-checking methods.
pub trait FluidStateExt {
    /// Returns true if this fluid state contains water.
    fn is_water(&self) -> bool;
    /// Returns true if this fluid state contains lava.
    fn is_lava(&self) -> bool;
}

impl FluidStateExt for FluidState {
    fn is_water(&self) -> bool {
        is_water_fluid(self.fluid_id)
    }
    fn is_lava(&self) -> bool {
        is_lava_fluid(self.fluid_id)
    }
}
