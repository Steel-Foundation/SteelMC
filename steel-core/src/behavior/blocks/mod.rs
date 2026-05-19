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
    DoorBlock, FenceBlock, RotatedPillarBlock, SlabBlock, StairBlock, WeatherState,
    WeatheringCopper, WeatheringCopperDoorBlock, WeatheringCopperFullBlock,
    WeatheringCopperSlabBlock, WeatheringCopperStairBlock,
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
    AzaleaBlock, BambooStalkBlock, BaseCoralFanBlock, BaseCoralPlantBlock, BaseCoralWallFanBlock,
    BigDripleafBlock, BigDripleafStemBlock, BushBlock, CarpetBlock, CaveVinesBlock,
    CaveVinesPlantBlock, ChorusFlowerBlock, ChorusPlantBlock, CoralFanBlock, CoralPlantBlock,
    CoralWallFanBlock, DoublePlantBlock, DryVegetationBlock, EyeblossomBlock, EyeblossomType,
    FireflyBushBlock, FlowerBedBlock, FlowerBlock, GlowLichenBlock, HangingMossBlock,
    HangingRootsBlock, KelpBlock, KelpPlantBlock, LeafLitterBlock, LilyPadBlock,
    MangrovePropaguleBlock, MossyCarpetBlock, MushroomBlock, NetherFungusBlock, NetherRootsBlock,
    NetherSproutsBlock, PointedDripstoneBlock, SaplingBlock, SculkVeinBlock, SeaPickleBlock,
    SeagrassBlock, ShortDryGrassBlock, SmallDripleafBlock, SnowLayerBlock, SporeBlossomBlock,
    SugarCaneBlock, SweetBerryBushBlock, TallDryGrassBlock, TallFlowerBlock, TallGrassBlock,
    TallSeagrassBlock, TwistingVinesBlock, TwistingVinesPlantBlock, VineBlock, WeepingVinesBlock,
    WeepingVinesPlantBlock, WitherRoseBlock,
};
