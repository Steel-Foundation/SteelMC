//! Hostile-mob markers.
//!
//! TODO(entity components): the `Monster` layer (sounds, spawn rules, light
//! checks) returns from `steel-core/legacy_entities/monster.rs` with the first
//! ported hostile mob.

/// Marks hostile entities, including those outside the `Monster` hierarchy.
/// Shared rules use this marker to reject leashing.
pub trait Enemy {}
