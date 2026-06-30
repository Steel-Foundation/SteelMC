//! Block behavior implementations for vanilla blocks.
//!
//! The actual behavior registration is auto-generated from classes.json.
//! See `src/generated/behaviors.rs` for the generated registration code.

mod building;
mod colored;
mod container;
mod decoration;
mod fluid;
mod portal;
mod redstone;
mod utils;
pub mod vegetation;

pub use building::{
    BarrierBlock, BedBlock, CampfireBlock, DoorBlock, FenceBlock, FenceGateBlock, HayBlock,
    HoneyBlock, IronBarsBlock, LavaCauldronBlock, MagmaBlock, PotentSulfurBlock, PowderSnowBlock,
    RotatedPillarBlock, ScaffoldingBlock, SlabBlock, SlimeBlock, SpongeBlock, StairBlock,
    WallBlock, WaterloggedTransparentBlock, WeatherState, WeatheringCopper,
    WeatheringCopperBarsBlock, WeatheringCopperDoorBlock, WeatheringCopperFullBlock,
    WeatheringCopperGrateBlock, WeatheringCopperSlabBlock, WeatheringCopperStairBlock,
    WetSpongeBlock,
};
pub use colored::StainedGlassPaneBlock;
pub use container::{BarrelBlock, BeehiveBlock, CraftingTableBlock};
pub use decoration::{
    CakeBlock, CandleBlock, CandleCakeBlock, CeilingHangingSignBlock, ChainBlock,
    StandingSignBlock, TorchBlock, WallHangingSignBlock, WallSignBlock, WallTorchBlock,
    WeatheringCopperChainBlock,
};
pub use fluid::{BubbleColumnBlock, LiquidBlock};
pub use portal::{
    EndGatewayBlock, EndPortalBlock, EndPortalFrameBlock, FireBlock, NetherPortalBlock,
    SoulFireBlock,
};
pub use redstone::{ButtonBlock, RedstoneTorchBlock, RedstoneWallTorchBlock};
pub use vegetation::{
    AzaleaBlock, BambooSaplingBlock, BambooStalkBlock, BeetrootBlock, CactusBlock,
    CactusFlowerBlock, CarrotBlock, CocoaBlock, CropBlock, DoublePlantBlock, FlowerBlock,
    NetherSproutsBlock, NetherWartBlock, PitcherCropBlock, PotatoBlock, RootedDirtBlock,
    SeagrassBlock, SugarCaneBlock, SweetBerryBushBlock, TallFlowerBlock, TallGrassBlock,
    TallSeagrassBlock, TorchflowerCropBlock,
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
    SporeBlossomBlock, SulfurSpikeBlock, TallDryGrassBlock, TwistingVinesBlock,
    TwistingVinesPlantBlock, VineBlock, WeepingVinesBlock, WeepingVinesPlantBlock, WitherRoseBlock,
};
