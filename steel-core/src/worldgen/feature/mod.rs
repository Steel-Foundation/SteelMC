//! Biome decoration runner for the `FEATURES` chunk stage.
//!
//! Vanilla treats biome decoration as one ordered pass over structure pieces and placed
//! features. This module builds the same per-step placed-feature ordering up front and
//! drives the per-chunk decoration seed loop. Placed-feature modifiers and selector
//! configured features execute normally; concrete block-mutating configured features are
//! added through the configured-feature runtime registry.

mod configured;
mod features;
pub(crate) mod instrumentation;
mod placed;
mod placement;
mod predicates;
mod prelude;
mod providers;
mod runner;
mod sorter;
mod state;
mod vanilla_collections;
mod weather;

pub(crate) use runner::FeatureDecorationRunner;

#[cfg(test)]
mod tests;
