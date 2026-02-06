//! Fluid registry for Minecraft fluids.

use crate::{RegistryExt, vanilla_fluids};
use rustc_hash::FxHashMap;
use steel_utils::Identifier;
use steel_utils::registry::registry_vanilla_or_custom_tag;

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
    #[must_use]
    pub const fn is_source(&self) -> bool {
        self.amount == 8 && !self.falling
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
            // Falling fluid (level 8-15)
            Self::flowing(fluid, 8, true)
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

    /// Gets a fluid by its numeric ID.
    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<FluidRef> {
        self.fluids_by_id.get(id).copied()
    }

    /// Gets the numeric ID for a fluid.
    #[must_use]
    pub fn get_id(&self, fluid: FluidRef) -> Option<&usize> {
        self.fluids_by_key.get(&fluid.key)
    }

    /// Gets a fluid by its key.
    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<FluidRef> {
        self.fluids_by_key.get(key).and_then(|id| self.by_id(*id))
    }

    /// Iterates over all fluids with their IDs.
    pub fn iter(&self) -> impl Iterator<Item = (usize, FluidRef)> + '_ {
        self.fluids_by_id
            .iter()
            .enumerate()
            .map(|(id, &fluid)| (id, fluid))
    }

    /// Returns the number of registered fluids.
    #[must_use]
    pub fn len(&self) -> usize {
        self.fluids_by_id.len()
    }

    /// Returns true if no fluids are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fluids_by_id.is_empty()
    }

    // Tag-related methods

    /// Registers a tag with a list of fluid keys.
    pub fn register_tag(&mut self, tag: Identifier, fluid_keys: &[&'static str]) {
        assert!(
            self.allows_registering,
            "Cannot register tags after registry has been frozen"
        );

        let identifier: Vec<Identifier> = fluid_keys
            .iter()
            .filter_map(|key| {
                let ident = registry_vanilla_or_custom_tag(key);
                // Only include if the item actually exists
                self.by_key(&ident).map(|_| ident)
            })
            .collect();

        self.tags.insert(tag, identifier);
    }

    /// Checks if a fluid is in a given tag.
    #[must_use]
    pub fn is_in_tag(&self, fluid: FluidRef, tag: &Identifier) -> bool {
        self.tags
            .get(tag)
            .is_some_and(|fluids| fluids.contains(&fluid.key))
    }

    /// Gives the access to all blocks to delete and add new entries
    pub fn modify_tag(
        &mut self,
        tag: &Identifier,
        f: impl FnOnce(Vec<Identifier>) -> Vec<Identifier>,
    ) {
        let existing = self.tags.remove(tag).unwrap_or_default();
        let fluids = f(existing)
            .into_iter()
            .filter(|fluid| {
                let exists = self.fluids_by_key.contains_key(fluid);
                if !exists {
                    tracing::error!("fluid {fluid} not found in registry, skipping from tag {tag}");
                }
                exists
            })
            .collect();
        self.tags.insert(tag.clone(), fluids);
    }

    /// Gets all fluids in a tag.
    #[must_use]
    pub fn get_tag(&self, tag: &Identifier) -> Option<Vec<FluidRef>> {
        self.tags.get(tag).map(|idents| {
            idents
                .iter()
                .filter_map(|ident| self.by_key(ident))
                .collect()
        })
    }

    /// Iterates over all fluids in a tag.
    pub fn iter_tag(&self, tag: &Identifier) -> impl Iterator<Item = FluidRef> + '_ {
        self.tags
            .get(tag)
            .into_iter()
            .flat_map(|v| v.iter().filter_map(|ident| self.by_key(ident)))
    }

    /// Gets all tag keys.
    pub fn tag_keys(&self) -> impl Iterator<Item = &Identifier> + '_ {
        self.tags.keys()
    }
}

impl RegistryExt for FluidRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}
