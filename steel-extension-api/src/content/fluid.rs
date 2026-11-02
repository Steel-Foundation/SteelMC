//! Registered fluids: a source and flowing pair with its own liquid block.
//!
//! Clients see the fluid as the vanilla liquid named by `client`, state by state,
//! so movement and rendering match that liquid. The server keeps the fluid's
//! own identity, spread rules, and hooks. A water-displayed fluid may carry a
//! water tint, which clients see in every biome cell that holds the fluid.

use stabby::{option::Option as ApiOption, string::String as ApiString};

use super::ItemDefinition;
use crate::BlockState;

/// Client water colors shown wherever the fluid is, as 24-bit RGB.
#[stabby::stabby]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FluidTint {
    /// Water surface color.
    pub water_color: i32,
    /// Underwater fog color.
    pub water_fog_color: i32,
}

/// A fluid pair, its liquid block, and its optional bucket.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct FluidDefinition {
    /// Source fluid key, in the owner's namespace.
    pub key: ApiString,
    /// Flowing fluid key; distinct from `key`.
    pub flowing_key: ApiString,
    /// Liquid block key, often the same as `key`.
    pub block_key: ApiString,
    /// Vanilla fluid clients see: `minecraft:water` or `minecraft:lava`.
    pub client: ApiString,
    /// The filled bucket item, registered with the fluid.
    pub bucket: ApiOption<ItemDefinition>,
    /// Water colors in this fluid's biome cells; requires `minecraft:water`.
    pub tint: ApiOption<FluidTint>,
    /// Explosion resistance of the liquid block.
    pub explosion_resistance: f32,
    /// Ticks between spread steps, from 1 to 1200 (water: 5, lava: 30).
    pub tick_delay: u16,
    /// Level lost per horizontal block, from 1 to 7 (water: 1, lava: 2).
    pub drop_off: u8,
    /// How far spreading looks for a downward slope, from 0 to 8 (water: 4).
    pub slope_find_distance: u8,
    /// Whether two sources form a new source, as water does by game rule.
    pub converts_to_source: bool,
    /// Whether the fluid receives random ticks (see `FluidBehavior::random_tick`).
    pub randomly_ticking: bool,
}

impl FluidDefinition {
    /// A water-displayed fluid named `key`, with `<key>_flowing`, a block named
    /// `key`, water's spread parameters, and no tint or bucket.
    #[must_use]
    pub fn water_like(key: &str) -> Self {
        Self {
            key: key.into(),
            flowing_key: format!("{key}_flowing").as_str().into(),
            block_key: key.into(),
            client: "minecraft:water".into(),
            tint: None.into(),
            tick_delay: 5,
            drop_off: 1,
            slope_find_distance: 4,
            converts_to_source: false,
            explosion_resistance: 100.0,
            randomly_ticking: false,
            bucket: None.into(),
        }
    }

    /// Sets the client water tint.
    #[must_use]
    pub fn tint(mut self, water_color: i32, water_fog_color: i32) -> Self {
        self.tint = Some(FluidTint {
            water_color,
            water_fog_color,
        })
        .into();
        self
    }

    /// Sets the spread speed and reach.
    #[must_use]
    pub const fn spread(mut self, tick_delay: u16, drop_off: u8) -> Self {
        self.tick_delay = tick_delay;
        self.drop_off = drop_off;
        self
    }

    /// Enables random ticks.
    #[must_use]
    pub const fn randomly_ticking(mut self) -> Self {
        self.randomly_ticking = true;
        self
    }

    /// Registers a bucket item for the fluid.
    #[must_use]
    pub fn bucket(mut self, item: ItemDefinition) -> Self {
        self.bucket = Some(item).into();
        self
    }
}

/// A registered fluid's liquid block states.
#[stabby::stabby]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisteredFluid {
    /// The source state (level 0).
    pub source: BlockState,
}
