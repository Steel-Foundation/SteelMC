//! Vanilla `ResolvableInt` and `ResolvableFloat`.
//!
//! These values appear inside data components that reference a registered
//! context int/float provider, for example `minecraft:brewing_fuel`. Build-time
//! generated vanilla components fold literal providers into [`ResolvableInt::Constant`]
//! and [`ResolvableFloat::Constant`]; providers that need a loot context stay as
//! references because Steel has no runtime context-provider registry yet.

use std::io::{Cursor, Result, Write};

use simdnbt::owned::NbtTag;
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};

/// A literal value or a reference to a registered `minecraft:context_int_provider`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvableInt {
    Constant(i32),
    Reference(Identifier),
}

impl ResolvableInt {
    #[must_use]
    pub const fn constant(value: i32) -> Self {
        Self::Constant(value)
    }

    #[must_use]
    pub const fn reference(key: Identifier) -> Self {
        Self::Reference(key)
    }

    /// Returns the literal value when this is not a provider reference.
    #[must_use]
    pub const fn constant_value(&self) -> Option<i32> {
        match self {
            Self::Constant(value) => Some(*value),
            Self::Reference(_) => None,
        }
    }
}

impl WriteTo for ResolvableInt {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        // `ByteBufCodecs.either` writes a boolean and then the payload.
        match self {
            Self::Constant(value) => {
                true.write(writer)?;
                value.write(writer)
            }
            Self::Reference(key) => {
                false.write(writer)?;
                key.write(writer)
            }
        }
    }
}

impl ReadFrom for ResolvableInt {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        if bool::read(data)? {
            Ok(Self::Constant(i32::read(data)?))
        } else {
            Ok(Self::Reference(Identifier::read(data)?))
        }
    }
}

impl ToNbtTag for ResolvableInt {
    fn to_nbt_tag(self) -> NbtTag {
        match self {
            Self::Constant(value) => NbtTag::Int(value),
            Self::Reference(key) => key.to_nbt_tag(),
        }
    }
}

impl FromNbtTag for ResolvableInt {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        if let Some(value) = tag.codec_i32() {
            return Some(Self::Constant(value));
        }
        Identifier::from_nbt_tag(tag).map(Self::Reference)
    }
}

impl HashComponent for ResolvableInt {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        // `EitherCodec` stores the chosen value inline, so the hash is the
        // literal or the identifier rather than a tagged wrapper.
        match self {
            Self::Constant(value) => value.hash_component(hasher),
            Self::Reference(key) => key.hash_component(hasher),
        }
    }
}

/// A literal value or a reference to a registered `minecraft:context_float_provider`.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvableFloat {
    Constant(f32),
    Reference(Identifier),
}

impl ResolvableFloat {
    #[must_use]
    pub const fn constant(value: f32) -> Self {
        Self::Constant(value)
    }

    #[must_use]
    pub const fn reference(key: Identifier) -> Self {
        Self::Reference(key)
    }

    /// Returns the literal value when this is not a provider reference.
    #[must_use]
    pub const fn constant_value(&self) -> Option<f32> {
        match self {
            Self::Constant(value) => Some(*value),
            Self::Reference(_) => None,
        }
    }
}

impl WriteTo for ResolvableFloat {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        match self {
            Self::Constant(value) => {
                true.write(writer)?;
                value.write(writer)
            }
            Self::Reference(key) => {
                false.write(writer)?;
                key.write(writer)
            }
        }
    }
}

impl ReadFrom for ResolvableFloat {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        if bool::read(data)? {
            Ok(Self::Constant(f32::read(data)?))
        } else {
            Ok(Self::Reference(Identifier::read(data)?))
        }
    }
}

impl ToNbtTag for ResolvableFloat {
    fn to_nbt_tag(self) -> NbtTag {
        match self {
            Self::Constant(value) => NbtTag::Float(value),
            Self::Reference(key) => key.to_nbt_tag(),
        }
    }
}

impl FromNbtTag for ResolvableFloat {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        if let Some(value) = tag.codec_f32() {
            return Some(Self::Constant(value));
        }
        Identifier::from_nbt_tag(tag).map(Self::Reference)
    }
}

impl HashComponent for ResolvableFloat {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        match self {
            Self::Constant(value) => value.hash_component(hasher),
            Self::Reference(key) => key.hash_component(hasher),
        }
    }
}
