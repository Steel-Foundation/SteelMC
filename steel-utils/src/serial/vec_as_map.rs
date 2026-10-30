//! Serde helpers that encode a `Vec<(K, V)>` as a map.

use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::marker::PhantomData;

/// Serializes items as map entries, preserving their order.
pub fn serialize<S, K, V>(items: &[(K, V)], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    K: Serialize,
    V: Serialize,
{
    let mut map = serializer.serialize_map(Some(items.len()))?;
    for (k, v) in items {
        map.serialize_entry(k, v)?;
    }
    map.end()
}

/// Deserializes map entries into items, preserving their order.
pub fn deserialize<'de, D, K, V>(deserializer: D) -> Result<Vec<(K, V)>, D::Error>
where
    D: Deserializer<'de>,
    K: Deserialize<'de>,
    V: Deserialize<'de>,
{
    struct VecVisitor<K, V>(PhantomData<(K, V)>);

    impl<'de, K, V> Visitor<'de> for VecVisitor<K, V>
    where
        K: Deserialize<'de>,
        V: Deserialize<'de>,
    {
        type Value = Vec<(K, V)>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a map")
        }

        fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut items = Vec::new();
            // `next_entry` yields pairs in the order they appear in the JSON map.
            while let Some((k, v)) = access.next_entry()? {
                items.push((k, v));
            }
            Ok(items)
        }
    }

    deserializer.deserialize_map(VecVisitor(PhantomData))
}
