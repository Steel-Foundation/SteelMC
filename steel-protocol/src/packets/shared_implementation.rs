use steel_macros::{ReadFrom, WriteTo};

#[derive(Clone, Debug, WriteTo, ReadFrom)]
pub struct KnownPack {
    #[write(as = Prefixed(VarInt))]
    #[read(as = Prefixed(VarInt))]
    pub namespace: String,
    #[write(as = Prefixed(VarInt))]
    #[read(as = Prefixed(VarInt))]
    pub id: String,
    #[write(as = Prefixed(VarInt))]
    #[read(as = Prefixed(VarInt))]
    pub version: String,
}

impl KnownPack {
    #[must_use]
    pub const fn new(namespace: String, id: String, version: String) -> Self {
        Self {
            namespace,
            id,
            version,
        }
    }
}
