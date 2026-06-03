use rustc_hash::FxHashMap;
use steel_utils::Identifier;

#[derive(Debug)]
pub struct VillagerType {
    pub key: Identifier,
}

pub type VillagerTypeRef = &'static VillagerType;

pub struct VillagerTypeRegistry {
    villager_types_by_id: Vec<VillagerTypeRef>,
    villager_types_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl VillagerTypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            villager_types_by_id: Vec::new(),
            villager_types_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    VillagerTypeRegistry,
    VillagerTypeRef,
    villager_types_by_id,
    villager_types_by_key,
    allows_registering
);

crate::impl_registry!(
    VillagerTypeRegistry,
    VillagerType,
    villager_types_by_id,
    villager_types_by_key,
    villager_types
);
