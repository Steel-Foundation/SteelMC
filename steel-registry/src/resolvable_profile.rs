//! Vanilla resolvable player profiles used by item components and entity data.

use std::io::{Cursor, Error, Result, Write};
use std::ops::Deref;

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::UuidExt as _;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::serial::{PrefixedRead as _, PrefixedWrite as _, ReadFrom, WriteTo};
use uuid::Uuid;

const MAX_PROPERTIES: usize = 16;
const MAX_PROPERTY_NAME: usize = 64;
const MAX_PROPERTY_VALUE: usize = 32_767;
const MAX_PROPERTY_SIGNATURE: usize = 1_024;
const MAX_PLAYER_NAME: usize = 16;

/// One authlib game-profile property.
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

    fn to_nbt_compound(&self) -> NbtCompound {
        let mut compound = NbtCompound::new();
        compound.insert("name", self.name.clone());
        compound.insert("value", self.value.clone());
        if let Some(signature) = &self.signature {
            compound.insert("signature", signature.clone());
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
        push_hash_entry(&mut entries, "name", &self.name);
        push_hash_entry(&mut entries, "value", &self.value);
        if let Some(signature) = &self.signature {
            push_hash_entry(&mut entries, "signature", signature);
        }
        hash_entries(hasher, &mut entries);
    }
}

/// Authlib property-map values grouped by first key occurrence.
#[derive(Debug, Default, Clone)]
struct ProfileProperties(Vec<ProfileProperty>);

impl ProfileProperties {
    fn new(properties: Vec<ProfileProperty>) -> Result<Self> {
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

/// Complete authlib profile contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredGameProfile {
    id: Uuid,
    name: String,
    properties: ProfileProperties,
}

impl StoredGameProfile {
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
}

/// Profile fields that may still require resolution.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PartialProfile {
    name: Option<String>,
    id: Option<Uuid>,
    properties: ProfileProperties,
}

impl PartialProfile {
    pub fn new(
        name: Option<String>,
        id: Option<Uuid>,
        properties: Vec<ProfileProperty>,
    ) -> Result<Self> {
        if let Some(name) = &name {
            validate_player_name(name)?;
        }
        Ok(Self {
            name,
            id,
            properties: ProfileProperties::new(properties)?,
        })
    }

    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    #[must_use]
    pub const fn id(&self) -> Option<Uuid> {
        self.id
    }

    #[must_use]
    pub fn properties(&self) -> &[ProfileProperty] {
        &self.properties
    }
}

/// Optional resource-pack skin model override.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PlayerModelType {
    Slim,
    #[default]
    Wide,
}

impl PlayerModelType {
    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Slim => "slim",
            Self::Wide => "wide",
        }
    }

    const fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "slim" => Some(Self::Slim),
            "wide" => Some(Self::Wide),
            _ => None,
        }
    }
}

/// Resource-pack skin assets applied over a resolved profile.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PlayerSkinPatch {
    texture: Option<Identifier>,
    cape: Option<Identifier>,
    elytra: Option<Identifier>,
    model: Option<PlayerModelType>,
}

impl PlayerSkinPatch {
    #[must_use]
    pub const fn new(
        texture: Option<Identifier>,
        cape: Option<Identifier>,
        elytra: Option<Identifier>,
        model: Option<PlayerModelType>,
    ) -> Self {
        Self {
            texture,
            cape,
            elytra,
            model,
        }
    }

    #[must_use]
    pub const fn texture(&self) -> Option<&Identifier> {
        self.texture.as_ref()
    }

    #[must_use]
    pub const fn cape(&self) -> Option<&Identifier> {
        self.cape.as_ref()
    }

    #[must_use]
    pub const fn elytra(&self) -> Option<&Identifier> {
        self.elytra.as_ref()
    }

    #[must_use]
    pub const fn model(&self) -> Option<PlayerModelType> {
        self.model
    }
}

/// Resolved, partial, or dynamically resolvable profile contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvableProfileContents {
    DynamicName(String),
    DynamicId(Uuid),
    StaticFull(StoredGameProfile),
    StaticPartial(PartialProfile),
}

