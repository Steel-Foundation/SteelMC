//! Vanilla firework explosion and rocket components.

use std::io::{Cursor, Error, Result, Write};

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};

/// Shape rendered by a firework explosion.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum FireworkExplosionShape {
    #[default]
    SmallBall,
    LargeBall,
    Star,
    Creeper,
    Burst,
}

impl FireworkExplosionShape {
    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            Self::SmallBall => 0,
            Self::LargeBall => 1,
            Self::Star => 2,
            Self::Creeper => 3,
            Self::Burst => 4,
        }
    }

    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::SmallBall => "small_ball",
            Self::LargeBall => "large_ball",
            Self::Star => "star",
            Self::Creeper => "creeper",
            Self::Burst => "burst",
        }
    }

    #[must_use]
    pub const fn by_id(id: i32) -> Self {
        match id {
            1 => Self::LargeBall,
            2 => Self::Star,
            3 => Self::Creeper,
            4 => Self::Burst,
            _ => Self::SmallBall,
        }
    }

    const fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "small_ball" => Some(Self::SmallBall),
            "large_ball" => Some(Self::LargeBall),
            "star" => Some(Self::Star),
            "creeper" => Some(Self::Creeper),
            "burst" => Some(Self::Burst),
            _ => None,
        }
    }
}

/// Visual data for one firework explosion.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FireworkExplosion {
    shape: FireworkExplosionShape,
    colors: Vec<i32>,
    fade_colors: Vec<i32>,
    has_trail: bool,
    has_twinkle: bool,
}

impl FireworkExplosion {
    #[must_use]
    pub const fn new(
        shape: FireworkExplosionShape,
        colors: Vec<i32>,
        fade_colors: Vec<i32>,
        has_trail: bool,
        has_twinkle: bool,
    ) -> Self {
        Self {
            shape,
            colors,
            fade_colors,
            has_trail,
            has_twinkle,
        }
    }

    #[must_use]
    pub const fn shape(&self) -> FireworkExplosionShape {
        self.shape
    }

    #[must_use]
    pub fn colors(&self) -> &[i32] {
        &self.colors
    }

    #[must_use]
    pub fn fade_colors(&self) -> &[i32] {
        &self.fade_colors
    }

    #[must_use]
    pub const fn has_trail(&self) -> bool {
        self.has_trail
    }

    #[must_use]
    pub const fn has_twinkle(&self) -> bool {
        self.has_twinkle
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("shape", self.shape.serialized_name());
        if !self.colors.is_empty() {
            compound.insert("colors", int_list_nbt(&self.colors));
        }
        if !self.fade_colors.is_empty() {
            compound.insert("fade_colors", int_list_nbt(&self.fade_colors));
        }
        if self.has_trail {
            compound.insert("has_trail", true);
        }
        if self.has_twinkle {
            compound.insert("has_twinkle", true);
        }
        NbtTag::Compound(compound)
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let shape = FireworkExplosionShape::from_serialized_name(
            &compound.get("shape")?.string()?.to_string(),
        )?;
        let colors = match compound.get("colors") {
            Some(tag) => int_list_from_nbt(tag)?,
            None => Vec::new(),
        };
        let fade_colors = match compound.get("fade_colors") {
            Some(tag) => int_list_from_nbt(tag)?,
            None => Vec::new(),
        };
        let has_trail = optional_bool(compound.get("has_trail"), false)?;
        let has_twinkle = optional_bool(compound.get("has_twinkle"), false)?;
        Some(Self::new(
            shape,
            colors,
            fade_colors,
            has_trail,
            has_twinkle,
        ))
    }
}

impl WriteTo for FireworkExplosion {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.shape.id()).write(writer)?;
        write_int_list(&self.colors, writer)?;
        write_int_list(&self.fade_colors, writer)?;
        self.has_trail.write(writer)?;
        self.has_twinkle.write(writer)
    }
}

