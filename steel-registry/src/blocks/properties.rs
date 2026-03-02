use std::fmt::Debug;

pub use steel_utils::{Direction, codec::VarInt, math::Axis, serial::ReadFrom};

pub trait Property<T>: Sync + Send {
    fn get_value(&self, value: &str) -> Option<T>;
    fn get_possible_values(&self) -> Box<[T]>;
    fn get_internal_index(&self, value: &T) -> usize;
    fn value_from_index(&self, index: usize) -> T;
    fn as_dyn(&self) -> &dyn DynProperty;
}

pub trait DynProperty: Debug + Sync + Send {
    fn get_possible_values(&self) -> Box<[&str]>;
    fn get_name(&self) -> &'static str;
}

pub trait PropertyEnum: PartialEq + Clone + Debug + Sync + Send {
    fn as_str(&self) -> &str;
}

#[derive(Debug, Clone)]
pub struct BoolProperty {
    pub name: &'static str,
}
impl BoolProperty {
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }

    #[must_use]
    pub const fn value_count(&self) -> usize {
        2
    }

    /// Convert a boolean value to its internal index (true=0, false=1 for Java compatibility)
    #[must_use]
    pub const fn index_of(&self, value: bool) -> usize {
        !value as usize
    }
}

impl DynProperty for BoolProperty {
    fn get_possible_values(&self) -> Box<[&str]> {
        ["true", "false"].into()
    }

    fn get_name(&self) -> &'static str {
        self.name
    }
}

impl Property<bool> for BoolProperty {
    fn get_value(&self, value: &str) -> Option<bool> {
        if value == "true" {
            Some(true)
        } else if value == "false" {
            Some(false)
        } else {
            None
        }
    }

    fn get_possible_values(&self) -> Box<[bool]> {
        [true, false].into()
    }

    fn get_internal_index(&self, value: &bool) -> usize {
        usize::from(!*value)
    }

    fn value_from_index(&self, index: usize) -> bool {
        index == 0
    }

    fn as_dyn(&self) -> &dyn DynProperty {
        self
    }
}

impl BoolProperty {
    #[must_use]
    pub const fn get_internal_index_const(self, value: bool) -> usize {
        if value { 0 } else { 1 }
    }
}

// Instead of million heap allocs we just use 42 bytes of static mem :)
const NUM_STR: [&str; 26] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
    "17", "18", "19", "20", "21", "22", "23", "24", "25",
];

#[derive(Debug, Clone)]
pub struct IntProperty {
    pub min: u8,
    pub max: u8,
    pub name: &'static str,
}

impl IntProperty {
    #[must_use]
    pub const fn new(name: &'static str, min: u8, max: u8) -> Self {
        Self { min, max, name }
    }

    #[must_use]
    pub const fn value_count(&self) -> usize {
        (self.max - self.min + 1) as usize
    }
}

impl DynProperty for IntProperty {
    fn get_possible_values(&self) -> Box<[&str]> {
        (self.min..=self.max).map(|v| NUM_STR[v as usize]).collect()
    }

    fn get_name(&self) -> &'static str {
        self.name
    }
}

impl Property<u8> for IntProperty {
    fn get_value(&self, value: &str) -> Option<u8> {
        value
            .parse()
            .ok()
            .filter(|v| v >= &self.min && v <= &self.max)
    }

    fn get_possible_values(&self) -> Box<[u8]> {
        (self.min..=self.max).collect()
    }

    fn get_internal_index(&self, value: &u8) -> usize {
        if *value <= self.max {
            (*value - self.min) as usize
        } else {
            0
        }
    }

    fn value_from_index(&self, index: usize) -> u8 {
        self.min + index as u8
    }

    fn as_dyn(&self) -> &dyn DynProperty {
        self
    }
}

impl IntProperty {
    #[must_use]
    pub const fn get_internal_index_const(self, value: &u8) -> usize {
        if *value <= self.max {
            (*value - self.min) as usize
        } else {
            0
        }
    }
}

#[derive(Debug, Clone)]
pub struct EnumProperty<T: PropertyEnum + 'static> {
    pub name: &'static str,
    pub possible_values: &'static [T],
}

impl<T: PropertyEnum + 'static> DynProperty for EnumProperty<T> {
    fn get_possible_values(&self) -> Box<[&str]> {
        self.possible_values
            .iter()
            .map(PropertyEnum::as_str)
            .collect()
    }

    fn get_name(&self) -> &'static str {
        self.name
    }
}

