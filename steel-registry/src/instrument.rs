use rustc_hash::FxHashMap;
use steel_utils::Identifier;
use text_components::TextComponent;

use crate::RegistryExt;

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

pub type InstrumentRef = &'static Instrument;

pub struct InstrumentRegistry {
    instruments_by_id: Vec<InstrumentRef>,
    instruments_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl InstrumentRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            instruments_by_id: Vec::new(),
            instruments_by_key: FxHashMap::default(),
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

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<InstrumentRef> {
        self.instruments_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, instrument: InstrumentRef) -> &usize {
        self.instruments_by_key
            .get(&instrument.key)
            .expect("Instrument not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<InstrumentRef> {
        self.instruments_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, InstrumentRef)> + '_ {
        self.instruments_by_id
            .iter()
            .enumerate()
            .map(|(id, &instrument)| (id, instrument))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.instruments_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.instruments_by_id.is_empty()
    }
}

impl RegistryExt for InstrumentRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for InstrumentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
