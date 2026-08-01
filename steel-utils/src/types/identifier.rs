use std::{
    borrow::Cow,
    cmp::Ordering,
    fmt::{self, Debug, Display, Formatter},
    mem::MaybeUninit,
    str::FromStr,
};

use serde::{Deserialize, Serialize, de::Error as _};
use simdnbt::owned::NbtTag;
use wincode::{SchemaRead, SchemaWrite, config::Config, io::Reader, io::Writer};

use crate::hash::{ComponentHasher, HashComponent};

/// An identifier used by Minecraft.
#[derive(Clone, PartialEq, Eq, Hash, Default)]
pub struct Identifier {
    /// The namespace of the identifier.
    pub namespace: Cow<'static, str>,
    /// The path of the identifier.
    pub path: Cow<'static, str>,
}

impl Debug for Identifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&format!("{}:{}", self.namespace, self.path))
    }
}

impl Identifier {
    /// The vanilla namespace.
    pub const VANILLA_NAMESPACE: &'static str = "minecraft";
    /// The Steel namespace.
    pub const STEEL_NAMESPACE: &'static str = "steel";

    /// Creates a new `Identifier` with the given namespace and path.
    #[must_use]
    pub fn new(
        namespace: impl Into<Cow<'static, str>>,
        path: impl Into<Cow<'static, str>>,
    ) -> Self {
        Identifier {
            namespace: namespace.into(),
            path: path.into(),
        }
    }
    #[must_use]
    pub const fn new_static(namespace: &'static str, path: &'static str) -> Self {
        Identifier {
            namespace: Cow::Borrowed(namespace),
            path: Cow::Borrowed(path),
        }
    }

    /// Creates a new `Identifier` with the Steel namespace.
    #[must_use]
    pub fn from_steel(path: impl Into<Cow<'static, str>>) -> Self {
        Self::new(Self::STEEL_NAMESPACE, path)
    }

    /// Creates a new `Identifier` with the vanilla namespace.
    #[must_use]
    pub const fn vanilla(path: String) -> Self {
        Identifier {
            namespace: Cow::Borrowed(Self::VANILLA_NAMESPACE),
            path: Cow::Owned(path),
        }
    }

    /// Creates a new `Identifier` with the vanilla namespace and a static path.
    #[must_use]
    pub const fn vanilla_static(path: &'static str) -> Self {
        Identifier {
            namespace: Cow::Borrowed(Self::VANILLA_NAMESPACE),
            path: Cow::Borrowed(path),
        }
    }

    /// Returns whether the character is a valid namespace character.
    #[must_use]
    pub const fn valid_namespace_char(char: char) -> bool {
        char == '_'
            || char == '-'
            || char.is_ascii_lowercase()
            || char.is_ascii_digit()
            || char == '.'
    }

    /// Returns whether the character is a valid path character.
    #[must_use]
    pub const fn valid_char(char: char) -> bool {
        Self::valid_namespace_char(char) || char == '/'
    }

    /// Returns whether the namespace is valid.
    pub fn validate_namespace(namespace: &str) -> bool {
        namespace != ".." && namespace.chars().all(Self::valid_namespace_char)
    }

    /// Returns whether the path is valid.
    pub fn validate_path(path: &str) -> bool {
        path.chars().all(Self::valid_char)
    }

    /// Returns whether the namespace and path are valid.
    #[must_use]
    pub fn validate(namespace: &str, path: &str) -> bool {
        Self::validate_namespace(namespace) && Self::validate_path(path)
    }
}

impl Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.path)
    }
}

impl PartialOrd for Identifier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Identifier {
    fn cmp(&self, other: &Self) -> Ordering {
        self.path
            .cmp(&other.path)
            .then_with(|| self.namespace.cmp(&other.namespace))
    }
}

impl FromStr for Identifier {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (namespace, path) = match s.split_once(':') {
            Some(("", path)) => (Self::VANILLA_NAMESPACE, path),
            Some((namespace, path)) => (namespace, path),
            None => (Self::VANILLA_NAMESPACE, s),
        };

        if !Identifier::validate_namespace(namespace) {
            return Err("Invalid namespace");
        }

        if !Identifier::validate_path(path) {
            return Err("Invalid path");
        }

        Ok(Identifier {
            namespace: Cow::Owned(namespace.to_owned()),
            path: Cow::Owned(path.to_owned()),
        })
    }
}
impl Serialize for Identifier {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Identifier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Identifier::from_str(&s).map_err(D::Error::custom)
    }
}

// SAFETY: This implementation delegates to the `str` and `String` implementations
// which are already safe, and the Identifier type has the same serialized representation
// as a String (length-prefixed UTF-8 bytes). The size_of method returns exactly the
// number of bytes that write will produce.
unsafe impl<C: Config> SchemaWrite<C> for Identifier {
    type Src = Identifier;

    fn size_of(src: &Self::Src) -> wincode::WriteResult<usize> {
        <str as SchemaWrite<C>>::size_of(&src.to_string())
    }

    fn write(writer: impl Writer, src: &Self::Src) -> wincode::WriteResult<()> {
        <str as SchemaWrite<C>>::write(writer, &src.to_string())
    }
}

// SAFETY: This implementation delegates to the `String` implementation which is
// already safe, and then validates the result as a valid Identifier. The read
// method initializes `dst` if and only if it returns Ok(()).
unsafe impl<'de, C: Config> SchemaRead<'de, C> for Identifier {
    type Dst = Identifier;

    fn read(reader: impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> wincode::ReadResult<()> {
        let mut s = MaybeUninit::<String>::uninit();
        <String as SchemaRead<'de, C>>::read(reader, &mut s)?;

        // SAFETY: String::read succeeded, so s is initialized
        let s = unsafe { s.assume_init() };

        dst.write(Identifier::from_str(&s).map_err(wincode::ReadError::Custom)?);
        Ok(())
    }
}

impl HashComponent for Identifier {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        // Identifiers are hashed as strings in "namespace:path" format
        hasher.put_string(&self.to_string());
    }
}

impl simdnbt::ToNbtTag for Identifier {
    fn to_nbt_tag(self) -> NbtTag {
        NbtTag::String(self.to_string().into())
    }
}

impl simdnbt::FromNbtTag for Identifier {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let s = tag.string()?.to_str();
        s.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::Identifier;

    #[test]
    fn ordering_matches_vanilla_identifier_compare_to() {
        let path_first = Identifier::new_static("z", "apple");
        let namespace_first = Identifier::new_static("a", "banana");
        let same_path_later_namespace = Identifier::new_static("z", "banana");

        assert!(path_first < namespace_first);
        assert!(namespace_first < same_path_later_namespace);
    }
}
