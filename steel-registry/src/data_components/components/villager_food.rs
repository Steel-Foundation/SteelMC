//! Vanilla `minecraft:villager_food` item component.

use std::io::{Cursor, Result, Write};

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};

/// How much a villager's breeding food level is raised by consuming this item.
///
/// Vanilla: `ExtraCodecs.POSITIVE_INT.fieldOf("nutrition")` persistent,
/// `ByteBufCodecs.VAR_INT` on the network (no range check on decode there,
/// matching `VillagerFood`'s public constructor).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VillagerFood {
    nutrition: i32,
}

impl VillagerFood {
    #[must_use]
    pub const fn new(nutrition: i32) -> Self {
        Self { nutrition }
    }

    #[must_use]
    pub const fn nutrition(self) -> i32 {
        self.nutrition
    }
}

impl WriteTo for VillagerFood {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.nutrition).write(writer)
    }
}

impl ReadFrom for VillagerFood {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(VarInt::read(data)?.0))
    }
}

impl ToNbtTag for VillagerFood {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("nutrition", NbtTag::Int(self.nutrition));
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for VillagerFood {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag<'_, '_>) -> Option<Self> {
        let compound = tag.compound()?;
        let nutrition = compound.get("nutrition")?.codec_i32()?;
        (nutrition > 0).then(|| Self::new(nutrition))
    }
}

impl HashComponent for VillagerFood {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut key_hasher = ComponentHasher::new();
        key_hasher.put_string("nutrition");
        let mut value_hasher = ComponentHasher::new();
        value_hasher.put_int(self.nutrition);
        let entry = HashEntry::new(key_hasher, value_hasher);

        hasher.start_map();
        hasher.put_raw_bytes(&entry.key_bytes);
        hasher.put_raw_bytes(&entry.value_bytes);
        hasher.end_map();
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::FromNbtTag as _;
    use simdnbt::borrow::read_tag;
    use simdnbt::owned::{NbtCompound, NbtTag};
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::VillagerFood;

    fn parse(tag: NbtTag) -> Option<VillagerFood> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        VillagerFood::from_nbt_tag(borrowed.as_tag())
    }

    fn compound_with(nutrition: NbtTag) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("nutrition", nutrition);
        NbtTag::Compound(compound)
    }

    #[test]
    fn persistent_codec_round_trips_positive_nutrition() {
        let value = VillagerFood::new(4);
        assert_eq!(parse(value.clone().to_nbt_tag()), Some(value));
    }

    #[test]
    fn persistent_codec_rejects_non_positive_nutrition() {
        assert_eq!(parse(compound_with(NbtTag::Int(0))), None);
        assert_eq!(parse(compound_with(NbtTag::Int(-1))), None);
    }

    #[test]
    fn persistent_codec_accepts_any_integral_nbt_tag_type() {
        // `ExtraCodecs.POSITIVE_INT` reads through `Codec.INT`, which simdnbt's
        // `codec_i32` mirrors by widening from any integral tag.
        assert_eq!(parse(compound_with(NbtTag::Byte(4))), Some(VillagerFood::new(4)));
        assert_eq!(parse(compound_with(NbtTag::Long(4))), Some(VillagerFood::new(4)));
    }

    #[test]
    fn network_codec_round_trips_varint() {
        let value = VillagerFood::new(4);
        let mut network = Vec::new();
        value.write(&mut network).expect("food should encode");
        assert_eq!(
            VillagerFood::read(&mut Cursor::new(network.as_slice())).expect("food should decode"),
            value
        );
    }
}
