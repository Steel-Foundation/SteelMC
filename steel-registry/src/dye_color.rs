//! Vanilla dye colors shared by item components, entities, and blocks.

use std::io::{Cursor, Result, Write};

use simdnbt::owned::NbtTag;
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::serial::{ReadFrom, WriteTo};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DyeColor {
    White,
    Orange,
    Magenta,
    LightBlue,
    Yellow,
    Lime,
    Pink,
    Gray,
    LightGray,
    Cyan,
    Purple,
    Blue,
    Brown,
    Green,
    Red,
    Black,
}

impl DyeColor {
    pub const VALUES: [Self; 16] = [
        Self::White,
        Self::Orange,
        Self::Magenta,
        Self::LightBlue,
        Self::Yellow,
        Self::Lime,
        Self::Pink,
        Self::Gray,
        Self::LightGray,
        Self::Cyan,
        Self::Purple,
        Self::Blue,
        Self::Brown,
        Self::Green,
        Self::Red,
        Self::Black,
    ];

    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            Self::White => 0,
            Self::Orange => 1,
            Self::Magenta => 2,
            Self::LightBlue => 3,
            Self::Yellow => 4,
            Self::Lime => 5,
            Self::Pink => 6,
            Self::Gray => 7,
            Self::LightGray => 8,
            Self::Cyan => 9,
            Self::Purple => 10,
            Self::Blue => 11,
            Self::Brown => 12,
            Self::Green => 13,
            Self::Red => 14,
            Self::Black => 15,
        }
    }

    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::White => "white",
            Self::Orange => "orange",
            Self::Magenta => "magenta",
            Self::LightBlue => "light_blue",
            Self::Yellow => "yellow",
            Self::Lime => "lime",
            Self::Pink => "pink",
            Self::Gray => "gray",
            Self::LightGray => "light_gray",
            Self::Cyan => "cyan",
            Self::Purple => "purple",
            Self::Blue => "blue",
            Self::Brown => "brown",
            Self::Green => "green",
            Self::Red => "red",
            Self::Black => "black",
        }
    }

    #[must_use]
    pub const fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "white" => Some(Self::White),
            "orange" => Some(Self::Orange),
            "magenta" => Some(Self::Magenta),
            "light_blue" => Some(Self::LightBlue),
            "yellow" => Some(Self::Yellow),
            "lime" => Some(Self::Lime),
            "pink" => Some(Self::Pink),
            "gray" => Some(Self::Gray),
            "light_gray" => Some(Self::LightGray),
            "cyan" => Some(Self::Cyan),
            "purple" => Some(Self::Purple),
            "blue" => Some(Self::Blue),
            "brown" => Some(Self::Brown),
            "green" => Some(Self::Green),
            "red" => Some(Self::Red),
            "black" => Some(Self::Black),
            _ => None,
        }
    }

    /// Mirrors Vanilla's zero fallback for out-of-range network IDs.
    #[must_use]
    pub const fn by_id(id: i32) -> Self {
        match id {
            1 => Self::Orange,
            2 => Self::Magenta,
            3 => Self::LightBlue,
            4 => Self::Yellow,
            5 => Self::Lime,
            6 => Self::Pink,
            7 => Self::Gray,
            8 => Self::LightGray,
            9 => Self::Cyan,
            10 => Self::Purple,
            11 => Self::Blue,
            12 => Self::Brown,
            13 => Self::Green,
            14 => Self::Red,
            15 => Self::Black,
            _ => Self::White,
        }
    }

    #[must_use]
    pub const fn texture_diffuse_color(self) -> i32 {
        opaque(match self {
            Self::White => 16_383_998,
            Self::Orange => 16_351_261,
            Self::Magenta => 13_061_821,
            Self::LightBlue => 3_847_130,
            Self::Yellow => 16_701_501,
            Self::Lime => 8_439_583,
            Self::Pink => 15_961_002,
            Self::Gray => 4_673_362,
            Self::LightGray => 10_329_495,
            Self::Cyan => 1_481_884,
            Self::Purple => 8_991_416,
            Self::Blue => 3_949_738,
            Self::Brown => 8_606_770,
            Self::Green => 6_192_150,
            Self::Red => 11_546_150,
            Self::Black => 1_908_001,
        })
    }

    #[must_use]
    pub const fn firework_color(self) -> i32 {
        match self {
            Self::White => 15_790_320,
            Self::Orange => 15_435_844,
            Self::Magenta => 12_801_229,
            Self::LightBlue => 6_719_955,
            Self::Yellow => 14_602_026,
            Self::Lime => 4_312_372,
            Self::Pink => 14_188_952,
            Self::Gray => 4_408_131,
            Self::LightGray => 11_250_603,
            Self::Cyan => 2_651_799,
            Self::Purple => 8_073_150,
            Self::Blue => 2_437_522,
            Self::Brown => 5_320_730,
            Self::Green => 3_887_386,
            Self::Red => 11_743_532,
            Self::Black => 1_973_019,
        }
    }

    #[must_use]
    pub const fn text_color(self) -> i32 {
        opaque(match self {
            Self::White => 16_777_215,
            Self::Orange => 16_738_335,
            Self::Magenta => 16_711_935,
            Self::LightBlue => 10_141_901,
            Self::Yellow => 16_776_960,
            Self::Lime => 12_582_656,
            Self::Pink => 16_738_740,
            Self::Gray => 8_421_504,
            Self::LightGray => 13_882_323,
            Self::Cyan => 65_535,
            Self::Purple => 10_494_192,
            Self::Blue => 255,
            Self::Brown => 9_127_187,
            Self::Green => 65_280,
            Self::Red => 16_711_680,
            Self::Black => 0,
        })
    }

    #[must_use]
    pub fn by_firework_color(color: i32) -> Option<Self> {
        Self::VALUES
            .into_iter()
            .find(|dye| dye.firework_color() == color)
    }
}