/// Profile component with an optional resource-pack skin patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvableProfile {
    contents: ResolvableProfileContents,
    skin_patch: PlayerSkinPatch,
}

impl Default for ResolvableProfile {
    fn default() -> Self {
        Self::static_partial(PartialProfile::default(), PlayerSkinPatch::default())
    }
}

impl ResolvableProfile {
    pub fn dynamic_name(name: String, skin_patch: PlayerSkinPatch) -> Result<Self> {
        validate_player_name(&name)?;
        Ok(Self {
            contents: ResolvableProfileContents::DynamicName(name),
            skin_patch,
        })
    }

    #[must_use]
    pub const fn dynamic_id(id: Uuid, skin_patch: PlayerSkinPatch) -> Self {
        Self {
            contents: ResolvableProfileContents::DynamicId(id),
            skin_patch,
        }
    }

    #[must_use]
    pub const fn static_full(profile: StoredGameProfile, skin_patch: PlayerSkinPatch) -> Self {
        Self {
            contents: ResolvableProfileContents::StaticFull(profile),
            skin_patch,
        }
    }

    #[must_use]
    pub const fn static_partial(profile: PartialProfile, skin_patch: PlayerSkinPatch) -> Self {
        Self {
            contents: ResolvableProfileContents::StaticPartial(profile),
            skin_patch,
        }
    }

    #[must_use]
    pub const fn contents(&self) -> &ResolvableProfileContents {
        &self.contents
    }

    #[must_use]
    pub const fn skin_patch(&self) -> &PlayerSkinPatch {
        &self.skin_patch
    }

    fn from_partial(profile: PartialProfile, skin_patch: PlayerSkinPatch) -> Self {
        if profile.properties.is_empty() {
            match (&profile.name, profile.id) {
                (Some(name), None) => {
                    return Self {
                        contents: ResolvableProfileContents::DynamicName(name.clone()),
                        skin_patch,
                    };
                }
                (None, Some(id)) => return Self::dynamic_id(id, skin_patch),
                (Some(_), Some(_)) | (None, None) => {}
            }
        }
        Self::static_partial(profile, skin_patch)
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        match &self.contents {
            ResolvableProfileContents::StaticFull(profile) => {
                compound.insert("id", uuid_to_nbt(profile.id));
                compound.insert("name", profile.name.clone());
                insert_properties(&mut compound, &profile.properties);
            }
            ResolvableProfileContents::DynamicName(name) => {
                compound.insert("name", name.clone());
            }
            ResolvableProfileContents::DynamicId(id) => {
                compound.insert("id", uuid_to_nbt(*id));
            }
            ResolvableProfileContents::StaticPartial(profile) => {
                if let Some(name) = &profile.name {
                    compound.insert("name", name.clone());
                }
                if let Some(id) = profile.id {
                    compound.insert("id", uuid_to_nbt(id));
                }
                insert_properties(&mut compound, &profile.properties);
            }
        }
        insert_skin_patch(&mut compound, &self.skin_patch);
        NbtTag::Compound(compound)
    }

    fn from_owned_nbt(tag: &NbtTag) -> Option<Self> {
        if let Some(name) = tag.string() {
            return Self::dynamic_name(name.to_string(), PlayerSkinPatch::default()).ok();
        }

        let compound = tag.compound()?;
        let name = match compound.get("name") {
            Some(tag) => Some(tag.string()?.to_string()),
            None => None,
        };
        let id = match compound.get("id") {
            Some(tag) => Some(uuid_from_nbt(tag)?),
            None => None,
        };
        let properties = read_properties(compound.get("properties"))?;
        let skin_patch = read_skin_patch(compound)?;

        if let (Some(id), Some(name)) = (id, &name) {
            return Some(Self::static_full(
                StoredGameProfile::new(id, name.clone(), properties).ok()?,
                skin_patch,
            ));
        }
        let partial = PartialProfile::new(name, id, properties).ok()?;
        Some(Self::from_partial(partial, skin_patch))
    }
}

