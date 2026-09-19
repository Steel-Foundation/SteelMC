use steel_registry::DyeColor;

mod stained_glass_block;
mod stained_glass_pane_block;
pub use stained_glass_block::StainedGlassBlock;
pub use stained_glass_pane_block::StainedGlassPaneBlock;

/// Vanilla `BeaconBeamBlock` interface.
pub trait BeaconBeamBlock {
    /// Returns the dye color used for the beacon beam.
    fn get_color(&self) -> DyeColor;
}
