//! Fluid registry for Minecraft fluids.

use crate::{RegistryExt, TaggedRegistryExt, vanilla_fluid_tags::FluidTag, vanilla_fluids};
use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// A fluid type definition (e.g., water, lava, empty).
#[derive(Debug)]
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

impl Fluid {
    /// Returns `true` if this fluid is tagged with the given tag.
    pub fn has_tag(&'static self, tag: &Identifier) -> bool {
        REGISTRY.fluids.is_in_tag(self, tag)
    }

    /// Returns this fluid's source variant.
    ///
    /// Vanilla's source and flowing fluids are distinct registry entries. Liquid
    /// blocks store a single block id plus a level property, so state decoding
    /// resolves the correct fluid variant from extracted fluid relationship data.
    pub fn source_variant(&'static self) -> FluidRef {
        let Some(source_key) = &self.source_fluid else {
            return self;
        };

        match REGISTRY.fluids.by_key(source_key) {
            Some(fluid) => fluid,
            None => panic!(
                "fluid `{}` references missing source fluid `{source_key}`",
                self.key
            ),
        }
    }

    /// Returns this fluid's flowing variant.
    pub fn flowing_variant(&'static self) -> FluidRef {
        let Some(flowing_key) = &self.flowing_fluid else {
            return self;
        };

        match REGISTRY.fluids.by_key(flowing_key) {
            Some(fluid) => fluid,
            None => panic!(
                "fluid `{}` references missing flowing fluid `{flowing_key}`",
                self.key
            ),
        }
    }
}

pub type FluidRef = &'static Fluid;

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

    /// Returns true if this state is owned by a source fluid type.
    ///
    /// Vanilla `FluidState.isSource()` delegates to the owning fluid. Source
    /// fluids can still carry `FALLING=true` through `FlowingFluid.getSource`.
    #[must_use]
    pub const fn is_source(&self) -> bool {
        self.fluid_id.is_source
    }

    /// Returns true if this fluid has vanilla's full amount (`8`).
    ///
    /// This intentionally does not require a source fluid type. Vanilla
    /// `FluidState.isFull()` is `getAmount() == 8`, so a falling full fluid is
    /// full without being a source.
    #[must_use]
    pub const fn is_full(&self) -> bool {
        self.amount == 8
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
    pub fn from_block_level(fluid: FluidRef, level: u8) -> Self {
        if level == 0 {
            // Source block
            Self::source(fluid.source_variant())
        } else if level <= 7 {
            // Flowing fluid: level 1 = amount 7, level 7 = amount 1
            Self::flowing(fluid.flowing_variant(), 8 - level, false)
        } else {
            // LiquidBlock clamps LEVEL 8-15 to the single cached falling state.
            Self::flowing(fluid.flowing_variant(), 8, true)
        }
    }

    /// Encodes this fluid state to a liquid block's LEVEL property (0-15).
    #[must_use]
    pub const fn to_block_level(self) -> u8 {
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

use crate::REGISTRY;

/// Returns true if the given `FluidRef` is water (including flowing water).
#[must_use]
pub fn is_water_fluid(fluid: FluidRef) -> bool {
    !fluid.is_empty && fluid.has_tag(&FluidTag::WATER)
}

/// Returns true if the given `FluidRef` is lava (including flowing lava).
#[must_use]
pub fn is_lava_fluid(fluid: FluidRef) -> bool {
    !fluid.is_empty && fluid.has_tag(&FluidTag::LAVA)
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

#[cfg(test)]
mod tests {
    use crate::{test_support::init_test_registry, vanilla_fluids};

    use super::*;

    #[test]
    fn from_block_level_uses_source_variant_for_level_zero() {
        init_test_registry();

        let water = FluidState::from_block_level(&vanilla_fluids::WATER, 0);
        let lava = FluidState::from_block_level(&vanilla_fluids::LAVA, 0);

        assert_eq!(water.fluid_id, &vanilla_fluids::WATER);
        assert_eq!(lava.fluid_id, &vanilla_fluids::LAVA);
        assert!(water.is_source());
        assert!(lava.is_source());
    }

    #[test]
    fn from_block_level_uses_flowing_variant_for_non_source_levels() {
        init_test_registry();

        let water = FluidState::from_block_level(&vanilla_fluids::WATER, 1);
        let lava = FluidState::from_block_level(&vanilla_fluids::LAVA, 8);

        assert_eq!(water.fluid_id, &vanilla_fluids::FLOWING_WATER);
        assert_eq!(lava.fluid_id, &vanilla_fluids::FLOWING_LAVA);
        assert!(!water.is_source());
        assert!(!lava.is_source());
        assert!(lava.falling);
    }

    #[test]
    fn from_block_level_clamps_all_falling_liquid_levels_to_full_amount() {
        init_test_registry();

        for level in 8..=15 {
            let water = FluidState::from_block_level(&vanilla_fluids::WATER, level);

            assert_eq!(water.fluid_id, &vanilla_fluids::FLOWING_WATER);
            assert_eq!(water.amount, 8);
            assert!(water.falling);
            assert!(water.is_full());
            assert!(!water.is_source());
        }
    }

    #[test]
    fn source_fluid_type_is_source_even_when_falling() {
        init_test_registry();

        let falling_source = FluidState::new(&vanilla_fluids::WATER, 8, true);

        assert!(falling_source.is_source());
        assert!(falling_source.is_full());
    }
}