impl WriteTo for ResolvableProfile {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        match &self.contents {
            ResolvableProfileContents::StaticFull(profile) => {
                true.write(writer)?;
                write_full_profile(profile, writer)?;
            }
            ResolvableProfileContents::DynamicName(name) => {
                false.write(writer)?;
                write_partial_profile_fields(Some(name), None, &[], writer)?;
            }
            ResolvableProfileContents::DynamicId(id) => {
                false.write(writer)?;
                write_partial_profile_fields(None, Some(*id), &[], writer)?;
            }
            ResolvableProfileContents::StaticPartial(profile) => {
                false.write(writer)?;
                write_partial_profile(profile, writer)?;
            }
        }
        write_skin_patch(&self.skin_patch, writer)
    }
}

impl ReadFrom for ResolvableProfile {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let contents = if bool::read(data)? {
            ResolvableProfileContents::StaticFull(read_full_profile(data)?)
        } else {
            let partial = read_partial_profile(data)?;
            let patch = read_skin_patch_network(data)?;
            return Ok(Self::from_partial(partial, patch));
        };
        Ok(Self {
            contents,
            skin_patch: read_skin_patch_network(data)?,
        })
    }
}

impl ToNbtTag for ResolvableProfile {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for ResolvableProfile {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Self::from_owned_nbt(&tag.to_owned())
    }
}

impl HashComponent for ResolvableProfile {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        match &self.contents {
            ResolvableProfileContents::StaticFull(profile) => {
                push_uuid_hash_entry(&mut entries, "id", profile.id);
                push_hash_entry(&mut entries, "name", &profile.name);
                push_properties_hash_entry(&mut entries, &profile.properties);
            }
            ResolvableProfileContents::DynamicName(name) => {
                push_hash_entry(&mut entries, "name", name);
            }
            ResolvableProfileContents::DynamicId(id) => {
                push_uuid_hash_entry(&mut entries, "id", *id);
            }
            ResolvableProfileContents::StaticPartial(profile) => {
                if let Some(name) = &profile.name {
                    push_hash_entry(&mut entries, "name", name);
                }
                if let Some(id) = profile.id {
                    push_uuid_hash_entry(&mut entries, "id", id);
                }
                push_properties_hash_entry(&mut entries, &profile.properties);
            }
        }
        if let Some(texture) = &self.skin_patch.texture {
            push_hash_entry(&mut entries, "texture", texture);
        }
        if let Some(cape) = &self.skin_patch.cape {
            push_hash_entry(&mut entries, "cape", cape);
        }
        if let Some(elytra) = &self.skin_patch.elytra {
            push_hash_entry(&mut entries, "elytra", elytra);
        }
        if let Some(model) = self.skin_patch.model {
            push_hash_entry(&mut entries, "model", model.serialized_name());
        }
        hash_entries(hasher, &mut entries);
    }
}

fn validate_player_name(name: &str) -> Result<()> {
    if name.encode_utf16().count() > MAX_PLAYER_NAME
        || name
            .chars()
            .any(|character| character <= ' ' || character >= '\u{7f}')
    {
        return Err(Error::other("Invalid player profile name"));
    }
    Ok(())
}

fn validate_properties(properties: &[ProfileProperty]) -> Result<()> {
    if properties.len() > MAX_PROPERTIES {
        return Err(Error::other("A profile may contain at most 16 properties"));
    }
    Ok(())
}

fn validate_string(value: &str, max: usize, name: &str) -> Result<()> {
    if value.encode_utf16().count() > max || value.len() > max.saturating_mul(3) {
        return Err(Error::other(format!(
            "{name} exceeds its {max}-character limit"
        )));
    }
    Ok(())
}

fn uuid_to_nbt(id: Uuid) -> NbtTag {
    NbtTag::IntArray(id.to_int_array().to_vec())
}

fn uuid_from_nbt(tag: &NbtTag) -> Option<Uuid> {
    Uuid::from_int_array(tag.int_array()?)
}

