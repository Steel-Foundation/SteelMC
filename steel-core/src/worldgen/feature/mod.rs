//! Biome decoration runner for the `FEATURES` chunk stage.
//!
//! Vanilla treats biome decoration as one ordered pass over structure pieces and placed
//! features. This module builds the same per-step placed-feature ordering up front and
//! drives the per-chunk decoration seed loop. Placed-feature modifiers and selector
//! configured features execute normally; concrete block-mutating configured features are
//! added through the configured-feature runtime registry.

mod basalt_columns;
mod basalt_pillar;
mod block_blob;
mod block_column;
mod block_pile;
mod blue_ice;
mod configured;
mod delta;
mod disk;
mod end_island;
mod end_platform;
mod glowstone_blob;
mod ore;
mod placed;
mod placement;
mod predicates;
mod prelude;
mod providers;
mod replace_blobs;
mod runner;
mod simple_block;
mod sorter;
mod spring;
mod state;
mod vines;
mod void_start_platform;

pub(crate) use runner::FeatureDecorationRunner;

#[cfg(test)]
mod tests;
