/// Structure piece generation context.
pub mod generation;
/// Core structure traits.
pub mod structure;
/// Structure starts and references.
pub mod structure_ref;

/// Block classification in the base-noise column (no surface rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnBlock {
    /// Empty.
    Air,
    /// Aquifer-placed fluid (lava/water).
    Fluid,
    /// Default solid block (stone, netherrack, end stone).
    Solid,
}
