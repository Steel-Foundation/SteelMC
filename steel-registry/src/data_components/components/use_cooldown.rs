//! Vanilla `minecraft:use_cooldown` item component.

use std::io::{Cursor, Result, Write};
use std::str::FromStr;

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::serial::{ReadFrom, WriteTo};
use steel_utils::{Identifier, nbt::NbtNumeric as _};

/// Vanilla `UseCooldown`: seconds plus an optional shared cooldown group.
#[derive(Debug, Clone, PartialEq)]
pub struct UseCooldown {
    pub seconds: f32,
    pub cooldown_group: Option<Identifier>,
}

impl UseCooldown {
    #[must_use]
    pub const fn new(seconds: f32, cooldown_group: Option<Identifier>) -> Self {
        Self {
            seconds,
            cooldown_group,
        }
    }

    /// Returns vanilla `UseCooldown.ticks()`.
    #[must_use]
    pub fn ticks(&self) -> i32 {
        (self.seconds * 20.0) as i32
    }
}

impl WriteTo for UseCooldown {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.seconds.write(writer)?;
        self.cooldown_group.write(writer)
    }
}

impl ReadFrom for UseCooldown {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            seconds: f32::read(data)?,
            cooldown_group: Option::<Identifier>::read(data)?,
        })
    }
}

impl ToNbtTag for UseCooldown {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("seconds", self.seconds);
        if let Some(group) = self.cooldown_group {
            compound.insert("cooldown_group", group.to_string());
        }
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for UseCooldown {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let seconds = compound.get("seconds")?.codec_f32()?;
        if !seconds.is_finite() || seconds <= 0.0 {
            return None;
        }
        let cooldown_group = match compound.get("cooldown_group") {
            Some(tag) => Some(Identifier::from_str(&tag.string()?.to_str()).ok()?),
            None => None,
        };
        Some(Self {
            seconds,
            cooldown_group,
        })
    }
}

impl HashComponent for UseCooldown {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        push_hash_entry(&mut entries, "seconds", &self.seconds);
        if let Some(group) = &self.cooldown_group {
            push_hash_entry(&mut entries, "cooldown_group", &group.to_string());
        }
        sort_map_entries(&mut entries);
        hasher.start_map();
        for entry in entries {
            hasher.put_raw_bytes(&entry.key_bytes);
            hasher.put_raw_bytes(&entry.value_bytes);
        }
        hasher.end_map();
    }
}

fn push_hash_entry<T: HashComponent + ?Sized>(entries: &mut Vec<HashEntry>, key: &str, value: &T) {
    let mut key_hasher = ComponentHasher::new();
    key_hasher.put_string(key);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::FromNbtTag;
    use simdnbt::borrow::{NbtTag as BorrowedNbtTag, read_tag};
    use simdnbt::owned::{NbtCompound, NbtTag};
    use steel_utils::Identifier;

    use super::UseCooldown;

    fn with_borrowed_tag<R>(tag: NbtTag, visitor: impl FnOnce(BorrowedNbtTag<'_, '_>) -> R) -> R {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed =
            read_tag(&mut Cursor::new(bytes.as_slice())).expect("owned test tag should parse");
        visitor(borrowed.as_tag())
    }

    fn parse_use_cooldown(tag: NbtTag) -> Option<UseCooldown> {
        with_borrowed_tag(tag, UseCooldown::from_nbt_tag)
    }

    #[test]
    fn nbt_accepts_positive_seconds_and_optional_group() {
        let mut compound = NbtCompound::new();
        compound.insert("seconds", 5.5_f64);
        compound.insert("cooldown_group", "minecraft:test_group");

        let parsed = parse_use_cooldown(NbtTag::Compound(compound))
            .expect("valid use_cooldown should parse");

        assert_eq!(parsed.seconds, 5.5);
        assert_eq!(
            parsed.cooldown_group,
            Some(Identifier::vanilla_static("test_group"))
        );
    }

    #[test]
    fn nbt_rejects_non_positive_seconds() {
        for seconds in [0.0_f32, -1.0] {
            let mut compound = NbtCompound::new();
            compound.insert("seconds", seconds);

            assert!(parse_use_cooldown(NbtTag::Compound(compound)).is_none());
        }
    }

    #[test]
    fn nbt_rejects_invalid_cooldown_group() {
        let mut compound = NbtCompound::new();
        compound.insert("seconds", 1.0_f32);
        compound.insert("cooldown_group", "not valid");

        assert!(parse_use_cooldown(NbtTag::Compound(compound)).is_none());
    }
}
