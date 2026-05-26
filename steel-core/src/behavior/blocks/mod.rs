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
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
pub mod vegetation;
=======
mod vegetation;
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
=======
pub mod vegetation;
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
=======
pub mod vegetation;
>>>>>>> refs/remotes/origin/master

pub use building::{
    DoorBlock, FenceBlock, RotatedPillarBlock, SlabBlock, StairBlock, WeatherState,
    WeatheringCopper, WeatheringCopperDoorBlock, WeatheringCopperFullBlock,
    WeatheringCopperSlabBlock, WeatheringCopperStairBlock,
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
<<<<<<< HEAD
<<<<<<< HEAD
<<<<<<< HEAD
=======
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
=======
>>>>>>> refs/remotes/origin/master
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
<<<<<<< HEAD
<<<<<<< HEAD
=======
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
>>>>>>> 3643c5b7e (Add worldgen features stage (#183))
=======
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
=======
>>>>>>> refs/remotes/origin/master
};
