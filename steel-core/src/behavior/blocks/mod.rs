//! Block behavior implementations for vanilla blocks.
//!
//! The actual behavior registration is auto-generated from classes.json.
//! See `src/generated/behaviors.rs` for the generated registration code.

mod building;
mod container;
mod decoration;
mod farming;
mod fluid;
mod portal;
mod redstone;
mod vegetation;

pub use building::{
    FenceBlock, RotatedPillarBlock, WeatherState, WeatheringCopper, WeatheringCopperFullBlock,
};
pub use container::{BarrelBlock, BeehiveBlock, CraftingTableBlock};
pub use decoration::{
    CandleBlock, CeilingHangingSignBlock, StandingSignBlock, TorchBlock, WallHangingSignBlock,
    WallSignBlock, WallTorchBlock,
};
pub use farming::{CactusBlock, CactusFlowerBlock, CropBlock, FarmlandBlock};
pub use fluid::LiquidBlock;
pub use portal::{EndPortalFrameBlock, FireBlock, NetherPortalBlock, SoulFireBlock};
pub use redstone::{ButtonBlock, RedstoneTorchBlock, RedstoneWallTorchBlock};
pub use vegetation::{
    AzaleaBlock, BambooStalkBlock, BushBlock, CarpetBlock, ChorusFlowerBlock, ChorusPlantBlock,
    DoublePlantBlock, DryVegetationBlock, FireflyBushBlock, FlowerBedBlock, FlowerBlock, KelpBlock,
    KelpPlantBlock, LeafLitterBlock, LilyPadBlock, MossyCarpetBlock, MushroomBlock,
    NetherFungusBlock, NetherRootsBlock, NetherSproutsBlock, SeaPickleBlock, SeagrassBlock,
    ShortDryGrassBlock, SmallDripleafBlock, SporeBlossomBlock, SweetBerryBushBlock,
    TallDryGrassBlock, TallFlowerBlock, TallGrassBlock, TallSeagrassBlock, WitherRoseBlock,
};