impl<T: PropertyEnum> EnumProperty<T> {
    pub const fn new(name: &'static str, possible_values: &'static [T]) -> Self {
        Self {
            name,
            possible_values,
        }
    }

    #[must_use]
    pub const fn value_count(&self) -> usize {
        self.possible_values.len()
    }
}

impl<T: PropertyEnum> Property<T> for EnumProperty<T> {
    fn get_value(&self, value: &str) -> Option<T> {
        self.possible_values
            .iter()
            .find(|v| v.as_str() == value)
            .cloned()
    }

    fn get_possible_values(&self) -> Box<[T]> {
        self.possible_values.into()
    }

    fn get_internal_index(&self, value: &T) -> usize {
        self.possible_values
            .iter()
            .position(|v| v == value)
            .unwrap()
    }

    fn value_from_index(&self, index: usize) -> T {
        self.possible_values[index].clone()
    }

    fn as_dyn(&self) -> &dyn DynProperty {
        self
    }
}

impl<T: const PartialEq + PropertyEnum + 'static> EnumProperty<T> {
    pub const fn get_internal_index_const(&self, value: &T) -> usize {
        let mut i = 0;
        while i < self.possible_values.len() {
            if &self.possible_values[i] == value {
                return i;
            }
            i += 1;
        }
        panic!("value not found in possible_values");
    }
}

impl PropertyEnum for Direction {
    fn as_str(&self) -> &str {
        Direction::as_str(self)
    }
}

// Additional enum types for properties
#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum FrontAndTop {
    DownEast,
    DownNorth,
    DownSouth,
    DownWest,
    UpEast,
    UpNorth,
    UpSouth,
    UpWest,
    WestUp,
    EastUp,
    NorthUp,
    SouthUp,
}

