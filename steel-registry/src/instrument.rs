use std::fmt::{self, Display, Formatter};
use std::io::{Cursor, Error, Result as IoResult, Write};

use rustc_hash::FxHashMap;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};
use text_components::TextComponent;

use crate::sound_event::SoundEventHolder;
use crate::{REGISTRY, RegistryExt, RegistryHolderEntry};

/// A complete instrument definition, either registered or stored inline.
///
/// The direct stream codec carries raw floats, but Steel validates them against
/// the persistent codec before constructing this value so item stacks cannot
/// contain an instrument that later fails to save.
#[derive(Debug, Clone)]
pub struct InstrumentValue {
    sound_event: SoundEventHolder,
    use_duration: f32,
    range: f32,
    description: TextComponent,
}

impl InstrumentValue {
    pub fn new(
        sound_event: SoundEventHolder,
        use_duration: f32,
        range: f32,
        description: TextComponent,
    ) -> Result<Self, InvalidInstrumentValue> {
        if !is_positive_float(use_duration) {
            return Err(InvalidInstrumentValue::UseDuration(use_duration));
        }
        if !is_positive_float(range) {
            return Err(InvalidInstrumentValue::Range(range));
        }
        Ok(Self {
            sound_event,
            use_duration,
            range,
            description,
        })
    }

    pub(crate) const fn from_validated_parts(
        sound_event: SoundEventHolder,
        use_duration: f32,
        range: f32,
        description: TextComponent,
    ) -> Self {
        assert!(is_positive_float(use_duration));
        assert!(is_positive_float(range));
        Self {
            sound_event,
            use_duration,
            range,
            description,
        }
    }

    #[must_use]
    pub const fn sound_event(&self) -> &SoundEventHolder {
        &self.sound_event
    }

    #[must_use]
    pub const fn use_duration(&self) -> f32 {
        self.use_duration
    }

    #[must_use]
    pub const fn range(&self) -> f32 {
        self.range
    }

    #[must_use]
    pub const fn description(&self) -> &TextComponent {
        &self.description
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("sound_event", self.sound_event.clone().to_nbt_tag());
        compound.insert("use_duration", self.use_duration);
        compound.insert("range", self.range);
        compound.insert("description", self.description.to_codec_nbt());
        NbtTag::Compound(compound)
    }
}

const fn is_positive_float(value: f32) -> bool {
    value > 0.0 && value <= f32::MAX
}

impl PartialEq for InstrumentValue {
    fn eq(&self, other: &Self) -> bool {
        self.sound_event == other.sound_event
            && java_float_equals(self.use_duration, other.use_duration)
            && java_float_equals(self.range, other.range)
            && self.description == other.description
    }
}

const fn java_float_equals(left: f32, right: f32) -> bool {
    (left.is_nan() && right.is_nan()) || left.to_bits() == right.to_bits()
}

impl WriteTo for InstrumentValue {
    fn write(&self, writer: &mut impl Write) -> IoResult<()> {
        self.sound_event.write(writer)?;
        self.use_duration.write(writer)?;
        self.range.write(writer)?;
        WriteTo::write(&self.description.to_codec_nbt(), writer)
    }
}

impl ReadFrom for InstrumentValue {
    fn read(data: &mut Cursor<&[u8]>) -> IoResult<Self> {
        Self::new(
            SoundEventHolder::read(data)?,
            f32::read(data)?,
            f32::read(data)?,
            TextComponent::read(data)?,
        )
        .map_err(Error::other)
    }
}

impl ToNbtTag for InstrumentValue {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for InstrumentValue {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let description = TextComponent::from_nbt(&compound.get("description")?.to_owned())?;
        Self::new(
            SoundEventHolder::from_nbt_tag(compound.get("sound_event")?)?,
            compound.get("use_duration")?.codec_f32()?,
            compound.get("range")?.codec_f32()?,
            description,
        )
        .ok()
    }
}

impl HashComponent for InstrumentValue {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        push_hash_entry(&mut entries, "sound_event", &self.sound_event);
        push_hash_entry(&mut entries, "use_duration", &self.use_duration);
        push_hash_entry(&mut entries, "range", &self.range);
        push_hash_entry(&mut entries, "description", &self.description);
        sort_map_entries(&mut entries);
        hasher.start_map();
        for entry in &entries {
            hasher.put_raw_bytes(&entry.key_bytes);
            hasher.put_raw_bytes(&entry.value_bytes);
        }
        hasher.end_map();
    }
}

fn push_hash_entry<T: HashComponent + ?Sized>(entries: &mut Vec<HashEntry>, key: &str, value: &T) {
    let mut key_hasher = ComponentHasher::new();
    key.hash_component(&mut key_hasher);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

/// Invalid value rejected by `ExtraCodecs.POSITIVE_FLOAT`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InvalidInstrumentValue {
    UseDuration(f32),
    Range(f32),
}

impl Display for InvalidInstrumentValue {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UseDuration(value) => write!(formatter, "use duration must be positive: {value}"),
            Self::Range(value) => write!(formatter, "range must be positive: {value}"),
        }
    }
}

impl std::error::Error for InvalidInstrumentValue {}

/// Registered instrument definition, primarily used by goat horns.
#[derive(Debug)]
pub struct Instrument {
    pub key: Identifier,
    value: InstrumentValue,
}

impl Instrument {
    #[must_use]
    pub const fn new(key: Identifier, value: InstrumentValue) -> Self {
        Self { key, value }
    }

    #[must_use]
    pub const fn value(&self) -> &InstrumentValue {
        &self.value
    }
}

impl ToNbtTag for &Instrument {
    fn to_nbt_tag(self) -> NbtTag {
        self.value.to_nbt_tag_ref()
    }
}

pub type InstrumentRef = &'static Instrument;

pub struct InstrumentRegistry {
    instruments_by_id: Vec<InstrumentRef>,
    instruments_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl InstrumentRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            instruments_by_id: Vec::new(),
            instruments_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    InstrumentRegistry,
    InstrumentRef,
    instruments_by_id,
    instruments_by_key,
    allows_registering
);

crate::impl_registry!(
    InstrumentRegistry,
    Instrument,
    instruments_by_id,
    instruments_by_key,
    instruments
);

crate::impl_tagged_registry!(InstrumentRegistry, instruments_by_key, "instrument");

impl RegistryHolderEntry for Instrument {
    type Value = InstrumentValue;

    const REGISTRY_NAME: &'static str = "instrument";

    fn holder_value(&self) -> &Self::Value {
        &self.value
    }

    fn holder_by_id(id: usize) -> Option<&'static Self> {
        REGISTRY.instruments.by_id(id)
    }

    fn holder_by_key(key: &Identifier) -> Option<&'static Self> {
        REGISTRY.instruments.by_key(key)
    }
}

#[cfg(test)]
mod tests {
    use simdnbt::ToNbtTag;
    use simdnbt::owned::NbtTag;

    use crate::{test_support::init_test_registry, vanilla_instruments};

    #[test]
    fn nbt_uses_sound_event_registry_key() {
        init_test_registry();

        let NbtTag::Compound(compound) = (&vanilla_instruments::PONDER_GOAT_HORN).to_nbt_tag()
        else {
            panic!("instrument did not serialize to a compound tag");
        };

        let Some(sound_event) = compound.string("sound_event") else {
            panic!("instrument NBT is missing sound_event string");
        };

        assert_eq!(
            sound_event.to_str().as_ref(),
            "minecraft:item.goat_horn.sound.0"
        );
    }
}
