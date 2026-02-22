use crate::RegistryExt;
use rustc_hash::FxHashMap;
use steel_utils::Identifier;
use steel_utils::registry::registry_vanilla_or_custom_tag;

/// Represents a world_clock definition from a data pack JSON file.
#[derive(Debug)]
pub struct WorldClock {
    pub key: Identifier,
}

impl WorldClock {
    pub fn to_nbt(&self) -> simdnbt::owned::NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};
        let mut compound = NbtCompound::new();
        compound.insert("key", self.key.to_string());
        NbtTag::Compound(compound)
    }
}

pub type WorldClockRef = &'static WorldClock;

pub struct WorldClockRegistry {
    world_clocks_by_id: Vec<WorldClockRef>,
    world_clocks_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl WorldClockRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            world_clocks_by_id: Vec::new(),
            world_clocks_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, world_clock: WorldClockRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register world_clocks after the registry has been frozen"
        );

        let id = self.world_clocks_by_id.len();
        self.world_clocks_by_key.insert(world_clock.key.clone(), id);
        self.world_clocks_by_id.push(world_clock);
        id
    }

    /// Replaces a world_clocks at a given index.
    /// Returns true if the world_clock was replaced and false if the world_clock wasn't replaced
    #[must_use]
    pub fn replace(&mut self, world_clock: WorldClockRef, id: usize) -> bool {
        if id >= self.world_clocks_by_id.len() {
            return false;
        }
        self.world_clocks_by_id[id] = world_clock;
        true
    }

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<WorldClockRef> {
        self.world_clocks_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, world_clock: WorldClockRef) -> &usize {
        self.world_clocks_by_key
            .get(&world_clock.key)
            .expect("WorldClock not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<WorldClockRef> {
        self.world_clocks_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, WorldClockRef)> + '_ {
        self.world_clocks_by_id
            .iter()
            .enumerate()
            .map(|(id, &world_clock)| (id, world_clock))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.world_clocks_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.world_clocks_by_id.is_empty()
    }

    // Tag-related methods

    /// Registers a tag with a list of world_clock keys.
    /// WorldClock keys that don't exist in the registry are silently skipped.
    pub fn register_tag(&mut self, tag: Identifier, world_clock_keys: &[&'static str]) {
        assert!(
            self.allows_registering,
            "Cannot register tags after registry has been frozen"
        );

        let identifier: Vec<Identifier> = world_clock_keys
            .iter()
            .filter_map(|key| {
                let ident = registry_vanilla_or_custom_tag(key);
                // Only include if the item actually exists
                self.by_key(&ident).map(|_| ident)
            })
            .collect();

        self.tags.insert(tag, identifier);
    }

    /// Checks if a fluid is in a given tag.
    #[must_use]
    pub fn is_in_tag(&self, world_clock: WorldClockRef, tag: &Identifier) -> bool {
        self.tags
            .get(tag)
            .is_some_and(|world_clocks| world_clocks.contains(&world_clock.key))
    }

    /// Gives the access to all blocks to delete and add new entries
    pub fn modify_tag(
        &mut self,
        tag: &Identifier,
        f: impl FnOnce(Vec<Identifier>) -> Vec<Identifier>,
    ) {
        let existing = self.tags.remove(tag).unwrap_or_default();
        let world_clocks = f(existing)
            .into_iter()
            .filter(|world_clock| {
                let exists = self.world_clocks_by_key.contains_key(world_clock);
                if !exists {
                    tracing::error!(
                        "world_clock {world_clock} not found in registry, skipping from tag {tag}"
                    );
                }
                exists
            })
            .collect();
        self.tags.insert(tag.clone(), world_clocks);
    }

    /// Gets all world_clocks in a tag.
    #[must_use]
    pub fn get_tag(&self, tag: &Identifier) -> Option<Vec<WorldClockRef>> {
        self.tags.get(tag).map(|idents| {
            idents
                .iter()
                .filter_map(|ident| self.by_key(ident))
                .collect()
        })
    }

    /// Iterates over all world_clocks in a tag.
    pub fn iter_tag(&self, tag: &Identifier) -> impl Iterator<Item = WorldClockRef> + '_ {
        self.tags
            .get(tag)
            .into_iter()
            .flat_map(|v| v.iter().filter_map(|ident| self.by_key(ident)))
    }

    /// Returns an iterator over all tag keys.
    pub fn tag_keys(&self) -> impl Iterator<Item = &Identifier> {
        self.tags.keys()
    }
}

impl RegistryExt for WorldClockRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for WorldClockRegistry {
    fn default() -> Self {
        Self::new()
    }
}