impl ReadFrom for FireworkExplosion {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(
            FireworkExplosionShape::by_id(VarInt::read(data)?.0),
            read_int_list(data)?,
            read_int_list(data)?,
            bool::read(data)?,
            bool::read(data)?,
        ))
    }
}

impl ToNbtTag for FireworkExplosion {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for FireworkExplosion {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Self::from_owned_nbt(&tag.to_owned())
    }
}

impl HashComponent for FireworkExplosion {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(5);
        push_hash_entry(&mut entries, "shape", self.shape.serialized_name());
        if !self.colors.is_empty() {
            push_hash_entry(&mut entries, "colors", &CodecIntList(&self.colors));
        }
        if !self.fade_colors.is_empty() {
            push_hash_entry(
                &mut entries,
                "fade_colors",
                &CodecIntList(&self.fade_colors),
            );
        }
        if self.has_trail {
            push_hash_entry(&mut entries, "has_trail", &true);
        }
        if self.has_twinkle {
            push_hash_entry(&mut entries, "has_twinkle", &true);
        }
        hash_entries(hasher, &mut entries);
    }
}

/// Flight duration and ordered explosions carried by a firework rocket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fireworks {
    flight_duration: i32,
    explosions: Vec<FireworkExplosion>,
}

impl Fireworks {
    pub const MAX_EXPLOSIONS: usize = 256;

    /// Creates a firework component accepted by Vanilla's stream codec.
    pub fn new(flight_duration: i32, explosions: Vec<FireworkExplosion>) -> Result<Self> {
        if explosions.len() > Self::MAX_EXPLOSIONS {
            return Err(Error::other(format!(
                "Got {} explosions, but maximum is {}",
                explosions.len(),
                Self::MAX_EXPLOSIONS
            )));
        }
        Ok(Self {
            flight_duration,
            explosions,
        })
    }

    pub(crate) const fn from_extracted(flight_duration: i32) -> Self {
        assert!(
            flight_duration >= 0 && flight_duration <= u8::MAX as i32,
            "extracted firework flight duration must be in 0..=255"
        );
        Self {
            flight_duration,
            explosions: Vec::new(),
        }
    }

    #[must_use]
    pub const fn flight_duration(&self) -> i32 {
        self.flight_duration
    }

    #[must_use]
    pub fn explosions(&self) -> &[FireworkExplosion] {
        &self.explosions
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if self.flight_duration != 0 {
            compound.insert("flight_duration", self.flight_duration as u8 as i8);
        }
        if !self.explosions.is_empty() {
            compound.insert(
                "explosions",
                NbtList::Compound(
                    self.explosions
                        .iter()
                        .map(|explosion| match explosion.to_nbt_tag_ref() {
                            NbtTag::Compound(compound) => compound,
                            _ => {
                                unreachable!("firework explosion codec always produces a compound")
                            }
                        })
                        .collect(),
                ),
            );
        }
        NbtTag::Compound(compound)
    }

    pub(crate) fn try_to_persistent_nbt(&self) -> Result<NbtTag> {
        if self.flight_duration > i32::from(u8::MAX) {
            return Err(Error::other("Firework flight duration exceeds 255"));
        }
        Ok(self.to_nbt_tag_ref())
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let flight_duration = match compound.get("flight_duration") {
            Some(tag) => i32::from(tag.codec_i32()? as i8 as u8),
            None => 0,
        };
        let explosions = match compound.get("explosions") {
            Some(tag) => {
                let list = tag.list()?.as_nbt_tags();
                if list.len() > Self::MAX_EXPLOSIONS {
                    return None;
                }
                list.iter()
                    .map(FireworkExplosion::from_owned_nbt)
                    .collect::<Option<Vec<_>>>()?
            }
            None => Vec::new(),
        };
        Self::new(flight_duration, explosions).ok()
    }
}

impl WriteTo for Fireworks {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.flight_duration).write(writer)?;
        write_count(self.explosions.len(), Self::MAX_EXPLOSIONS, writer)?;
        for explosion in &self.explosions {
            explosion.write(writer)?;
        }
        Ok(())
    }
}

