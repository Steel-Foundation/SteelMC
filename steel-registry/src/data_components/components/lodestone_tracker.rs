//! Vanilla `minecraft:lodestone_tracker` item component.

use std::io::{Cursor, Result, Write};
use std::str::FromStr;

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};
use steel_utils::{BlockPos, Identifier};

/// A block position paired with a dimension resource key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalPos {
    dimension: Identifier,
    pos: BlockPos,
}

impl GlobalPos {
    #[must_use]
    pub const fn new(dimension: Identifier, pos: BlockPos) -> Self {
        Self { dimension, pos }
    }

    #[must_use]
    pub const fn dimension(&self) -> &Identifier {
        &self.dimension
    }

    #[must_use]
    pub const fn pos(&self) -> BlockPos {
        self.pos
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("dimension", self.dimension.to_string());
        compound.insert(
            "pos",
            NbtTag::IntArray(vec![self.pos.x(), self.pos.y(), self.pos.z()]),
        );
        NbtTag::Compound(compound)
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let dimension =
            Identifier::from_str(&compound.get("dimension")?.string()?.to_string()).ok()?;
        let coordinates = int_stream_from_nbt(compound.get("pos")?)?;
        let [x, y, z]: [i32; 3] = coordinates.try_into().ok()?;
        Some(Self::new(dimension, BlockPos::new(x, y, z)))
    }
}

impl WriteTo for GlobalPos {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.dimension.write(writer)?;
        self.pos.write(writer)
    }
}

impl ReadFrom for GlobalPos {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(Identifier::read(data)?, BlockPos::read(data)?))
    }
}

impl HashComponent for GlobalPos {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(2);
        push_hash_entry(&mut entries, "dimension", &self.dimension);
        push_hash_entry(&mut entries, "pos", &CodecBlockPos(self.pos));
        hash_entries(hasher, &mut entries);
    }
}

/// Optional lodestone target and whether Vanilla should keep validating it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LodestoneTracker {
    target: Option<GlobalPos>,
    tracked: bool,
}

impl LodestoneTracker {
    #[must_use]
    pub const fn new(target: Option<GlobalPos>, tracked: bool) -> Self {
        Self { target, tracked }
    }

    #[must_use]
    pub const fn target(&self) -> Option<&GlobalPos> {
        self.target.as_ref()
    }

    #[must_use]
    pub const fn tracked(&self) -> bool {
        self.tracked
    }
}

impl WriteTo for LodestoneTracker {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.target.is_some().write(writer)?;
        if let Some(target) = &self.target {
            target.write(writer)?;
        }
        self.tracked.write(writer)
    }
}

impl ReadFrom for LodestoneTracker {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let target = if bool::read(data)? {
            Some(GlobalPos::read(data)?)
        } else {
            None
        };
        Ok(Self::new(target, bool::read(data)?))
    }
}

impl ToNbtTag for LodestoneTracker {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(target) = self.target {
            compound.insert("target", target.to_nbt_tag_ref());
        }
        if !self.tracked {
            compound.insert("tracked", false);
        }
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for LodestoneTracker {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let target = match compound.get("target") {
            Some(tag) => Some(GlobalPos::from_owned_nbt(&tag.to_owned())?),
            None => None,
        };
        let tracked = match compound.get("tracked") {
            Some(tag) => tag.codec_bool()?,
            None => true,
        };
        Some(Self::new(target, tracked))
    }
}

impl HashComponent for LodestoneTracker {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(2);
        if let Some(target) = &self.target {
            push_hash_entry(&mut entries, "target", target);
        }
        if !self.tracked {
            push_hash_entry(&mut entries, "tracked", &false);
        }
        hash_entries(hasher, &mut entries);
    }
}

struct CodecBlockPos(BlockPos);

impl HashComponent for CodecBlockPos {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_int_array(&[self.0.x(), self.0.y(), self.0.z()]);
    }
}

fn int_stream_from_nbt(tag: &NbtTag) -> Option<Vec<i32>> {
    match tag {
        NbtTag::IntArray(values) => Some(values.clone()),
        NbtTag::List(list) => list
            .as_nbt_tags()
            .iter()
            .map(steel_utils::nbt::NbtNumeric::codec_i32)
            .collect(),
        _ => None,
    }
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

    use simdnbt::owned::{NbtCompound, NbtTag};
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};
    use steel_utils::{BlockPos, Identifier};

    use super::{GlobalPos, LodestoneTracker};

    fn parse(tag: NbtTag) -> Option<LodestoneTracker> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        LodestoneTracker::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn lodestone_target_round_trips_and_hashes_through_global_pos_codec() {
        let tracker = LodestoneTracker::new(
            Some(GlobalPos::new(
                Identifier::vanilla_static("overworld"),
                BlockPos::new(12, -4, 99),
            )),
            false,
        );
        let nbt = tracker.clone().to_nbt_tag();
        assert_eq!(parse(nbt.clone()), Some(tracker.clone()));
        // HashOps preserves Codec.BOOL while NbtOps represents booleans as bytes.
        assert_ne!(tracker.compute_hash(), nbt.compute_hash());

        let mut network = Vec::new();
        tracker.write(&mut network).expect("tracker should encode");
        assert_eq!(
            LodestoneTracker::read(&mut Cursor::new(network.as_slice()))
                .expect("tracker should decode"),
            tracker
        );
    }

    #[test]
    fn absent_fields_default_to_no_target_and_tracked() {
        assert_eq!(
            parse(NbtTag::Compound(NbtCompound::new())),
            Some(LodestoneTracker::new(None, true))
        );
    }
}
