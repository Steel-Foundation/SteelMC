use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_utils::Identifier;

/// Represents a world_clock definition from a data pack JSON file.
#[derive(Debug)]
pub struct WorldClock {
    pub key: Identifier,
}

impl ToNbtTag for &WorldClock {
    fn to_nbt_tag(self) -> NbtTag {
        NbtTag::Compound(NbtCompound::new())
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

    pub fn iter(&self) -> impl Iterator<Item = (usize, WorldClockRef)> + '_ {
        self.world_clocks_by_id
            .iter()
            .enumerate()
            .map(|(id, &world_clock)| (id, world_clock))
    }
}

crate::impl_registry!(
    WorldClockRegistry,
    WorldClock,
    world_clocks_by_id,
    world_clocks_by_key,
    world_clocks
);

crate::impl_tagged_registry!(WorldClockRegistry, world_clocks_by_key, "World Clock");

impl Default for WorldClockRegistry {
    fn default() -> Self {
        Self::new()
    }
}
