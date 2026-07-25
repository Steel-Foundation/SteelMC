//! Registry holders used by Vanilla registry-aware codecs.

use std::fmt::Debug;
use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::RegistryEntry;

/// Registry operations and direct value type required by [`RegistryHolder`].
pub trait RegistryHolderEntry: RegistryEntry + Debug + Send + Sync {
    /// The value stored in the registry entry and carried by a direct holder.
    type Value: Clone
        + Debug
        + PartialEq
        + Send
        + Sync
        + WriteTo
        + ReadFrom
        + ToNbtTag
        + FromNbtTag
        + HashComponent
        + 'static;

    /// Human-readable registry name used in codec errors.
    const REGISTRY_NAME: &'static str;

    /// Returns the entry's registry-independent value.
    fn holder_value(&self) -> &Self::Value;

    /// Looks up a registry reference by its protocol ID.
    fn holder_by_id(id: usize) -> Option<&'static Self>;

    /// Looks up a registry reference by its key.
    fn holder_by_key(key: &Identifier) -> Option<&'static Self>;
}

/// Vanilla `Holder<T>` for registries whose codecs permit inline values.
#[derive(Debug)]
pub enum RegistryHolder<T: RegistryHolderEntry> {
    /// A value owned by the target registry.
    Reference(&'static T),
    /// A complete value embedded directly in the holder.
    Direct(T::Value),
}

impl<T: RegistryHolderEntry> Clone for RegistryHolder<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Reference(value) => Self::Reference(value),
            Self::Direct(value) => Self::Direct(value.clone()),
        }
    }
}

impl<T: RegistryHolderEntry> PartialEq for RegistryHolder<T> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Reference(left), Self::Reference(right)) => *left == *right,
            (Self::Direct(left), Self::Direct(right)) => left == right,
            (Self::Reference(_), Self::Direct(_)) | (Self::Direct(_), Self::Reference(_)) => false,
        }
    }
}

impl<T: RegistryHolderEntry> RegistryHolder<T> {
    #[must_use]
    pub const fn reference(value: &'static T) -> Self {
        Self::Reference(value)
    }

    #[must_use]
    pub const fn direct(value: T::Value) -> Self {
        Self::Direct(value)
    }

    #[must_use]
    pub fn value(&self) -> &T::Value {
        match self {
            Self::Reference(value) => value.holder_value(),
            Self::Direct(value) => value,
        }
    }

    #[must_use]
    pub const fn as_reference(&self) -> Option<&'static T> {
        match self {
            Self::Reference(value) => Some(value),
            Self::Direct(_) => None,
        }
    }

    #[must_use]
    pub const fn as_direct(&self) -> Option<&T::Value> {
        match self {
            Self::Reference(_) => None,
            Self::Direct(value) => Some(value),
        }
    }
}

impl<T: RegistryHolderEntry> WriteTo for RegistryHolder<T> {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        match self {
            Self::Reference(value) => {
                let id = value.try_id().ok_or_else(|| {
                    Error::other(format!("Unknown {}: {}", T::REGISTRY_NAME, value.key()))
                })?;
                let id = i32::try_from(id).map_err(|_| {
                    Error::other(format!(
                        "{} id out of protocol range: {id}",
                        T::REGISTRY_NAME
                    ))
                })?;
                let encoded_id = id.checked_add(1).ok_or_else(|| {
                    Error::other(format!("{} id exceeds protocol range", T::REGISTRY_NAME))
                })?;
                VarInt(encoded_id).write(writer)
            }
            Self::Direct(value) => {
                VarInt(0).write(writer)?;
                value.write(writer)
            }
        }
    }
}

impl<T: RegistryHolderEntry> ReadFrom for RegistryHolder<T> {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let encoded_id = VarInt::read(data)?.0;
        if encoded_id == 0 {
            return T::Value::read(data).map(Self::Direct);
        }

        let id = encoded_id
            .checked_sub(1)
            .and_then(|id| usize::try_from(id).ok())
            .ok_or_else(|| {
                Error::other(format!(
                    "Invalid {} holder id: {encoded_id}",
                    T::REGISTRY_NAME
                ))
            })?;
        T::holder_by_id(id).map(Self::Reference).ok_or_else(|| {
            Error::other(format!(
                "Unknown {} holder id: {encoded_id}",
                T::REGISTRY_NAME
            ))
        })
    }
}

impl<T: RegistryHolderEntry> ToNbtTag for RegistryHolder<T> {
    fn to_nbt_tag(self) -> simdnbt::owned::NbtTag {
        match self {
            Self::Reference(value) => value.key().to_string().to_nbt_tag(),
            Self::Direct(value) => value.to_nbt_tag(),
        }
    }
}

impl<T: RegistryHolderEntry> FromNbtTag for RegistryHolder<T> {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        if let Some(value) = tag.string() {
            let key = Identifier::from_str(&value.to_str()).ok()?;
            return T::holder_by_key(&key).map(Self::Reference);
        }

        T::Value::from_nbt_tag(tag).map(Self::Direct)
    }
}

impl<T: RegistryHolderEntry> HashComponent for RegistryHolder<T> {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        match self {
            Self::Reference(value) => value.key().to_string().hash_component(hasher),
            Self::Direct(value) => value.hash_component(hasher),
        }
    }
}
