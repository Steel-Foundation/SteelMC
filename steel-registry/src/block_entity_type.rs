use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// Represents a block entity type in Minecraft.
/// Block entities are used for blocks that need to store additional data
/// beyond their block state, such as chests, furnaces, signs, etc.
#[derive(Debug)]
pub struct BlockEntityType {
    pub key: Identifier,
}

pub type BlockEntityTypeRef = &'static BlockEntityType;

pub struct BlockEntityTypeRegistry {
    block_entity_types_by_id: Vec<BlockEntityTypeRef>,
    block_entity_types_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl BlockEntityTypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            block_entity_types_by_id: Vec::new(),
            block_entity_types_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    BlockEntityTypeRegistry,
    BlockEntityTypeRef,
    block_entity_types_by_id,
    block_entity_types_by_key,
    allows_registering
);

crate::impl_registry!(
    BlockEntityTypeRegistry,
    BlockEntityType,
    block_entity_types_by_id,
    block_entity_types_by_key,
    block_entity_types
);
