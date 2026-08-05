use simdnbt::owned::NbtTag;
use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::config::C_REGISTRY_DATA;
use steel_utils::Identifier;

#[derive(Clone, Debug, WriteTo)]
pub struct RegistryEntry {
    pub id: Identifier,
    pub data: Option<NbtTag>,
}

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Config = C_REGISTRY_DATA)]
pub struct CRegistryData {
    pub registry: Identifier,
    #[write(as = Prefixed(VarInt))]
    pub entries: Vec<RegistryEntry>,
}

impl CRegistryData {
    #[must_use]
    pub const fn new(registry: Identifier, entries: Vec<RegistryEntry>) -> Self {
        Self { registry, entries }
    }
}

impl RegistryEntry {
    #[must_use]
    pub const fn new(id: Identifier, data: Option<NbtTag>) -> Self {
        Self { id, data }
    }
}