impl PropertyEnum for FrontAndTop {
    fn as_str(&self) -> &str {
        match self {
            FrontAndTop::DownEast => "down_east",
            FrontAndTop::DownNorth => "down_north",
            FrontAndTop::DownSouth => "down_south",
            FrontAndTop::DownWest => "down_west",
            FrontAndTop::UpEast => "up_east",
            FrontAndTop::UpNorth => "up_north",
            FrontAndTop::UpSouth => "up_south",
            FrontAndTop::UpWest => "up_west",
            FrontAndTop::WestUp => "west_up",
            FrontAndTop::EastUp => "east_up",
            FrontAndTop::NorthUp => "north_up",
            FrontAndTop::SouthUp => "south_up",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum AttachFace {
    Floor,
    Wall,
    Ceiling,
}

impl PropertyEnum for AttachFace {
    fn as_str(&self) -> &str {
        match self {
            AttachFace::Floor => "floor",
            AttachFace::Wall => "wall",
            AttachFace::Ceiling => "ceiling",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum BellAttachType {
    Floor,
    Ceiling,
    SingleWall,
    DoubleWall,
}

impl PropertyEnum for BellAttachType {
    fn as_str(&self) -> &str {
        match self {
            BellAttachType::Floor => "floor",
            BellAttachType::Ceiling => "ceiling",
            BellAttachType::SingleWall => "single_wall",
            BellAttachType::DoubleWall => "double_wall",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum WallSide {
    None,
    Low,
    Tall,
}

impl PropertyEnum for WallSide {
    fn as_str(&self) -> &str {
        match self {
            WallSide::None => "none",
            WallSide::Low => "low",
            WallSide::Tall => "tall",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum RedstoneSide {
    Up,
    Side,
    None,
}

impl PropertyEnum for RedstoneSide {
    fn as_str(&self) -> &str {
        match self {
            RedstoneSide::None => "none",
            RedstoneSide::Side => "side",
            RedstoneSide::Up => "up",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum DoubleBlockHalf {
    Upper,
    Lower,
}

impl PropertyEnum for DoubleBlockHalf {
    fn as_str(&self) -> &str {
        match self {
            DoubleBlockHalf::Upper => "upper",
            DoubleBlockHalf::Lower => "lower",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum Half {
    Top,
    Bottom,
}

impl PropertyEnum for Half {
    fn as_str(&self) -> &str {
        match self {
            Half::Top => "top",
            Half::Bottom => "bottom",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum SideChainPart {
    Unconnected,
    Right,
    Center,
    Left,
}

impl PropertyEnum for SideChainPart {
    fn as_str(&self) -> &str {
        match self {
            SideChainPart::Unconnected => "unconnected",
            SideChainPart::Right => "right",
            SideChainPart::Center => "center",
            SideChainPart::Left => "left",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum RailShape {
    NorthSouth,
    EastWest,
    AscendingEast,
    AscendingWest,
    AscendingNorth,
    AscendingSouth,
    SouthEast,
    SouthWest,
    NorthWest,
    NorthEast,
}

impl PropertyEnum for RailShape {
    fn as_str(&self) -> &str {
        match self {
            RailShape::NorthSouth => "north_south",
            RailShape::EastWest => "east_west",
            RailShape::AscendingEast => "ascending_east",
            RailShape::AscendingWest => "ascending_west",
            RailShape::AscendingNorth => "ascending_north",
            RailShape::AscendingSouth => "ascending_south",
            RailShape::SouthEast => "south_east",
            RailShape::SouthWest => "south_west",
            RailShape::NorthWest => "north_west",
            RailShape::NorthEast => "north_east",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum BedPart {
    Head,
    Foot,
}

impl PropertyEnum for BedPart {
    fn as_str(&self) -> &str {
        match self {
            BedPart::Head => "head",
            BedPart::Foot => "foot",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum ChestType {
    Single,
    Left,
    Right,
}

impl PropertyEnum for ChestType {
    fn as_str(&self) -> &str {
        match self {
            ChestType::Single => "single",
            ChestType::Left => "left",
            ChestType::Right => "right",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum ComparatorMode {
    Compare,
    Subtract,
}

impl PropertyEnum for ComparatorMode {
    fn as_str(&self) -> &str {
        match self {
            ComparatorMode::Compare => "compare",
            ComparatorMode::Subtract => "subtract",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum DoorHingeSide {
    Left,
    Right,
}

impl PropertyEnum for DoorHingeSide {
    fn as_str(&self) -> &str {
        match self {
            DoorHingeSide::Left => "left",
            DoorHingeSide::Right => "right",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum NoteBlockInstrument {
    Harp,
    Basedrum,
    Snare,
    Hat,
    Bass,
    Flute,
    Bell,
    Guitar,
    Chime,
    Xylophone,
    IronXylophone,
    CowBell,
    Didgeridoo,
    Bit,
    Banjo,
    Pling,
    Zombie,
    Skeleton,
    Creeper,
    Dragon,
    WitherSkeleton,
    Piglin,
    CustomHead,
}

impl PropertyEnum for NoteBlockInstrument {
    fn as_str(&self) -> &str {
        match self {
            NoteBlockInstrument::Harp => "harp",
            NoteBlockInstrument::Basedrum => "basedrum",
            NoteBlockInstrument::Snare => "snare",
            NoteBlockInstrument::Hat => "hat",
            NoteBlockInstrument::Bass => "bass",
            NoteBlockInstrument::Flute => "flute",
            NoteBlockInstrument::Bell => "bell",
            NoteBlockInstrument::Guitar => "guitar",
            NoteBlockInstrument::Chime => "chime",
            NoteBlockInstrument::Xylophone => "xylophone",
            NoteBlockInstrument::IronXylophone => "iron_xylophone",
            NoteBlockInstrument::CowBell => "cow_bell",
            NoteBlockInstrument::Didgeridoo => "didgeridoo",
            NoteBlockInstrument::Bit => "bit",
            NoteBlockInstrument::Banjo => "banjo",
            NoteBlockInstrument::Pling => "pling",
            NoteBlockInstrument::Zombie => "zombie",
            NoteBlockInstrument::Skeleton => "skeleton",
            NoteBlockInstrument::Creeper => "creeper",
            NoteBlockInstrument::Dragon => "dragon",
            NoteBlockInstrument::WitherSkeleton => "wither_skeleton",
            NoteBlockInstrument::Piglin => "piglin",
            NoteBlockInstrument::CustomHead => "custom_head",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum PistonType {
    Normal,
    Sticky,
}

impl PropertyEnum for PistonType {
    fn as_str(&self) -> &str {
        match self {
            PistonType::Normal => "normal",
            PistonType::Sticky => "sticky",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum SlabType {
    Bottom,
    Top,
    Double,
}

impl PropertyEnum for SlabType {
    fn as_str(&self) -> &str {
        match self {
            SlabType::Bottom => "bottom",
            SlabType::Top => "top",
            SlabType::Double => "double",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum StairsShape {
    Straight,
    InnerLeft,
    InnerRight,
    OuterLeft,
    OuterRight,
}

impl PropertyEnum for StairsShape {
    fn as_str(&self) -> &str {
        match self {
            StairsShape::Straight => "straight",
            StairsShape::InnerLeft => "inner_left",
            StairsShape::InnerRight => "inner_right",
            StairsShape::OuterLeft => "outer_left",
            StairsShape::OuterRight => "outer_right",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum StructureMode {
    Save,
    Load,
    Corner,
    Data,
}

impl PropertyEnum for StructureMode {
    fn as_str(&self) -> &str {
        match self {
            StructureMode::Save => "save",
            StructureMode::Load => "load",
            StructureMode::Corner => "corner",
            StructureMode::Data => "data",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum BambooLeaves {
    None,
    Small,
    Large,
}

impl PropertyEnum for BambooLeaves {
    fn as_str(&self) -> &str {
        match self {
            BambooLeaves::None => "none",
            BambooLeaves::Small => "small",
            BambooLeaves::Large => "large",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum Tilt {
    None,
    Unstable,
    Partial,
    Full,
}

impl PropertyEnum for Tilt {
    fn as_str(&self) -> &str {
        match self {
            Tilt::None => "none",
            Tilt::Unstable => "unstable",
            Tilt::Partial => "partial",
            Tilt::Full => "full",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum DripstoneThickness {
    TipMerge,
    Tip,
    Frustum,
    Middle,
    Base,
}

impl PropertyEnum for DripstoneThickness {
    fn as_str(&self) -> &str {
        match self {
            DripstoneThickness::TipMerge => "tip_merge",
            DripstoneThickness::Tip => "tip",
            DripstoneThickness::Frustum => "frustum",
            DripstoneThickness::Middle => "middle",
            DripstoneThickness::Base => "base",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum SculkSensorPhase {
    Inactive,
    Active,
    Cooldown,
}

impl PropertyEnum for SculkSensorPhase {
    fn as_str(&self) -> &str {
        match self {
            SculkSensorPhase::Inactive => "inactive",
            SculkSensorPhase::Active => "active",
            SculkSensorPhase::Cooldown => "cooldown",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum TrialSpawnerState {
    Inactive,
    WaitingForPlayers,
    Active,
    WaitingForRewardEjection,
    EjectingReward,
    Cooldown,
}

impl PropertyEnum for TrialSpawnerState {
    fn as_str(&self) -> &str {
        match self {
            TrialSpawnerState::Inactive => "inactive",
            TrialSpawnerState::WaitingForPlayers => "waiting_for_players",
            TrialSpawnerState::Active => "active",
            TrialSpawnerState::WaitingForRewardEjection => "waiting_for_reward_ejection",
            TrialSpawnerState::EjectingReward => "ejecting_reward",
            TrialSpawnerState::Cooldown => "cooldown",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum VaultState {
    Inactive,
    Active,
    Unlocking,
    Ejecting,
}

impl PropertyEnum for VaultState {
    fn as_str(&self) -> &str {
        match self {
            VaultState::Inactive => "inactive",
            VaultState::Active => "active",
            VaultState::Unlocking => "unlocking",
            VaultState::Ejecting => "ejecting",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum CreakingHeartState {
    Uprooted,
    Dormant,
    Awake,
}

impl PropertyEnum for CreakingHeartState {
    fn as_str(&self) -> &str {
        match self {
            CreakingHeartState::Uprooted => "uprooted",
            CreakingHeartState::Dormant => "dormant",
            CreakingHeartState::Awake => "awake",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum TestBlockMode {
    Start,
    Log,
    Fail,
    Accept,
}

impl PropertyEnum for TestBlockMode {
    fn as_str(&self) -> &str {
        match self {
            TestBlockMode::Start => "start",
            TestBlockMode::Log => "log",
            TestBlockMode::Fail => "fail",
            TestBlockMode::Accept => "accept",
        }
    }
}

#[derive(Clone, Debug)]
#[derive_const(PartialEq)]
pub enum Pose {
    Standing,
    Sitting,
    Running,
    Star,
}

impl PropertyEnum for Pose {
    fn as_str(&self) -> &str {
        match self {
            Pose::Standing => "standing",
            Pose::Sitting => "sitting",
            Pose::Running => "running",
            Pose::Star => "star",
        }
    }
}

impl PropertyEnum for Axis {
    fn as_str(&self) -> &str {
        self.as_str()
    }
}

pub struct BlockStateProperties;

//TODO: These got quickly implemented so the ordering might be off. Fix in the future.
impl BlockStateProperties {
    pub const ATTACHED: BoolProperty = BoolProperty::new("attached");
    pub const BERRIES: BoolProperty = BoolProperty::new("berries");
    pub const BLOOM: BoolProperty = BoolProperty::new("bloom");
    pub const BOTTOM: BoolProperty = BoolProperty::new("bottom");
    pub const CAN_SUMMON: BoolProperty = BoolProperty::new("can_summon");
    pub const CONDITIONAL: BoolProperty = BoolProperty::new("conditional");
    pub const DISARMED: BoolProperty = BoolProperty::new("disarmed");
    pub const DRAG: BoolProperty = BoolProperty::new("drag");
    pub const ENABLED: BoolProperty = BoolProperty::new("enabled");
    pub const EXTENDED: BoolProperty = BoolProperty::new("extended");
    pub const EYE: BoolProperty = BoolProperty::new("eye");
    pub const FALLING: BoolProperty = BoolProperty::new("falling");
    pub const HANGING: BoolProperty = BoolProperty::new("hanging");
    pub const HAS_BOTTLE_0: BoolProperty = BoolProperty::new("has_bottle_0");
    pub const HAS_BOTTLE_1: BoolProperty = BoolProperty::new("has_bottle_1");
    pub const HAS_BOTTLE_2: BoolProperty = BoolProperty::new("has_bottle_2");
    pub const HAS_RECORD: BoolProperty = BoolProperty::new("has_record");
    pub const HAS_BOOK: BoolProperty = BoolProperty::new("has_book");
    pub const INVERTED: BoolProperty = BoolProperty::new("inverted");
    pub const IN_WALL: BoolProperty = BoolProperty::new("in_wall");
    pub const LIT: BoolProperty = BoolProperty::new("lit");
    pub const LOCKED: BoolProperty = BoolProperty::new("locked");
    pub const NATURAL: BoolProperty = BoolProperty::new("natural");
    pub const OCCUPIED: BoolProperty = BoolProperty::new("occupied");
    pub const OPEN: BoolProperty = BoolProperty::new("open");
    pub const PERSISTENT: BoolProperty = BoolProperty::new("persistent");
    pub const POWERED: BoolProperty = BoolProperty::new("powered");
    pub const SHORT: BoolProperty = BoolProperty::new("short");
    pub const SHRIEKING: BoolProperty = BoolProperty::new("shrieking");
    pub const SIGNAL_FIRE: BoolProperty = BoolProperty::new("signal_fire");
    pub const SNOWY: BoolProperty = BoolProperty::new("snowy");
    pub const TIP: BoolProperty = BoolProperty::new("tip");
    pub const TRIGGERED: BoolProperty = BoolProperty::new("triggered");
    pub const UNSTABLE: BoolProperty = BoolProperty::new("unstable");
    pub const WATERLOGGED: BoolProperty = BoolProperty::new("waterlogged");
    pub const HORIZONTAL_AXIS: EnumProperty<Axis> = EnumProperty::new("axis", &[Axis::X, Axis::Z]);
    pub const AXIS: EnumProperty<Axis> = EnumProperty::new("axis", &[Axis::X, Axis::Y, Axis::Z]);
    pub const UP: BoolProperty = BoolProperty::new("up");
    pub const DOWN: BoolProperty = BoolProperty::new("down");
    pub const NORTH: BoolProperty = BoolProperty::new("north");
    pub const EAST: BoolProperty = BoolProperty::new("east");
    pub const SOUTH: BoolProperty = BoolProperty::new("south");
    pub const WEST: BoolProperty = BoolProperty::new("west");
    pub const FACING: EnumProperty<Direction> = EnumProperty::new(
        "facing",
        &[
            Direction::North,
            Direction::East,
            Direction::South,
            Direction::West,
            Direction::Up,
            Direction::Down,
        ],
    );
    pub const FACING_HOPPER: EnumProperty<Direction> = EnumProperty::new(
        "facing",
        &[
            Direction::Down,
            Direction::North,
            Direction::South,
            Direction::West,
            Direction::East,
        ],
    );
    pub const HORIZONTAL_FACING: EnumProperty<Direction> = EnumProperty::new(
        "facing",
        &[
            Direction::North,
            Direction::South,
            Direction::West,
            Direction::East,
        ],
    );
    pub const FLOWER_AMOUNT: IntProperty = IntProperty::new("flower_amount", 1, 4);
    pub const SEGMENT_AMOUNT: IntProperty = IntProperty::new("segment_amount", 1, 4);

    // Additional enum types needed for properties
    pub const ORIENTATION: EnumProperty<FrontAndTop> = EnumProperty::new(
        "orientation",
        &[
            FrontAndTop::DownEast,
            FrontAndTop::DownNorth,
            FrontAndTop::DownSouth,
            FrontAndTop::DownWest,
            FrontAndTop::UpEast,
            FrontAndTop::UpNorth,
            FrontAndTop::UpSouth,
            FrontAndTop::UpWest,
            FrontAndTop::WestUp,
            FrontAndTop::EastUp,
            FrontAndTop::NorthUp,
            FrontAndTop::SouthUp,
        ],
    );
    pub const ATTACH_FACE: EnumProperty<AttachFace> = EnumProperty::new(
        "face",
        &[AttachFace::Floor, AttachFace::Wall, AttachFace::Ceiling],
    );
    pub const BELL_ATTACHMENT: EnumProperty<BellAttachType> = EnumProperty::new(
        "attachment",
        &[
            BellAttachType::Floor,
            BellAttachType::Ceiling,
            BellAttachType::SingleWall,
            BellAttachType::DoubleWall,
        ],
    );
    pub const EAST_WALL: EnumProperty<WallSide> =
        EnumProperty::new("east", &[WallSide::None, WallSide::Low, WallSide::Tall]);
    pub const NORTH_WALL: EnumProperty<WallSide> =
        EnumProperty::new("north", &[WallSide::None, WallSide::Low, WallSide::Tall]);
    pub const SOUTH_WALL: EnumProperty<WallSide> =
        EnumProperty::new("south", &[WallSide::None, WallSide::Low, WallSide::Tall]);
    pub const WEST_WALL: EnumProperty<WallSide> =
        EnumProperty::new("west", &[WallSide::None, WallSide::Low, WallSide::Tall]);
    pub const EAST_REDSTONE: EnumProperty<RedstoneSide> = EnumProperty::new(
        "east",
        &[RedstoneSide::Up, RedstoneSide::Side, RedstoneSide::None],
    );
    pub const NORTH_REDSTONE: EnumProperty<RedstoneSide> = EnumProperty::new(
        "north",
        &[RedstoneSide::Up, RedstoneSide::Side, RedstoneSide::None],
    );
    pub const SOUTH_REDSTONE: EnumProperty<RedstoneSide> = EnumProperty::new(
        "south",
        &[RedstoneSide::Up, RedstoneSide::Side, RedstoneSide::None],
    );
    pub const WEST_REDSTONE: EnumProperty<RedstoneSide> = EnumProperty::new(
        "west",
        &[RedstoneSide::Up, RedstoneSide::Side, RedstoneSide::None],
    );
    pub const DOUBLE_BLOCK_HALF: EnumProperty<DoubleBlockHalf> =
        EnumProperty::new("half", &[DoubleBlockHalf::Upper, DoubleBlockHalf::Lower]);
    pub const HALF: EnumProperty<Half> = EnumProperty::new("half", &[Half::Top, Half::Bottom]);
    pub const SIDE_CHAIN_PART: EnumProperty<SideChainPart> = EnumProperty::new(
        "side_chain",
        &[
            SideChainPart::Unconnected,
            SideChainPart::Right,
            SideChainPart::Center,
            SideChainPart::Left,
        ],
    );
    pub const RAIL_SHAPE: EnumProperty<RailShape> = EnumProperty::new(
        "shape",
        &[
            RailShape::NorthSouth,
            RailShape::EastWest,
            RailShape::AscendingEast,
            RailShape::AscendingWest,
            RailShape::AscendingNorth,
            RailShape::AscendingSouth,
            RailShape::SouthEast,
            RailShape::SouthWest,
            RailShape::NorthWest,
            RailShape::NorthEast,
        ],
    );
    pub const RAIL_SHAPE_STRAIGHT: EnumProperty<RailShape> = EnumProperty::new(
        "shape",
        &[
            RailShape::NorthSouth,
            RailShape::EastWest,
            RailShape::AscendingEast,
            RailShape::AscendingWest,
            RailShape::AscendingNorth,
            RailShape::AscendingSouth,
        ],
    );

    // Age properties
    pub const AGE_1: IntProperty = IntProperty::new("age", 0, 1);
    pub const AGE_2: IntProperty = IntProperty::new("age", 0, 2);
    pub const AGE_3: IntProperty = IntProperty::new("age", 0, 3);
    pub const AGE_4: IntProperty = IntProperty::new("age", 0, 4);
    pub const AGE_5: IntProperty = IntProperty::new("age", 0, 5);
    pub const AGE_7: IntProperty = IntProperty::new("age", 0, 7);
    pub const AGE_15: IntProperty = IntProperty::new("age", 0, 15);
    pub const AGE_25: IntProperty = IntProperty::new("age", 0, 25);

    // Other integer properties
    pub const BITES: IntProperty = IntProperty::new("bites", 0, 6);
    pub const CANDLES: IntProperty = IntProperty::new("candles", 1, 4);
    pub const DELAY: IntProperty = IntProperty::new("delay", 1, 4);
    pub const DISTANCE: IntProperty = IntProperty::new("distance", 1, 7);
    pub const EGGS: IntProperty = IntProperty::new("eggs", 1, 4);
    pub const HATCH: IntProperty = IntProperty::new("hatch", 0, 2);
    pub const LAYERS: IntProperty = IntProperty::new("layers", 1, 8);
    pub const LEVEL_CAULDRON: IntProperty = IntProperty::new("level", 1, 3);
    pub const LEVEL_COMPOSTER: IntProperty = IntProperty::new("level", 0, 8);
    pub const LEVEL_FLOWING: IntProperty = IntProperty::new("level", 1, 8);
    pub const LEVEL_HONEY: IntProperty = IntProperty::new("honey_level", 0, 5);
    pub const LEVEL: IntProperty = IntProperty::new("level", 0, 15);
    pub const MOISTURE: IntProperty = IntProperty::new("moisture", 0, 7);
    pub const NOTE: IntProperty = IntProperty::new("note", 0, 24);
    pub const PICKLES: IntProperty = IntProperty::new("pickles", 1, 4);
    pub const POWER: IntProperty = IntProperty::new("power", 0, 15);
    pub const STAGE: IntProperty = IntProperty::new("stage", 0, 1);
    pub const STABILITY_DISTANCE: IntProperty = IntProperty::new("distance", 0, 7);
    pub const RESPAWN_ANCHOR_CHARGES: IntProperty = IntProperty::new("charges", 0, 4);
    pub const DRIED_GHAST_HYDRATION_LEVELS: IntProperty = IntProperty::new("hydration", 0, 3);
    pub const ROTATION_16: IntProperty = IntProperty::new("rotation", 0, 15);
    pub const DUSTED: IntProperty = IntProperty::new("dusted", 0, 3);

    // Enum properties
    pub const BED_PART: EnumProperty<BedPart> =
        EnumProperty::new("part", &[BedPart::Head, BedPart::Foot]);
    pub const CHEST_TYPE: EnumProperty<ChestType> = EnumProperty::new(
        "type",
        &[ChestType::Single, ChestType::Left, ChestType::Right],
    );
    pub const MODE_COMPARATOR: EnumProperty<ComparatorMode> =
        EnumProperty::new("mode", &[ComparatorMode::Compare, ComparatorMode::Subtract]);
    pub const DOOR_HINGE: EnumProperty<DoorHingeSide> =
        EnumProperty::new("hinge", &[DoorHingeSide::Left, DoorHingeSide::Right]);
    pub const NOTEBLOCK_INSTRUMENT: EnumProperty<NoteBlockInstrument> = EnumProperty::new(
        "instrument",
        &[
            NoteBlockInstrument::Harp,
            NoteBlockInstrument::Basedrum,
            NoteBlockInstrument::Snare,
            NoteBlockInstrument::Hat,
            NoteBlockInstrument::Bass,
            NoteBlockInstrument::Flute,
            NoteBlockInstrument::Bell,
            NoteBlockInstrument::Guitar,
            NoteBlockInstrument::Chime,
            NoteBlockInstrument::Xylophone,
            NoteBlockInstrument::IronXylophone,
            NoteBlockInstrument::CowBell,
            NoteBlockInstrument::Didgeridoo,
            NoteBlockInstrument::Bit,
            NoteBlockInstrument::Banjo,
            NoteBlockInstrument::Pling,
            NoteBlockInstrument::Zombie,
            NoteBlockInstrument::Skeleton,
            NoteBlockInstrument::Creeper,
            NoteBlockInstrument::Dragon,
            NoteBlockInstrument::WitherSkeleton,
            NoteBlockInstrument::Piglin,
            NoteBlockInstrument::CustomHead,
        ],
    );
    pub const PISTON_TYPE: EnumProperty<PistonType> =
        EnumProperty::new("type", &[PistonType::Normal, PistonType::Sticky]);
    pub const SLAB_TYPE: EnumProperty<SlabType> =
        EnumProperty::new("type", &[SlabType::Top, SlabType::Bottom, SlabType::Double]);
    pub const STAIRS_SHAPE: EnumProperty<StairsShape> = EnumProperty::new(
        "shape",
        &[
            StairsShape::Straight,
            StairsShape::InnerLeft,
            StairsShape::InnerRight,
            StairsShape::OuterLeft,
            StairsShape::OuterRight,
        ],
    );
    pub const STRUCTUREBLOCK_MODE: EnumProperty<StructureMode> = EnumProperty::new(
        "mode",
        &[
            StructureMode::Save,
            StructureMode::Load,
            StructureMode::Corner,
            StructureMode::Data,
        ],
    );
    pub const BAMBOO_LEAVES: EnumProperty<BambooLeaves> = EnumProperty::new(
        "leaves",
        &[BambooLeaves::None, BambooLeaves::Small, BambooLeaves::Large],
    );
    pub const TILT: EnumProperty<Tilt> = EnumProperty::new(
        "tilt",
        &[Tilt::None, Tilt::Unstable, Tilt::Partial, Tilt::Full],
    );
    pub const VERTICAL_DIRECTION: EnumProperty<Direction> =
        EnumProperty::new("vertical_direction", &[Direction::Up, Direction::Down]);
    pub const DRIPSTONE_THICKNESS: EnumProperty<DripstoneThickness> = EnumProperty::new(
        "thickness",
        &[
            DripstoneThickness::TipMerge,
            DripstoneThickness::Tip,
            DripstoneThickness::Frustum,
            DripstoneThickness::Middle,
            DripstoneThickness::Base,
        ],
    );
    pub const SCULK_SENSOR_PHASE: EnumProperty<SculkSensorPhase> = EnumProperty::new(
        "sculk_sensor_phase",
        &[
            SculkSensorPhase::Inactive,
            SculkSensorPhase::Active,
            SculkSensorPhase::Cooldown,
        ],
    );
    pub const TRIAL_SPAWNER_STATE: EnumProperty<TrialSpawnerState> = EnumProperty::new(
        "trial_spawner_state",
        &[
            TrialSpawnerState::Inactive,
            TrialSpawnerState::WaitingForPlayers,
            TrialSpawnerState::Active,
            TrialSpawnerState::WaitingForRewardEjection,
            TrialSpawnerState::EjectingReward,
            TrialSpawnerState::Cooldown,
        ],
    );
    pub const VAULT_STATE: EnumProperty<VaultState> = EnumProperty::new(
        "vault_state",
        &[
            VaultState::Inactive,
            VaultState::Active,
            VaultState::Unlocking,
            VaultState::Ejecting,
        ],
    );
    pub const CREAKING_HEART_STATE: EnumProperty<CreakingHeartState> = EnumProperty::new(
        "creaking_heart_state",
        &[
            CreakingHeartState::Uprooted,
            CreakingHeartState::Dormant,
            CreakingHeartState::Awake,
        ],
    );
    pub const TEST_BLOCK_MODE: EnumProperty<TestBlockMode> = EnumProperty::new(
        "mode",
        &[
            TestBlockMode::Start,
            TestBlockMode::Log,
            TestBlockMode::Fail,
            TestBlockMode::Accept,
        ],
    );
    pub const COPPER_GOLEM_POSE: EnumProperty<Pose> = EnumProperty::new(
        "copper_golem_pose",
        &[Pose::Standing, Pose::Sitting, Pose::Running, Pose::Star],
    );

    // Additional boolean properties
    pub const SLOT_0_OCCUPIED: BoolProperty = BoolProperty::new("slot_0_occupied");
    pub const SLOT_1_OCCUPIED: BoolProperty = BoolProperty::new("slot_1_occupied");
    pub const SLOT_2_OCCUPIED: BoolProperty = BoolProperty::new("slot_2_occupied");
    pub const SLOT_3_OCCUPIED: BoolProperty = BoolProperty::new("slot_3_occupied");
    pub const SLOT_4_OCCUPIED: BoolProperty = BoolProperty::new("slot_4_occupied");
    pub const SLOT_5_OCCUPIED: BoolProperty = BoolProperty::new("slot_5_occupied");
    pub const CRACKED: BoolProperty = BoolProperty::new("cracked");
    pub const CRAFTING: BoolProperty = BoolProperty::new("crafting");
    pub const OMINOUS: BoolProperty = BoolProperty::new("ominous");
    pub const MAP: BoolProperty = BoolProperty::new("map");
}
