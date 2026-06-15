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
}

crate::impl_standard_methods!(
    WorldClockRegistry,
    WorldClockRef,
    world_clocks_by_id,
    world_clocks_by_key,
    allows_registering
);

crate::impl_registry!(
    WorldClockRegistry,
    WorldClock,
    world_clocks_by_id,
    world_clocks_by_key,
    world_clocks
);

crate::impl_tagged_registry!(WorldClockRegistry, world_clocks_by_key, "World Clock");