fn insert_properties(compound: &mut NbtCompound, properties: &[ProfileProperty]) {
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

fn read_properties(tag: Option<&NbtTag>) -> Option<Vec<ProfileProperty>> {
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

fn insert_skin_patch(compound: &mut NbtCompound, patch: &PlayerSkinPatch) {
    if let Some(texture) = &patch.texture {
        compound.insert("texture", texture.to_string());
    }
    if let Some(cape) = &patch.cape {
        compound.insert("cape", cape.to_string());
    }
    if let Some(elytra) = &patch.elytra {
        compound.insert("elytra", elytra.to_string());
    }
    if let Some(model) = patch.model {
        compound.insert("model", model.serialized_name());
    }
}

fn read_skin_patch(compound: &NbtCompound) -> Option<PlayerSkinPatch> {
    Some(PlayerSkinPatch::new(
        read_optional_identifier(compound.get("texture"))?,
        read_optional_identifier(compound.get("cape"))?,
        read_optional_identifier(compound.get("elytra"))?,
        match compound.get("model") {
            Some(tag) => Some(PlayerModelType::from_serialized_name(
                &tag.string()?.to_string(),
            )?),
            None => None,
        },
    ))
}

#[expect(
    clippy::option_option,
    reason = "the outer option reports codec failure while the inner option represents an absent field"
)]
fn read_optional_identifier(tag: Option<&NbtTag>) -> Option<Option<Identifier>> {
    match tag {
        Some(tag) => Some(Some(tag.string()?.to_string().parse().ok()?)),
        None => Some(None),
    }
}

fn write_full_profile(profile: &StoredGameProfile, writer: &mut impl Write) -> Result<()> {
    profile.id.write(writer)?;
    write_string(&profile.name, MAX_PLAYER_NAME, writer)?;
    write_properties_network(&profile.properties, writer)
}

fn read_full_profile(data: &mut Cursor<&[u8]>) -> Result<StoredGameProfile> {
    StoredGameProfile::new(
        Uuid::read(data)?,
        read_string(data, MAX_PLAYER_NAME)?,
        read_properties_network(data)?,
    )
}

fn write_partial_profile(profile: &PartialProfile, writer: &mut impl Write) -> Result<()> {
    write_partial_profile_fields(
        profile.name.as_deref(),
        profile.id,
        &profile.properties,
        writer,
    )
}

fn write_partial_profile_fields(
    name: Option<&str>,
    id: Option<Uuid>,
    properties: &[ProfileProperty],
    writer: &mut impl Write,
) -> Result<()> {
    name.is_some().write(writer)?;
    if let Some(name) = name {
        write_string(name, MAX_PLAYER_NAME, writer)?;
    }
    id.is_some().write(writer)?;
    if let Some(id) = id {
        id.write(writer)?;
    }
    write_properties_network(properties, writer)
}

fn read_partial_profile(data: &mut Cursor<&[u8]>) -> Result<PartialProfile> {
    let name = if bool::read(data)? {
        Some(read_string(data, MAX_PLAYER_NAME)?)
    } else {
        None
    };
    let id = if bool::read(data)? {
        Some(Uuid::read(data)?)
    } else {
        None
    };
    PartialProfile::new(name, id, read_properties_network(data)?)
}

