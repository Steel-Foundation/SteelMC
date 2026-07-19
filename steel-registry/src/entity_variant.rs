//! Closed Vanilla entity variants shared by item components and entities.

use std::io::{Cursor, Result, Write};

use simdnbt::owned::NbtTag;
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::serial::{ReadFrom, WriteTo};

macro_rules! impl_variant_codecs {
    ($type:ty) => {
        impl WriteTo for $type {
            fn write(&self, writer: &mut impl Write) -> Result<()> {
                VarInt(self.id()).write(writer)
            }
        }

        impl ReadFrom for $type {
            fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
                Ok(Self::by_id(VarInt::read(data)?.0))
            }
        }

        impl ToNbtTag for $type {
            fn to_nbt_tag(self) -> NbtTag {
                self.serialized_name().to_nbt_tag()
            }
        }

        impl FromNbtTag for $type {
            fn from_nbt_tag(tag: simdnbt::borrow::NbtTag<'_, '_>) -> Option<Self> {
                Self::from_serialized_name(&tag.string()?.to_str())
            }
        }

        impl HashComponent for $type {
            fn hash_component(&self, hasher: &mut ComponentHasher) {
                hasher.put_string(self.serialized_name());
            }
        }
    };
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FoxVariant {
    #[default]
    Red,
    Snow,
}

impl FoxVariant {
    pub const VALUES: [Self; 2] = [Self::Red, Self::Snow];

    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            Self::Red => 0,
            Self::Snow => 1,
        }
    }

    #[must_use]
    pub const fn by_id(id: i32) -> Self {
        match id {
            1 => Self::Snow,
            _ => Self::Red,
        }
    }

    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Snow => "snow",
        }
    }

    #[must_use]
    pub const fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "red" => Some(Self::Red),
            "snow" => Some(Self::Snow),
            _ => None,
        }
    }
}

impl_variant_codecs!(FoxVariant);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SalmonVariant {
    Small,
    #[default]
    Medium,
    Large,
}

impl SalmonVariant {
    pub const VALUES: [Self; 3] = [Self::Small, Self::Medium, Self::Large];

    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            Self::Small => 0,
            Self::Medium => 1,
            Self::Large => 2,
        }
    }

    #[must_use]
    pub const fn by_id(id: i32) -> Self {
        match id {
            i32::MIN..=0 => Self::Small,
            1 => Self::Medium,
            _ => Self::Large,
        }
    }

    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    #[must_use]
    pub const fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "small" => Some(Self::Small),
            "medium" => Some(Self::Medium),
            "large" => Some(Self::Large),
            _ => None,
        }
    }

    #[must_use]
    pub const fn bounding_box_scale(self) -> f32 {
        match self {
            Self::Small => 0.5,
            Self::Medium => 1.0,
            Self::Large => 1.5,
        }
    }
}

impl_variant_codecs!(SalmonVariant);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParrotVariant {
    #[default]
    RedBlue,
    Blue,
    Green,
    YellowBlue,
    Gray,
}

impl ParrotVariant {
    pub const VALUES: [Self; 5] = [
        Self::RedBlue,
        Self::Blue,
        Self::Green,
        Self::YellowBlue,
        Self::Gray,
    ];

    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            Self::RedBlue => 0,
            Self::Blue => 1,
            Self::Green => 2,
            Self::YellowBlue => 3,
            Self::Gray => 4,
        }
    }

    #[must_use]
    pub const fn by_id(id: i32) -> Self {
        match id {
            i32::MIN..=0 => Self::RedBlue,
            1 => Self::Blue,
            2 => Self::Green,
            3 => Self::YellowBlue,
            _ => Self::Gray,
        }
    }

    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::RedBlue => "red_blue",
            Self::Blue => "blue",
            Self::Green => "green",
            Self::YellowBlue => "yellow_blue",
            Self::Gray => "gray",
        }
    }

    #[must_use]
    pub const fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "red_blue" => Some(Self::RedBlue),
            "blue" => Some(Self::Blue),
            "green" => Some(Self::Green),
            "yellow_blue" => Some(Self::YellowBlue),
            "gray" => Some(Self::Gray),
            _ => None,
        }
    }
}

impl_variant_codecs!(ParrotVariant);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TropicalFishBase {
    Small,
    Large,
}

impl TropicalFishBase {
    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            Self::Small => 0,
            Self::Large => 1,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TropicalFishPattern {
    #[default]
    Kob,
    Sunstreak,
    Snooper,
    Dasher,
    Brinely,
    Spotty,
    Flopper,
    Stripey,
    Glitter,
    Blockfish,
    Betty,
    Clayfish,
}

impl TropicalFishPattern {
    pub const VALUES: [Self; 12] = [
        Self::Kob,
        Self::Sunstreak,
        Self::Snooper,
        Self::Dasher,
        Self::Brinely,
        Self::Spotty,
        Self::Flopper,
        Self::Stripey,
        Self::Glitter,
        Self::Blockfish,
        Self::Betty,
        Self::Clayfish,
    ];

