//! Regression tests for build-time density code generation.
// Exercise the build-time modules with the same types used to generate densities.
#[expect(
    dead_code,
    reason = "build-time types also describe operations outside these tests"
)]
#[path = "../build/density/types.rs"]
mod density;
#[expect(
    dead_code,
    reason = "build-time entry points are included for their bounds tests"
)]
#[path = "../build/density/transpiler/mod.rs"]
mod transpiler;
