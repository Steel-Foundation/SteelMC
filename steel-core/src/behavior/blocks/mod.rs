//! Block behavior implementations for vanilla blocks.
//!
//! The actual behavior registration is auto-generated from classes.json.
//! See `src/generated/behaviors.rs` for the generated registration code.

mod building;
mod colored;
mod container;
mod decoration;
mod falling;
mod fluid;
mod portal;
mod redstone;
mod utils;
pub mod vegetation;

pub use building::{
    AmethystBlock, AmethystClusterBlock, BarrierBlock, BedBlock, BrushableBlock,
    BuddingAmethystBlock, CampfireBlock, CauldronBlock, ComposterBlock, CryingObsidianBlock,
    DoorBlock, DropExperienceBlock, FenceBlock, FenceGateBlock, GlazedTerracottaBlock, HayBlock,
    HeavyCoreBlock, HoneyBlock, IceBlock, IronBarsBlock, LadderBlock, LavaCauldronBlock,
    LayeredCauldronBlock, MagmaBlock, MudBlock, PotentSulfurBlock, PowderSnowBlock,
    RotatedPillarBlock, ScaffoldingBlock, SlabBlock, SlimeBlock, SnowLayerBlock, SpongeBlock,
    StairBlock, TrapDoorBlock, WallBlock, WaterloggedTransparentBlock, WeatherState,
    WeatheringCopper, WeatheringCopperBarsBlock, WeatheringCopperDoorBlock,
    WeatheringCopperFullBlock, WeatheringCopperGrateBlock, WeatheringCopperSlabBlock,
    WeatheringCopperStairBlock, WeatheringCopperTrapDoorBlock, WebBlock, WetSpongeBlock,
};
pub use colored::StainedGlassPaneBlock;
pub use container::{
    AnvilBlock, BarrelBlock, BeehiveBlock, ChiseledBookShelfBlock, CraftingTableBlock,
};
pub use decoration::{
    BannerBlock, CakeBlock, CandleBlock, CandleCakeBlock, CeilingHangingSignBlock, ChainBlock,
    EndRodBlock, LanternBlock, StandingSignBlock, TorchBlock, WallBannerBlock,
    WallHangingSignBlock, WallSignBlock, WallTorchBlock, WeatheringCopperChainBlock,
    WeatheringLanternBlock,
};
pub use falling::{ConcretePowderBlock, FallingBlock, SandBlock};
pub use fluid::{BubbleColumnBlock, LiquidBlock};
pub use portal::{
    EndGatewayBlock, EndPortalBlock, EndPortalFrameBlock, FireBlock, NetherPortalBlock,
    RespawnAnchorBlock, SoulFireBlock,
};
pub use redstone::{
    ButtonBlock, ComparatorBlock, CopperBulbBlock, DaylightDetectorBlock, DetectorRailBlock,
    LeverBlock, MovingPistonBlock, NoteBlock, ObserverBlock, PistonBaseBlock, PistonHeadBlock,
    PoweredBlock, PoweredRailBlock, PressurePlateBlock, PressurePlateSensitivity, RailBlock,
    RedStoneOreBlock, RedStoneWireBlock, RedstoneLampBlock, RedstoneTorchBlock,
    RedstoneWallTorchBlock, RepeaterBlock, TargetBlock, TripWireBlock, TripWireHookBlock,
    WeatheringCopperBulbBlock, WeightedPressurePlateBlock,
};
pub(crate) use redstone::{MAX_REDSTONE_SIGNAL, MIN_REDSTONE_SIGNAL};
pub use vegetation::{
    AttachedStemBlock, AzaleaBlock, BambooSaplingBlock, BambooStalkBlock, BeetrootBlock,
    CactusBlock, CactusFlowerBlock, CarrotBlock, CocoaBlock, CoralBlock, CropBlock,
    DoublePlantBlock, FlowerBlock, MangroveLeavesBlock, MultifaceBlock, NetherSproutsBlock,
    NetherWartBlock, PitcherCropBlock, PotatoBlock, PumpkinBlock, RootedDirtBlock, SeagrassBlock,
    StemBlock, SugarCaneBlock, SweetBerryBushBlock, TallFlowerBlock, TallGrassBlock,
    TallSeagrassBlock, TintedParticleLeavesBlock, TorchflowerCropBlock,
    UntintedParticleLeavesBlock,
};
pub use vegetation::{
    BaseCoralFanBlock, BaseCoralPlantBlock, BaseCoralWallFanBlock, BigDripleafBlock,
    BigDripleafStemBlock, BushBlock, CarpetBlock, CaveVinesBlock, CaveVinesPlantBlock,
    ChorusFlowerBlock, ChorusPlantBlock, CoralFanBlock, CoralPlantBlock, CoralWallFanBlock,
    DirtPathBlock, DryVegetationBlock, EyeblossomBlock, EyeblossomType, FarmlandBlock,
    FireflyBushBlock, FlowerBedBlock, GlowLichenBlock, HangingMossBlock, HangingRootsBlock,
    KelpBlock, KelpPlantBlock, LeafLitterBlock, LilyPadBlock, MangrovePropaguleBlock,
    MossyCarpetBlock, MushroomBlock, NetherFungusBlock, NetherRootsBlock, PointedDripstoneBlock,
    SaplingBlock, SculkVeinBlock, SeaPickleBlock, ShortDryGrassBlock, SmallDripleafBlock,
    SporeBlossomBlock, SulfurSpikeBlock, TallDryGrassBlock, TwistingVinesBlock,
    TwistingVinesPlantBlock, VineBlock, WeepingVinesBlock, WeepingVinesPlantBlock, WitherRoseBlock,
    WoolCarpetBlock,
};
