//! Block behavior implementations for vanilla blocks.
//!
//! The actual behavior registration is auto-generated from classes.json.
//! See `src/generated/behaviors.rs` for the generated registration code.

mod building;
mod container;
mod decoration;
mod fluid;
mod portal;
mod redstone;
pub mod vegetation;

pub use building::{
    BedBlock, CampfireBlock, DoorBlock, FenceBlock, HayBlock, HoneyBlock, LavaCauldronBlock,
    MagmaBlock, PowderSnowBlock, RotatedPillarBlock, ScaffoldingBlock, SlabBlock, SlimeBlock,
    StairBlock, WeatherState, WeatheringCopper, WeatheringCopperDoorBlock,
    WeatheringCopperFullBlock, WeatheringCopperSlabBlock, WeatheringCopperStairBlock,
};
pub use container::{BarrelBlock, BeehiveBlock, CraftingTableBlock};
pub use decoration::{
    CakeBlock, CandleBlock, CandleCakeBlock, CeilingHangingSignBlock, StandingSignBlock,
    TorchBlock, WallHangingSignBlock, WallSignBlock, WallTorchBlock,
};
pub use fluid::LiquidBlock;
pub use portal::{EndPortalFrameBlock, FireBlock, NetherPortalBlock, SoulFireBlock};
pub use redstone::{ButtonBlock, RedstoneTorchBlock, RedstoneWallTorchBlock};
pub use vegetation::{
    AzaleaBlock, BambooSaplingBlock, BambooStalkBlock, BeetrootBlock, CactusBlock,
    CactusFlowerBlock, CarrotBlock, CropBlock, DoublePlantBlock, FlowerBlock, NetherSproutsBlock,
    NetherWartBlock, PitcherCropBlock, PotatoBlock, SeagrassBlock, SugarCaneBlock,
    SweetBerryBushBlock, TallFlowerBlock, TallGrassBlock, TallSeagrassBlock, TorchflowerCropBlock,
};
pub use vegetation::{
    BaseCoralFanBlock, BaseCoralPlantBlock, BaseCoralWallFanBlock, BigDripleafBlock,
    BigDripleafStemBlock, BushBlock, CarpetBlock, CaveVinesBlock, CaveVinesPlantBlock,
    ChorusFlowerBlock, ChorusPlantBlock, CoralFanBlock, CoralPlantBlock, CoralWallFanBlock,
    DryVegetationBlock, EyeblossomBlock, EyeblossomType, FarmlandBlock, FireflyBushBlock,
    FlowerBedBlock, GlowLichenBlock, HangingMossBlock, HangingRootsBlock, KelpBlock,
    KelpPlantBlock, LeafLitterBlock, LilyPadBlock, MangrovePropaguleBlock, MossyCarpetBlock,
    MushroomBlock, NetherFungusBlock, NetherRootsBlock, PointedDripstoneBlock, SaplingBlock,
    SculkVeinBlock, SeaPickleBlock, ShortDryGrassBlock, SmallDripleafBlock, SnowLayerBlock,
    SporeBlossomBlock, TallDryGrassBlock, TwistingVinesBlock, TwistingVinesPlantBlock, VineBlock,
    WeepingVinesBlock, WeepingVinesPlantBlock, WitherRoseBlock,
};