    #[must_use]
    pub const fn base(self) -> TropicalFishBase {
        match self {
            Self::Kob
            | Self::Sunstreak
            | Self::Snooper
            | Self::Dasher
            | Self::Brinely
            | Self::Spotty => TropicalFishBase::Small,
            Self::Flopper
            | Self::Stripey
            | Self::Glitter
            | Self::Blockfish
            | Self::Betty
            | Self::Clayfish => TropicalFishBase::Large,
        }
    }

    #[must_use]
    pub const fn id(self) -> i32 {
        let index = match self {
            Self::Kob | Self::Flopper => 0,
            Self::Sunstreak | Self::Stripey => 1,
            Self::Snooper | Self::Glitter => 2,
            Self::Dasher | Self::Blockfish => 3,
            Self::Brinely | Self::Betty => 4,
            Self::Spotty | Self::Clayfish => 5,
        };
        self.base().id() | index << 8
    }

    #[must_use]
    pub const fn by_id(id: i32) -> Self {
        match id {
            0 => Self::Kob,
            256 => Self::Sunstreak,
            512 => Self::Snooper,
            768 => Self::Dasher,
            1024 => Self::Brinely,
            1280 => Self::Spotty,
            1 => Self::Flopper,
            257 => Self::Stripey,
            513 => Self::Glitter,
            769 => Self::Blockfish,
            1025 => Self::Betty,
            1281 => Self::Clayfish,
            _ => Self::Kob,
        }
    }

    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Kob => "kob",
            Self::Sunstreak => "sunstreak",
            Self::Snooper => "snooper",
            Self::Dasher => "dasher",
            Self::Brinely => "brinely",
            Self::Spotty => "spotty",
            Self::Flopper => "flopper",
            Self::Stripey => "stripey",
            Self::Glitter => "glitter",
            Self::Blockfish => "blockfish",
            Self::Betty => "betty",
            Self::Clayfish => "clayfish",
        }
    }

    #[must_use]
    pub const fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "kob" => Some(Self::Kob),
            "sunstreak" => Some(Self::Sunstreak),
            "snooper" => Some(Self::Snooper),
            "dasher" => Some(Self::Dasher),
            "brinely" => Some(Self::Brinely),
            "spotty" => Some(Self::Spotty),
            "flopper" => Some(Self::Flopper),
            "stripey" => Some(Self::Stripey),
            "glitter" => Some(Self::Glitter),
            "blockfish" => Some(Self::Blockfish),
            "betty" => Some(Self::Betty),
            "clayfish" => Some(Self::Clayfish),
            _ => None,
        }
    }
}

impl_variant_codecs!(TropicalFishPattern);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MooshroomVariant {
    #[default]
    Red,
    Brown,
}

impl MooshroomVariant {
    pub const VALUES: [Self; 2] = [Self::Red, Self::Brown];

    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            Self::Red => 0,
            Self::Brown => 1,
        }
    }

    #[must_use]
    pub const fn by_id(id: i32) -> Self {
        if id <= 0 { Self::Red } else { Self::Brown }
    }

    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Brown => "brown",
        }
    }

    #[must_use]
    pub const fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "red" => Some(Self::Red),
            "brown" => Some(Self::Brown),
            _ => None,
        }
    }
}

impl_variant_codecs!(MooshroomVariant);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RabbitVariant {
    #[default]
    Brown,
    White,
    Black,
    WhiteSplotched,
    Gold,
    Salt,
    Evil,
}

impl RabbitVariant {
    pub const VALUES: [Self; 7] = [
        Self::Brown,
        Self::White,
        Self::Black,
        Self::WhiteSplotched,
        Self::Gold,
        Self::Salt,
        Self::Evil,
    ];

    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            Self::Brown => 0,
            Self::White => 1,
            Self::Black => 2,
            Self::WhiteSplotched => 3,
            Self::Gold => 4,
            Self::Salt => 5,
            Self::Evil => 99,
        }
    }

    #[must_use]
    pub const fn by_id(id: i32) -> Self {
        match id {
            1 => Self::White,
            2 => Self::Black,
            3 => Self::WhiteSplotched,
            4 => Self::Gold,
            5 => Self::Salt,
            99 => Self::Evil,
            _ => Self::Brown,
        }
    }

    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Brown => "brown",
            Self::White => "white",
            Self::Black => "black",
            Self::WhiteSplotched => "white_splotched",
            Self::Gold => "gold",
            Self::Salt => "salt",
            Self::Evil => "evil",
        }
    }

    #[must_use]
    pub const fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "brown" => Some(Self::Brown),
            "white" => Some(Self::White),
            "black" => Some(Self::Black),
            "white_splotched" => Some(Self::WhiteSplotched),
            "gold" => Some(Self::Gold),
            "salt" => Some(Self::Salt),
            "evil" => Some(Self::Evil),
            _ => None,
        }
    }
}

