//! Validated game profiles

use std::io::{Cursor, Error, Result, Write};
use std::ops::Deref;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::serial::{PrefixedRead as _, PrefixedWrite as _, ReadFrom, WriteTo};
use uuid::Uuid;

const MAX_PROPERTIES: usize = 16;
const MAX_PROPERTY_NAME: usize = 64;
const MAX_PROPERTY_VALUE: usize = 32_767;
const MAX_PROPERTY_SIGNATURE: usize = 1_024;
pub(crate) const MAX_PLAYER_NAME: usize = 16;

/// A game profile property
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileProperty {
    name: String,
    value: String,
    signature: Option<String>,
}

impl ProfileProperty {
    pub fn new(name: String, value: String, signature: Option<String>) -> Result<Self> {
        validate_string(&name, MAX_PROPERTY_NAME, "Profile property name")?;
        validate_string(&value, MAX_PROPERTY_VALUE, "Profile property value")?;
        if let Some(signature) = &signature {
            validate_string(
                signature,
                MAX_PROPERTY_SIGNATURE,
                "Profile property signature",
            )?;
        }
        Ok(Self {
            name,
            value,
            signature,
        })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    #[must_use]
    pub fn signature(&self) -> Option<&str> {
        self.signature.as_deref()
    }
}

/// Profile properties grouped by the first occurrence of each name
#[derive(Debug, Default, Clone)]
pub(crate) struct ProfileProperties(Vec<ProfileProperty>);

impl ProfileProperties {
    pub(crate) fn new(properties: Vec<ProfileProperty>) -> Result<Self> {
        validate_properties(&properties)?;
        let mut grouped = Vec::with_capacity(properties.len());
        for property in properties {
            let insertion_index = grouped
                .iter()
                .rposition(|existing: &ProfileProperty| existing.name() == property.name())
                .map(|index| index + 1);
            if let Some(index) = insertion_index {
                grouped.insert(index, property);
            } else {
                grouped.push(property);
            }
        }
        Ok(Self(grouped))
    }
}

impl Deref for ProfileProperties {
    type Target = [ProfileProperty];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PartialEq for ProfileProperties {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }

        let mut left_start = 0;
        while let Some(first) = self.get(left_start) {
            let name = first.name();
            let left_end = property_group_end(self, left_start);
            let Some(right_start) = other.iter().position(|property| property.name() == name)
            else {
                return false;
            };
            let right_end = property_group_end(other, right_start);
            if self[left_start..left_end] != other[right_start..right_end] {
                return false;
            }
            left_start = left_end;
        }
        true
    }
}

impl Eq for ProfileProperties {}

fn property_group_end(properties: &[ProfileProperty], start: usize) -> usize {
    let Some(first) = properties.get(start) else {
        return start;
    };
    properties[start + 1..]
        .iter()
        .position(|property| property.name() != first.name())
        .map_or(properties.len(), |offset| start + offset + 1)
}

/// A player UUID with name and signed properties such as skin textures
///
/// Construction validates the vanilla network limits
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameProfile {
    id: Uuid,
    name: String,
    properties: ProfileProperties,
}

impl GameProfile {
    pub fn new(id: Uuid, name: String, properties: Vec<ProfileProperty>) -> Result<Self> {
        validate_player_name(&name)?;
        Ok(Self {
            id,
            name,
            properties: ProfileProperties::new(properties)?,
        })
    }

    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn properties(&self) -> &[ProfileProperty] {
        &self.properties
    }

    /// Writes the properties in their network form
    /// Used by packets that send the name and properties separately from the id
    pub fn write_properties(&self, writer: &mut impl Write) -> Result<()> {
        write_properties_network(&self.properties, writer)
    }
}

impl WriteTo for GameProfile {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.id.write(writer)?;
        write_string(&self.name, MAX_PLAYER_NAME, writer)?;
        write_properties_network(&self.properties, writer)
    }
}

impl ReadFrom for GameProfile {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Self::new(
            Uuid::read(data)?,
            read_string(data, MAX_PLAYER_NAME)?,
            read_properties_network(data)?,
        )
    }
}

/// Session server JSON shape that is validated into a game profile on deserialization
#[derive(Deserialize)]
struct GameProfileJson {
    id: Uuid,
    name: String,
    #[serde(default)]
    properties: Vec<ProfilePropertyJson>,
}

#[derive(Deserialize)]
struct ProfilePropertyJson {
    name: String,
    value: String,
    signature: Option<String>,
}

