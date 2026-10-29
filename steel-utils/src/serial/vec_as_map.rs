mod vec_as_map {
    use serde::de::{MapAccess, Visitor};
    use serde::ser::SerializeMap;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::fmt;
    use std::marker::PhantomData;

    pub fn serialize<S, K, V>(items: &Vec<(K, V)>, serializer: S) -> Result<S::Ok, S::Error>
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
                f.write_str("une map")
            }

            fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut items = Vec::new();
                // next_entry lit les paires dans l'ordre du texte JSON
                while let Some((k, v)) = access.next_entry()? {
                    items.push((k, v));
                }
                Ok(items)
            }
        }

        deserializer.deserialize_map(VecVisitor(PhantomData))
    }
}