const fn opaque(rgb: i32) -> i32 {
    (0xff00_0000_u32 | (rgb as u32 & 0x00ff_ffff)) as i32
}

impl WriteTo for DyeColor {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.id()).write(writer)
    }
}

impl ReadFrom for DyeColor {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::by_id(VarInt::read(data)?.0))
    }
}

impl ToNbtTag for DyeColor {
    fn to_nbt_tag(self) -> NbtTag {
        self.serialized_name().to_nbt_tag()
    }
}

impl FromNbtTag for DyeColor {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag<'_, '_>) -> Option<Self> {
        Self::from_serialized_name(&tag.string()?.to_str())
    }
}

impl HashComponent for DyeColor {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_string(self.serialized_name());
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::owned::NbtTag;
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::codec::VarInt;
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::DyeColor;

    #[test]
    fn ids_names_and_network_fallback_match_vanilla() {
        for (id, color) in DyeColor::VALUES.into_iter().enumerate() {
            assert_eq!(color.id(), id as i32);
            assert_eq!(
                DyeColor::from_serialized_name(color.serialized_name()),
                Some(color)
            );

            let mut encoded = Vec::new();
            color.write(&mut encoded).expect("color should encode");
            assert_eq!(
                DyeColor::read(&mut Cursor::new(encoded.as_slice())).expect("color should decode"),
                color
            );
        }

        for id in [-1, 16, i32::MAX] {
            let mut encoded = Vec::new();
            VarInt(id).write(&mut encoded).expect("id should encode");
            assert_eq!(
                DyeColor::read(&mut Cursor::new(encoded.as_slice())).expect("id should decode"),
                DyeColor::White
            );
        }
    }

    #[test]
    fn vanilla_color_metadata_matches_representative_values() {
        assert_eq!(
            DyeColor::White.texture_diffuse_color(),
            0xfff9_fffe_u32 as i32
        );
        assert_eq!(DyeColor::Red.firework_color(), 11_743_532);
        assert_eq!(DyeColor::Black.text_color(), 0xff00_0000_u32 as i32);
        assert_eq!(
            DyeColor::by_firework_color(DyeColor::Cyan.firework_color()),
            Some(DyeColor::Cyan)
        );
    }

    #[test]
    fn persistent_codec_and_hash_use_serialized_names() {
        let value = DyeColor::LightBlue;
        assert_eq!(value.to_nbt_tag(), NbtTag::String("light_blue".into()));

        let mut bytes = Vec::new();
        NbtTag::String("light_blue".into()).write(&mut bytes);
        let borrowed =
            read_tag(&mut Cursor::new(bytes.as_slice())).expect("owned color tag should parse");
        assert_eq!(
            DyeColor::from_nbt_tag(borrowed.as_tag()),
            Some(DyeColor::LightBlue)
        );
        assert_eq!(
            value.compute_hash(),
            NbtTag::String("light_blue".into()).compute_hash()
        );
    }
}