impl<'de> Deserialize<'de> for GameProfile {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let json = GameProfileJson::deserialize(deserializer)?;
        let properties = json
            .properties
            .into_iter()
            .map(|property| ProfileProperty::new(property.name, property.value, property.signature))
            .collect::<Result<Vec<_>>>()
            .map_err(D::Error::custom)?;
        Self::new(json.id, json.name, properties).map_err(D::Error::custom)
    }
}

pub(crate) fn validate_player_name(name: &str) -> Result<()> {
    if name.encode_utf16().count() > MAX_PLAYER_NAME
        || name
            .chars()
            .any(|character| character <= ' ' || character >= '\u{7f}')
    {
        return Err(Error::other("Invalid player profile name"));
    }
    Ok(())
}

pub(crate) fn validate_properties(properties: &[ProfileProperty]) -> Result<()> {
    if properties.len() > MAX_PROPERTIES {
        return Err(Error::other("A profile may contain at most 16 properties"));
    }
    Ok(())
}

pub(crate) fn validate_string(value: &str, max: usize, name: &str) -> Result<()> {
    if value.encode_utf16().count() > max || value.len() > max.saturating_mul(3) {
        return Err(Error::other(format!(
            "{name} exceeds its {max}-character limit"
        )));
    }
    Ok(())
}

pub(crate) fn write_properties_network(
    properties: &[ProfileProperty],
    writer: &mut impl Write,
) -> Result<()> {
    validate_properties(properties)?;
    VarInt(properties.len() as i32).write(writer)?;
    for property in properties {
        write_string(&property.name, MAX_PROPERTY_NAME, writer)?;
        write_string(&property.value, MAX_PROPERTY_VALUE, writer)?;
        property.signature.is_some().write(writer)?;
        if let Some(signature) = &property.signature {
            write_string(signature, MAX_PROPERTY_SIGNATURE, writer)?;
        }
    }
    Ok(())
}

pub(crate) fn read_properties_network(data: &mut Cursor<&[u8]>) -> Result<Vec<ProfileProperty>> {
    let count = VarInt::read(data)?.0;
    let count = usize::try_from(count)
        .map_err(|_| Error::other(format!("Negative profile property count: {count}")))?;
    if count > MAX_PROPERTIES {
        return Err(Error::other("A profile may contain at most 16 properties"));
    }
    let mut properties = Vec::with_capacity(count);
    for _ in 0..count {
        properties.push(ProfileProperty::new(
            read_string(data, MAX_PROPERTY_NAME)?,
            read_string(data, MAX_PROPERTY_VALUE)?,
            if bool::read(data)? {
                Some(read_string(data, MAX_PROPERTY_SIGNATURE)?)
            } else {
                None
            },
        )?);
    }
    Ok(properties)
}

pub(crate) fn write_string(value: &str, max: usize, writer: &mut impl Write) -> Result<()> {
    validate_string(value, max, "Profile string")?;
    value.write_prefixed::<VarInt>(writer)
}

pub(crate) fn read_string(data: &mut Cursor<&[u8]>, max: usize) -> Result<String> {
    let value = String::read_prefixed_bound::<VarInt>(data, max.saturating_mul(3))?;
    validate_string(&value, max, "Profile string")?;
    Ok(value)
}

impl ProfileProperty {
    fn to_nbt_compound(&self) -> NbtCompound {
        let mut compound = NbtCompound::new();
        compound.insert("name", self.name().to_owned());
        compound.insert("value", self.value().to_owned());
        if let Some(signature) = self.signature() {
            compound.insert("signature", signature.to_owned());
        }
        compound
    }

    fn from_nbt_compound(compound: &NbtCompound) -> Option<Self> {
        Self::new(
            compound.get("name")?.string()?.to_string(),
            compound.get("value")?.string()?.to_string(),
            match compound.get("signature") {
                Some(tag) => Some(tag.string()?.to_string()),
                None => None,
            },
        )
        .ok()
    }
}

impl HashComponent for ProfileProperty {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(3);
        push_hash_entry(&mut entries, "name", self.name());
        push_hash_entry(&mut entries, "value", self.value());
        if let Some(signature) = self.signature() {
            push_hash_entry(&mut entries, "signature", signature);
        }
        hash_entries(hasher, &mut entries);
    }
}

