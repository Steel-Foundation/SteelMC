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

crate::define_registry!(
    WorldClockRegistry,
    WorldClock,
    stem: world_clocks,
    tagged: "World Clock",
);
