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

impl ToNbtTag for Instrument {
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

    pub fn register(&mut self, instrument: InstrumentRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register instruments after the registry has been frozen"
        );

        let id = self.instruments_by_id.len();
        self.instruments_by_key.insert(instrument.key.clone(), id);
        self.instruments_by_id.push(instrument);
        id
    }

    /// Replaces a instrument at a given index.
    /// Returns true if the instrument was replaced and false if the instrument wasn't replaced
    #[must_use]
    pub fn replace(&mut self, instrument: InstrumentRef, id: usize) -> bool {
        if id >= self.instruments_by_id.len() {
            return false;
        }
        self.instruments_by_id[id] = instrument;
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, InstrumentRef)> + '_ {
        self.instruments_by_id
            .iter()
            .enumerate()
            .map(|(id, &instrument)| (id, instrument))
    }
}

impl Default for InstrumentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry!(
    InstrumentRegistry,
    Instrument,
    instruments_by_id,
    instruments_by_key,
    instruments
);

crate::impl_tagged_registry!(InstrumentRegistry, instruments_by_key, "instrument");
