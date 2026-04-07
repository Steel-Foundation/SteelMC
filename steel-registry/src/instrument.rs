use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;
use text_components::TextComponent;

/// Represents a musical instrument definition from a data pack JSON file,
/// primarily used for Goat Horns.
#[derive(Debug)]
pub struct Instrument {
    pub key: Identifier,
    pub sound_event: Identifier,
    pub use_duration: f32,
    pub range: f32,
    pub description: TextComponent,
}

impl ToNbtTag for &Instrument {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::NbtCompound;
        let mut compound = NbtCompound::new();
        let sound_event = self.sound_event.to_string();
        compound.insert("sound_event", sound_event.as_str());
        compound.insert("use_duration", self.use_duration);
        compound.insert("range", self.range);
        compound.insert("description", (&self.description).to_nbt_tag());
        NbtTag::Compound(compound)
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