pub(crate) fn insert_properties(compound: &mut NbtCompound, properties: &[ProfileProperty]) {
    if !properties.is_empty() {
        compound.insert(
            "properties",
            NbtList::Compound(
                properties
                    .iter()
                    .map(ProfileProperty::to_nbt_compound)
                    .collect(),
            ),
        );
    }
}

pub(crate) fn read_properties(tag: Option<&NbtTag>) -> Option<Vec<ProfileProperty>> {
    let Some(tag) = tag else {
        return Some(Vec::new());
    };
    if let Some(list) = tag.list() {
        let list = list.as_nbt_tags();
        if list.len() > MAX_PROPERTIES {
            return None;
        }
        return list
            .iter()
            .map(|tag| ProfileProperty::from_nbt_compound(tag.compound()?))
            .collect();
    }

    let map = tag.compound()?;
    if map.len() > MAX_PROPERTIES {
        return None;
    }
    let mut properties = Vec::new();
    for (name, values) in map.iter() {
        let values = values.list()?;
        match values {
            NbtList::Empty => {}
            NbtList::String(values) => {
                for value in values {
                    properties.push(
                        ProfileProperty::new(name.to_string(), value.to_string(), None).ok()?,
                    );
                }
            }
            _ => return None,
        }
        if properties.len() > MAX_PROPERTIES {
            return None;
        }
    }
    Some(properties)
}

struct ProfilePropertyList<'a>(&'a [ProfileProperty]);

impl HashComponent for ProfilePropertyList<'_> {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.start_list();
        for property in self.0 {
            hasher.put_component_hash(property);
        }
        hasher.end_list();
    }
}

pub(crate) fn push_properties_hash_entry(
    entries: &mut Vec<HashEntry>,
    properties: &[ProfileProperty],
) {
    if !properties.is_empty() {
        push_hash_entry(entries, "properties", &ProfilePropertyList(properties));
    }
}

pub(crate) fn push_hash_entry<T: HashComponent + ?Sized>(
    entries: &mut Vec<HashEntry>,
    key: &str,
    value: &T,
) {
    let mut key_hasher = ComponentHasher::new();
    key_hasher.put_string(key);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

pub(crate) fn hash_entries(hasher: &mut ComponentHasher, entries: &mut [HashEntry]) {
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

    use uuid::Uuid;

    use super::{GameProfile, ProfileProperty, read_properties_network, write_properties_network};

    #[test]
    fn properties_use_authlib_multimap_order_and_equality() {
        let first = GameProfile::new(
            Uuid::nil(),
            "Alex".to_owned(),
            vec![
                ProfileProperty::new("textures".to_owned(), "first".to_owned(), None)
                    .expect("valid property"),
                ProfileProperty::new("cape".to_owned(), "only".to_owned(), None)
                    .expect("valid property"),
                ProfileProperty::new("textures".to_owned(), "second".to_owned(), None)
                    .expect("valid property"),
            ],
        )
        .expect("valid profile");
        let cross_key_reordered = GameProfile::new(
            Uuid::nil(),
            "Alex".to_owned(),
            vec![
                ProfileProperty::new("cape".to_owned(), "only".to_owned(), None)
                    .expect("valid property"),
                ProfileProperty::new("textures".to_owned(), "first".to_owned(), None)
                    .expect("valid property"),
                ProfileProperty::new("textures".to_owned(), "second".to_owned(), None)
                    .expect("valid property"),
            ],
        )
        .expect("valid profile");
        let same_key_reordered = GameProfile::new(
            Uuid::nil(),
            "Alex".to_owned(),
            vec![
                ProfileProperty::new("textures".to_owned(), "second".to_owned(), None)
                    .expect("valid property"),
                ProfileProperty::new("textures".to_owned(), "first".to_owned(), None)
                    .expect("valid property"),
                ProfileProperty::new("cape".to_owned(), "only".to_owned(), None)
                    .expect("valid property"),
            ],
        )
        .expect("valid profile");

        assert_eq!(
            first
                .properties()
                .iter()
                .map(|property| (property.name(), property.value()))
                .collect::<Vec<_>>(),
            vec![
                ("textures", "first"),
                ("textures", "second"),
                ("cape", "only")
            ]
        );
        assert_eq!(first, cross_key_reordered);
        assert_ne!(first, same_key_reordered);

        let mut network = Vec::new();
        write_properties_network(first.properties(), &mut network)
            .expect("properties should encode");
        let decoded = read_properties_network(&mut Cursor::new(network.as_slice()))
            .expect("properties should decode");
        assert_eq!(decoded, first.properties());
    }
}