fn write_properties_network(properties: &[ProfileProperty], writer: &mut impl Write) -> Result<()> {
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

fn read_properties_network(data: &mut Cursor<&[u8]>) -> Result<Vec<ProfileProperty>> {
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

fn write_skin_patch(patch: &PlayerSkinPatch, writer: &mut impl Write) -> Result<()> {
    for texture in [&patch.texture, &patch.cape, &patch.elytra] {
        texture.is_some().write(writer)?;
        if let Some(texture) = texture {
            texture.write(writer)?;
        }
    }
    patch.model.is_some().write(writer)?;
    if let Some(model) = patch.model {
        (model == PlayerModelType::Slim).write(writer)?;
    }
    Ok(())
}

fn read_skin_patch_network(data: &mut Cursor<&[u8]>) -> Result<PlayerSkinPatch> {
    let texture = read_optional_identifier_network(data)?;
    let cape = read_optional_identifier_network(data)?;
    let elytra = read_optional_identifier_network(data)?;
    let model = if bool::read(data)? {
        Some(if bool::read(data)? {
            PlayerModelType::Slim
        } else {
            PlayerModelType::Wide
        })
    } else {
        None
    };
    Ok(PlayerSkinPatch::new(texture, cape, elytra, model))
}

fn read_optional_identifier_network(data: &mut Cursor<&[u8]>) -> Result<Option<Identifier>> {
    if bool::read(data)? {
        Ok(Some(Identifier::read(data)?))
    } else {
        Ok(None)
    }
}

fn write_string(value: &str, max: usize, writer: &mut impl Write) -> Result<()> {
    validate_string(value, max, "Profile string")?;
    value.write_prefixed::<VarInt>(writer)
}

fn read_string(data: &mut Cursor<&[u8]>, max: usize) -> Result<String> {
    let value = String::read_prefixed_bound::<VarInt>(data, max.saturating_mul(3))?;
    validate_string(&value, max, "Profile string")?;
    Ok(value)
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

fn push_uuid_hash_entry(entries: &mut Vec<HashEntry>, key: &str, id: Uuid) {
    let values = id.to_int_array();
    let mut key_hasher = ComponentHasher::new();
    key_hasher.put_string(key);
    let mut value_hasher = ComponentHasher::new();
    value_hasher.put_int_array(&values);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

fn push_properties_hash_entry(entries: &mut Vec<HashEntry>, properties: &[ProfileProperty]) {
    if !properties.is_empty() {
        push_hash_entry(entries, "properties", &ProfilePropertyList(properties));
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

    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::Identifier;
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};
    use uuid::Uuid;

    use super::{
        PlayerModelType, PlayerSkinPatch, ProfileProperty, ResolvableProfile,
        ResolvableProfileContents, StoredGameProfile, read_properties_network,
        write_properties_network,
    };

    fn parse(tag: simdnbt::owned::NbtTag) -> Option<ResolvableProfile> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        ResolvableProfile::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn full_profile_round_trips_both_codecs_and_hashes_primary_shape() {
        let profile = ResolvableProfile::static_full(
            StoredGameProfile::new(
                Uuid::from_u128(1),
                "Steve".to_owned(),
                vec![
                    ProfileProperty::new(
                        "textures".to_owned(),
                        "value".to_owned(),
                        Some("signature".to_owned()),
                    )
                    .expect("valid property"),
                ],
            )
            .expect("valid profile"),
            PlayerSkinPatch::new(
                Some(Identifier::vanilla_static("steve")),
                None,
                None,
                Some(PlayerModelType::Wide),
            ),
        );
        let nbt = profile.clone().to_nbt_tag();
        assert_eq!(parse(nbt.clone()), Some(profile.clone()));
        assert_eq!(profile.compute_hash(), nbt.compute_hash());

        let mut network = Vec::new();
        profile.write(&mut network).expect("profile should encode");
        assert_eq!(
            ResolvableProfile::read(&mut Cursor::new(network.as_slice()))
                .expect("profile should decode"),
            profile
        );
    }

    #[test]
    fn player_name_alternative_normalizes_to_dynamic_profile() {
        let parsed = parse(simdnbt::owned::NbtTag::String("Alex".into()))
            .expect("name alternative should decode");
        assert_eq!(
            parsed.contents(),
            &ResolvableProfileContents::DynamicName("Alex".to_owned())
        );
        assert_eq!(parse(parsed.clone().to_nbt_tag()), Some(parsed));
    }

    #[test]
    fn properties_use_authlib_multimap_order_and_equality() {
        let first = StoredGameProfile::new(
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
        let cross_key_reordered = StoredGameProfile::new(
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
        let same_key_reordered = StoredGameProfile::new(
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

    #[test]
    fn profile_limits_prevent_values_that_cannot_be_reencoded() {
        assert!(
            ResolvableProfile::dynamic_name("a".repeat(17), PlayerSkinPatch::default()).is_err()
        );
        let properties = (0..17)
            .map(|index| {
                ProfileProperty::new(format!("p{index}"), "value".to_owned(), None)
                    .expect("test property should be valid")
            })
            .collect();
        assert!(StoredGameProfile::new(Uuid::nil(), "A".to_owned(), properties).is_err());
    }
}
