//! Biome decoration runner for the `FEATURES` chunk stage.
//!
//! Vanilla treats biome decoration as one ordered pass over structure pieces and placed
//! features. This module builds the same per-step placed-feature ordering up front and
//! drives the per-chunk decoration seed loop. Placed-feature modifiers and selector
//! configured features execute normally; concrete block-mutating configured features are
//! added through the configured-feature runtime registry.

mod basalt_pillar;
mod block_blob;
mod block_column;
mod block_pile;
mod configured;
mod disk;
mod ore;
mod placed;
mod placement;
mod predicates;
mod prelude;
mod providers;
mod runner;
mod simple_block;
mod sorter;
mod spring;
mod state;

pub(crate) use runner::FeatureDecorationRunner;

#[cfg(test)]
mod tests;