impl_variant_codecs!(RabbitVariant);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HorseVariant {
    #[default]
    White,
    Creamy,
    Chestnut,
    Brown,
    Black,
    Gray,
    DarkBrown,
}

impl HorseVariant {
    pub const VALUES: [Self; 7] = [
        Self::White,
        Self::Creamy,
        Self::Chestnut,
        Self::Brown,
        Self::Black,
        Self::Gray,
        Self::DarkBrown,
    ];

    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            Self::White => 0,
            Self::Creamy => 1,
            Self::Chestnut => 2,
            Self::Brown => 3,
            Self::Black => 4,
            Self::Gray => 5,
            Self::DarkBrown => 6,
        }
    }

    #[must_use]
    pub const fn by_id(id: i32) -> Self {
        match id.rem_euclid(7) {
            0 => Self::White,
            1 => Self::Creamy,
            2 => Self::Chestnut,
            3 => Self::Brown,
            4 => Self::Black,
            5 => Self::Gray,
            _ => Self::DarkBrown,
        }
    }

    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::White => "white",
            Self::Creamy => "creamy",
            Self::Chestnut => "chestnut",
            Self::Brown => "brown",
            Self::Black => "black",
            Self::Gray => "gray",
            Self::DarkBrown => "dark_brown",
        }
    }

    #[must_use]
    pub const fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "white" => Some(Self::White),
            "creamy" => Some(Self::Creamy),
            "chestnut" => Some(Self::Chestnut),
            "brown" => Some(Self::Brown),
            "black" => Some(Self::Black),
            "gray" => Some(Self::Gray),
            "dark_brown" => Some(Self::DarkBrown),
            _ => None,
        }
    }
}

impl_variant_codecs!(HorseVariant);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LlamaVariant {
    #[default]
    Creamy,
    White,
    Brown,
    Gray,
}

impl LlamaVariant {
    pub const VALUES: [Self; 4] = [Self::Creamy, Self::White, Self::Brown, Self::Gray];

    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            Self::Creamy => 0,
            Self::White => 1,
            Self::Brown => 2,
            Self::Gray => 3,
        }
    }

    #[must_use]
    pub const fn by_id(id: i32) -> Self {
        match id {
            i32::MIN..=0 => Self::Creamy,
            1 => Self::White,
            2 => Self::Brown,
            _ => Self::Gray,
        }
    }

    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Creamy => "creamy",
            Self::White => "white",
            Self::Brown => "brown",
            Self::Gray => "gray",
        }
    }

    #[must_use]
    pub const fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "creamy" => Some(Self::Creamy),
            "white" => Some(Self::White),
            "brown" => Some(Self::Brown),
            "gray" => Some(Self::Gray),
            _ => None,
        }
    }
}

impl_variant_codecs!(LlamaVariant);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AxolotlVariant {
    #[default]
    Lucy,
    Wild,
    Gold,
    Cyan,
    Blue,
}

impl AxolotlVariant {
    pub const VALUES: [Self; 5] = [Self::Lucy, Self::Wild, Self::Gold, Self::Cyan, Self::Blue];

    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            Self::Lucy => 0,
            Self::Wild => 1,
            Self::Gold => 2,
            Self::Cyan => 3,
            Self::Blue => 4,
        }
    }

    #[must_use]
    pub const fn by_id(id: i32) -> Self {
        match id {
            1 => Self::Wild,
            2 => Self::Gold,
            3 => Self::Cyan,
            4 => Self::Blue,
            _ => Self::Lucy,
        }
    }

    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Lucy => "lucy",
            Self::Wild => "wild",
            Self::Gold => "gold",
            Self::Cyan => "cyan",
            Self::Blue => "blue",
        }
    }

    #[must_use]
    pub const fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "lucy" => Some(Self::Lucy),
            "wild" => Some(Self::Wild),
            "gold" => Some(Self::Gold),
            "cyan" => Some(Self::Cyan),
            "blue" => Some(Self::Blue),
            _ => None,
        }
    }

    #[must_use]
    pub const fn is_common(self) -> bool {
        !matches!(self, Self::Blue)
    }
}

impl_variant_codecs!(AxolotlVariant);

#[cfg(test)]
mod tests {
    use std::fmt::Debug;
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::owned::NbtTag;
    use steel_utils::codec::VarInt;
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::ReadFrom;

