//! Vanilla item color and map ID components.

use std::io::{Cursor, Result, Write};

use simdnbt::owned::NbtTag;
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};

use super::rgb_color::decode_rgb_color;
use crate::DyeColor;

/// RGB color applied to dyeable items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DyedItemColor {
    rgb: i32,
}

impl DyedItemColor {
    pub const LEATHER_COLOR: i32 = -6_265_536;

    #[must_use]
    pub const fn new(rgb: i32) -> Self {
        Self { rgb }
    }

    #[must_use]
    pub const fn rgb(self) -> i32 {
        self.rgb
    }

    /// Applies Vanilla's intensity-preserving dye blend.
    #[must_use]
    pub fn apply_dyes(current: Option<Self>, dyes: &[DyeColor]) -> Self {
        let mut red_total = 0;
        let mut green_total = 0;
        let mut blue_total = 0;
        let mut intensity_total = 0;
        let mut color_count = 0;

        if let Some(current) = current {
            let (red, green, blue) = rgb_channels(current.rgb);
            intensity_total += red.max(green).max(blue);
            red_total += red;
            green_total += green;
            blue_total += blue;
            color_count += 1;
        }
        for dye in dyes {
            let (red, green, blue) = rgb_channels(dye.texture_diffuse_color());
            intensity_total += red.max(green).max(blue);
            red_total += red;
            green_total += green;
            blue_total += blue;
            color_count += 1;
        }

        let mut red = red_total / color_count;
        let mut green = green_total / color_count;
        let mut blue = blue_total / color_count;
        let average_intensity = intensity_total as f32 / color_count as f32;
        let result_intensity = red.max(green).max(blue) as f32;
        red = (red as f32 * average_intensity / result_intensity) as i32;
        green = (green as f32 * average_intensity / result_intensity) as i32;
        blue = (blue as f32 * average_intensity / result_intensity) as i32;
        Self::new((red << 16) | (green << 8) | blue)
    }
}

const fn rgb_channels(color: i32) -> (i32, i32, i32) {
    ((color >> 16) & 255, (color >> 8) & 255, color & 255)
}

impl WriteTo for DyedItemColor {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.rgb.write(writer)
    }
}

impl ReadFrom for DyedItemColor {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(i32::read(data)?))
    }
}

impl ToNbtTag for DyedItemColor {
    fn to_nbt_tag(self) -> NbtTag {
        NbtTag::Int(self.rgb)
    }
}

impl FromNbtTag for DyedItemColor {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag<'_, '_>) -> Option<Self> {
        decode_rgb_color(&tag.to_owned()).map(Self::new)
    }
}

impl HashComponent for DyedItemColor {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_int(self.rgb);
    }
}

/// Color used to tint a filled map item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapItemColor {
    rgb: i32,
}

impl MapItemColor {
    pub const DEFAULT: Self = Self::new(4_603_950);

    #[must_use]
    pub const fn new(rgb: i32) -> Self {
        Self { rgb }
    }

    #[must_use]
    pub const fn rgb(self) -> i32 {
        self.rgb
    }
}

impl WriteTo for MapItemColor {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.rgb.write(writer)
    }
}

impl ReadFrom for MapItemColor {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(i32::read(data)?))
    }
}

impl ToNbtTag for MapItemColor {
    fn to_nbt_tag(self) -> NbtTag {
        NbtTag::Int(self.rgb)
    }
}

impl FromNbtTag for MapItemColor {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag<'_, '_>) -> Option<Self> {
        tag.codec_i32().map(Self::new)
    }
}

impl HashComponent for MapItemColor {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_int(self.rgb);
    }
}

/// Numeric identifier for a map saved-data entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapId {
    id: i32,
}

impl MapId {
    #[must_use]
    pub const fn new(id: i32) -> Self {
        Self { id }
    }

    #[must_use]
    pub const fn id(self) -> i32 {
        self.id
    }

    #[must_use]
    pub fn key(self) -> String {
        format!("maps/{}", self.id)
    }
}

impl WriteTo for MapId {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.id).write(writer)
    }
}

impl ReadFrom for MapId {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(VarInt::read(data)?.0))
    }
}

impl ToNbtTag for MapId {
    fn to_nbt_tag(self) -> NbtTag {
        NbtTag::Int(self.id)
    }
}

impl FromNbtTag for MapId {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag<'_, '_>) -> Option<Self> {
        tag.codec_i32().map(Self::new)
    }
}

impl HashComponent for MapId {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_int(self.id);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::ToNbtTag as _;
    use simdnbt::borrow::read_tag;
    use simdnbt::owned::{NbtList, NbtTag};
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::{DyedItemColor, MapId, MapItemColor};

    fn parse<T: simdnbt::FromNbtTag>(tag: NbtTag) -> Option<T> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        T::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn dyed_color_accepts_ints_and_rgb_vectors() {
        assert_eq!(
            parse(NbtTag::Short(0x1234)),
            Some(DyedItemColor::new(0x1234))
        );
        assert_eq!(
            parse(NbtTag::List(NbtList::Float(vec![1.0, 0.5, 0.0]))),
            Some(DyedItemColor::new(0xffff_7f00_u32 as i32))
        );
        assert_eq!(
            parse::<DyedItemColor>(NbtTag::List(NbtList::Float(vec![1.0]))),
            None
        );
        assert_eq!(
            DyedItemColor::new(0x123456).to_nbt_tag(),
            NbtTag::Int(0x123456)
        );
    }

    #[test]
    fn raw_int_network_codecs_round_trip() {
        for value in [DyedItemColor::new(-1), DyedItemColor::new(0x123456)] {
            let mut encoded = Vec::new();
            value.write(&mut encoded).expect("color should encode");
            assert_eq!(
                DyedItemColor::read(&mut Cursor::new(encoded.as_slice()))
                    .expect("color should decode"),
                value
            );
        }

        let value = MapItemColor::DEFAULT;
        let mut encoded = Vec::new();
        value.write(&mut encoded).expect("map color should encode");
        assert_eq!(
            MapItemColor::read(&mut Cursor::new(encoded.as_slice()))
                .expect("map color should decode"),
            value
        );
    }

    #[test]
    fn map_id_uses_varint_network_and_int_persistence() {
        let value = MapId::new(-17);
        let mut encoded = Vec::new();
        value.write(&mut encoded).expect("map ID should encode");
        assert_eq!(
            MapId::read(&mut Cursor::new(encoded.as_slice())).expect("map ID should decode"),
            value
        );
        assert_eq!(parse(NbtTag::Long(42)), Some(MapId::new(42)));
        assert_eq!(value.to_nbt_tag(), NbtTag::Int(-17));
        assert_eq!(MapId::new(42).key(), "maps/42");
    }

    #[test]
    fn persistent_hashes_use_int_codec_shape() {
        for (actual, expected) in [
            (
                DyedItemColor::new(0x123456).compute_hash(),
                NbtTag::Int(0x123456).compute_hash(),
            ),
            (
                MapItemColor::DEFAULT.compute_hash(),
                NbtTag::Int(MapItemColor::DEFAULT.rgb()).compute_hash(),
            ),
            (MapId::new(7).compute_hash(), NbtTag::Int(7).compute_hash()),
        ] {
            assert_eq!(actual, expected);
        }
    }
}
