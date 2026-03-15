use crate::RegistryExt;
use rustc_hash::FxHashMap;
use steel_utils::Identifier;
use steel_utils::registry::registry_vanilla_or_custom_tag;
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

    pub fn register_tag(&mut self, tag: Identifier, keys: &[&'static str]) {
        assert!(
            self.allows_registering,
            "Cannot register tags after registry has been frozen"
        );

        let identifiers: Vec<Identifier> = keys
            .iter()
            .filter_map(|key| {
                let ident = registry_vanilla_or_custom_tag(key);
                self.by_key(&ident).map(|_| ident)
            })
            .collect();

        self.tags.insert(tag, identifiers);
    }

    #[must_use]
    pub fn is_in_tag(&self, entry: InstrumentRef, tag: &Identifier) -> bool {
        self.tags
            .get(tag)
            .is_some_and(|entries| entries.contains(&entry.key))
    }

    pub fn modify_tag(
        &mut self,
        tag: &Identifier,
        f: impl FnOnce(Vec<Identifier>) -> Vec<Identifier>,
    ) {
        let existing = self.tags.remove(tag).unwrap_or_default();
        let entries = f(existing)
            .into_iter()
            .filter(|key| {
                let exists = self.instruments_by_key.contains_key(key);
                if !exists {
                    tracing::error!(
                        "instrument {key} not found in registry, skipping from tag {tag}"
                    );
                }
                exists
            })
            .collect();
        self.tags.insert(tag.clone(), entries);
    }

    #[must_use]
    pub fn get_tag(&self, tag: &Identifier) -> Option<Vec<InstrumentRef>> {
        self.tags.get(tag).map(|idents| {
            idents
                .iter()
                .filter_map(|ident| self.by_key(ident))
                .collect()
        })
    }

    pub fn iter_tag(&self, tag: &Identifier) -> impl Iterator<Item = InstrumentRef> + '_ {
        self.tags
            .get(tag)
            .into_iter()
            .flat_map(|v| v.iter().filter_map(|ident| self.by_key(ident)))
    }

    pub fn tag_keys(&self) -> impl Iterator<Item = &Identifier> {
        self.tags.keys()
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