impl ReadFrom for Fireworks {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let flight_duration = VarInt::read(data)?.0;
        let count = read_count(data, Self::MAX_EXPLOSIONS)?;
        let mut explosions = Vec::with_capacity(count);
        for _ in 0..count {
            explosions.push(FireworkExplosion::read(data)?);
        }
        Self::new(flight_duration, explosions)
    }
}

impl ToNbtTag for Fireworks {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for Fireworks {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Self::from_owned_nbt(&tag.to_owned())
    }
}

impl HashComponent for Fireworks {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(2);
        if self.flight_duration != 0 {
            push_hash_entry(
                &mut entries,
                "flight_duration",
                &(self.flight_duration as u8 as i8),
            );
        }
        if !self.explosions.is_empty() {
            push_hash_entry(
                &mut entries,
                "explosions",
                &CodecExplosionList(&self.explosions),
            );
        }
        hash_entries(hasher, &mut entries);
    }
}

struct CodecIntList<'a>(&'a [i32]);

impl HashComponent for CodecIntList<'_> {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.start_list();
        for color in self.0 {
            hasher.put_component_hash(color);
        }
        hasher.end_list();
    }
}

struct CodecExplosionList<'a>(&'a [FireworkExplosion]);

impl HashComponent for CodecExplosionList<'_> {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.start_list();
        for explosion in self.0 {
            hasher.put_component_hash(explosion);
        }
        hasher.end_list();
    }
}

fn int_list_nbt(values: &[i32]) -> NbtList {
    if values.is_empty() {
        NbtList::Empty
    } else {
        NbtList::Int(values.to_vec())
    }
}

fn int_list_from_nbt(tag: &NbtTag) -> Option<Vec<i32>> {
    tag.list()?
        .as_nbt_tags()
        .iter()
        .map(steel_utils::nbt::NbtNumeric::codec_i32)
        .collect()
}

fn optional_bool(tag: Option<&NbtTag>, default: bool) -> Option<bool> {
    match tag {
        Some(tag) => tag.codec_bool(),
        None => Some(default),
    }
}

fn write_int_list(values: &[i32], writer: &mut impl Write) -> Result<()> {
    write_count(values.len(), i32::MAX as usize, writer)?;
    for value in values {
        value.write(writer)?;
    }
    Ok(())
}

fn read_int_list(data: &mut Cursor<&[u8]>) -> Result<Vec<i32>> {
    let count = read_count(data, i32::MAX as usize)?;
    let mut values = Vec::with_capacity(count.min(65_536));
    for _ in 0..count {
        values.push(i32::read(data)?);
    }
    Ok(values)
}

fn write_count(count: usize, max: usize, writer: &mut impl Write) -> Result<()> {
    if count > max || count > i32::MAX as usize {
        return Err(Error::other(format!(
            "Collection size {count} exceeds {max}"
        )));
    }
    VarInt(count as i32).write(writer)
}

fn read_count(data: &mut Cursor<&[u8]>, max: usize) -> Result<usize> {
    let count = VarInt::read(data)?.0;
    let count = usize::try_from(count)
        .map_err(|_| Error::other(format!("Negative collection size: {count}")))?;
    if count > max {
        return Err(Error::other(format!(
            "Collection size {count} exceeds {max}"
        )));
    }
    Ok(count)
}