    use super::{
        AxolotlVariant, FoxVariant, HorseVariant, LlamaVariant, MooshroomVariant, ParrotVariant,
        RabbitVariant, SalmonVariant, TropicalFishBase, TropicalFishPattern,
    };

    fn assert_variant<T>(value: T, id: i32, name: &str)
    where
        T: Copy
            + Debug
            + PartialEq
            + ReadFrom
            + steel_utils::serial::WriteTo
            + simdnbt::FromNbtTag
            + simdnbt::ToNbtTag
            + steel_utils::hash::HashComponent,
    {
        let mut encoded = Vec::new();
        value.write(&mut encoded).expect("variant should encode");
        assert_eq!(
            VarInt::read(&mut Cursor::new(encoded.as_slice()))
                .expect("variant ID should decode")
                .0,
            id
        );
        assert_eq!(
            T::read(&mut Cursor::new(encoded.as_slice())).expect("variant should decode"),
            value
        );
        assert_eq!(value.to_nbt_tag(), NbtTag::String(name.into()));

        let mut bytes = Vec::new();
        NbtTag::String(name.into()).write(&mut bytes);
        let borrowed =
            read_tag(&mut Cursor::new(bytes.as_slice())).expect("owned variant tag should parse");
        assert_eq!(T::from_nbt_tag(borrowed.as_tag()), Some(value));
        assert_eq!(
            value.compute_hash(),
            NbtTag::String(name.into()).compute_hash()
        );
    }

    #[test]
    fn ids_names_persistent_codecs_and_hashes_match_vanilla() {
        for value in FoxVariant::VALUES {
            assert_variant(value, value.id(), value.serialized_name());
        }
        for value in SalmonVariant::VALUES {
            assert_variant(value, value.id(), value.serialized_name());
        }
        for value in ParrotVariant::VALUES {
            assert_variant(value, value.id(), value.serialized_name());
        }
        for value in TropicalFishPattern::VALUES {
            assert_variant(value, value.id(), value.serialized_name());
        }
        for value in MooshroomVariant::VALUES {
            assert_variant(value, value.id(), value.serialized_name());
        }
        for value in RabbitVariant::VALUES {
            assert_variant(value, value.id(), value.serialized_name());
        }
        for value in HorseVariant::VALUES {
            assert_variant(value, value.id(), value.serialized_name());
        }
        for value in LlamaVariant::VALUES {
            assert_variant(value, value.id(), value.serialized_name());
        }
        for value in AxolotlVariant::VALUES {
            assert_variant(value, value.id(), value.serialized_name());
        }
    }

    #[test]
    fn network_out_of_bounds_strategies_match_vanilla() {
        assert_eq!(FoxVariant::by_id(-1), FoxVariant::Red);
        assert_eq!(FoxVariant::by_id(2), FoxVariant::Red);
        assert_eq!(SalmonVariant::by_id(-1), SalmonVariant::Small);
        assert_eq!(SalmonVariant::by_id(3), SalmonVariant::Large);
        assert_eq!(ParrotVariant::by_id(-1), ParrotVariant::RedBlue);
        assert_eq!(ParrotVariant::by_id(5), ParrotVariant::Gray);
        assert_eq!(TropicalFishPattern::by_id(2), TropicalFishPattern::Kob);
        assert_eq!(MooshroomVariant::by_id(-1), MooshroomVariant::Red);
        assert_eq!(MooshroomVariant::by_id(2), MooshroomVariant::Brown);
        assert_eq!(RabbitVariant::by_id(6), RabbitVariant::Brown);
        assert_eq!(HorseVariant::by_id(-1), HorseVariant::DarkBrown);
        assert_eq!(HorseVariant::by_id(7), HorseVariant::White);
        assert_eq!(LlamaVariant::by_id(-1), LlamaVariant::Creamy);
        assert_eq!(LlamaVariant::by_id(4), LlamaVariant::Gray);
        assert_eq!(AxolotlVariant::by_id(-1), AxolotlVariant::Lucy);
        assert_eq!(AxolotlVariant::by_id(5), AxolotlVariant::Lucy);
    }

    #[test]
    fn variant_specific_metadata_matches_vanilla() {
        assert_eq!(SalmonVariant::Small.bounding_box_scale(), 0.5);
        assert_eq!(SalmonVariant::Large.bounding_box_scale(), 1.5);
        assert_eq!(TropicalFishPattern::Kob.base(), TropicalFishBase::Small);
        assert_eq!(TropicalFishPattern::Flopper.base(), TropicalFishBase::Large);
        assert_eq!(TropicalFishPattern::Spotty.id(), 1280);
        assert_eq!(TropicalFishPattern::Clayfish.id(), 1281);
        assert!(AxolotlVariant::Cyan.is_common());
        assert!(!AxolotlVariant::Blue.is_common());
    }
}