fn push_hash_entry<T: HashComponent + ?Sized>(entries: &mut Vec<HashEntry>, key: &str, value: &T) {
    let mut key_hasher = ComponentHasher::new();
    key_hasher.put_string(key);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

fn hash_entries(hasher: &mut ComponentHasher, entries: &mut [HashEntry]) {
    sort_map_entries(entries);
    hasher.start_map();
    for entry in entries {
        hasher.put_raw_bytes(&entry.key_bytes);
        hasher.put_raw_bytes(&entry.value_bytes);
    }
    hasher.end_map();
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::ToNbtTag as _;
    use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::{FireworkExplosion, FireworkExplosionShape, Fireworks};
    use crate::data_components::vanilla_components::FIREWORKS;
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryExt};

    fn parse<T: simdnbt::FromNbtTag>(tag: NbtTag) -> Option<T> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        T::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn explosion_codecs_match_shape_and_optional_fields() {
        let explosion = FireworkExplosion::new(
            FireworkExplosionShape::Star,
            vec![0x123456],
            vec![0x654321],
            true,
            false,
        );
        let nbt = explosion.clone().to_nbt_tag();
        assert_eq!(parse(nbt.clone()), Some(explosion.clone()));
        // HashOps preserves Codec.BOOL while NbtOps represents booleans as bytes.
        assert_ne!(explosion.compute_hash(), nbt.compute_hash());

        let mut network = Vec::new();
        explosion
            .write(&mut network)
            .expect("explosion should encode");
        assert_eq!(
            FireworkExplosion::read(&mut Cursor::new(network.as_slice()))
                .expect("explosion should decode"),
            explosion
        );
    }

    #[test]
    fn unknown_stream_shape_ids_fall_back_to_small_ball() {
        let mut network = Vec::new();
        steel_utils::codec::VarInt(99)
            .write(&mut network)
            .expect("shape should encode");
        steel_utils::codec::VarInt(0)
            .write(&mut network)
            .expect("colors should encode");
        steel_utils::codec::VarInt(0)
            .write(&mut network)
            .expect("fade colors should encode");
        false.write(&mut network).expect("trail should encode");
        false.write(&mut network).expect("twinkle should encode");
        let decoded = FireworkExplosion::read(&mut Cursor::new(network.as_slice()))
            .expect("explosion should decode");
        assert_eq!(decoded.shape(), FireworkExplosionShape::SmallBall);
    }

    #[test]
    fn fireworks_use_unsigned_byte_persistence_and_bounded_lists() {
        let firework =
            Fireworks::new(255, vec![FireworkExplosion::default()]).expect("valid firework");
        let mut explosion = NbtCompound::new();
        explosion.insert("shape", "small_ball");
        let mut expected = NbtCompound::new();
        expected.insert("flight_duration", -1_i8);
        expected.insert("explosions", NbtList::Compound(vec![explosion]));
        let expected = NbtTag::Compound(expected);
        assert_eq!(firework.clone().to_nbt_tag(), expected);
        assert_eq!(parse(expected.clone()), Some(firework.clone()));
        assert_eq!(firework.compute_hash(), expected.compute_hash());

        let mut network = Vec::new();
        firework
            .write(&mut network)
            .expect("firework should encode");
        assert_eq!(
            Fireworks::read(&mut Cursor::new(network.as_slice())).expect("firework should decode"),
            firework
        );
        let negative = Fireworks::new(-1, Vec::new()).expect("stream permits negative flight");
        let oversized = Fireworks::new(256, Vec::new()).expect("stream permits any VarInt flight");
        for value in [&negative, &oversized] {
            let mut encoded = Vec::new();
            value
                .write(&mut encoded)
                .expect("stream value should encode");
            assert_eq!(
                Fireworks::read(&mut Cursor::new(encoded.as_slice()))
                    .expect("stream value should decode"),
                *value
            );
        }
        assert!(negative.try_to_persistent_nbt().is_ok());
        assert!(oversized.try_to_persistent_nbt().is_err());
        assert!(Fireworks::new(0, vec![FireworkExplosion::default(); 257]).is_err());
    }

    #[test]
    fn extracted_firework_rocket_has_one_unit_of_flight() {
        init_test_registry();
        let rocket = REGISTRY
            .items
            .by_key(&steel_utils::Identifier::vanilla_static("firework_rocket"))
            .expect("firework rocket should be registered");
        assert_eq!(
            rocket.components.get(FIREWORKS),
            Some(Fireworks::new(1, Vec::new()).expect("valid default rocket"))
        );
    }
}
