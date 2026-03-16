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

    pub fn register(&mut self, block_entity_type: BlockEntityTypeRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register block entity types after the registry has been frozen"
        );

        let id = self.block_entity_types_by_id.len();
        self.block_entity_types_by_key
            .insert(block_entity_type.key.clone(), id);
        self.block_entity_types_by_id.push(block_entity_type);
        id
    }

    /// Replaces a block_entity_type at a given index.
    /// Returns true if the block_entity_type was replaced and false if the block_entity_type wasn't replaced
    #[must_use]
    pub fn replace(&mut self, block_entity_type: BlockEntityTypeRef, id: usize) -> bool {
        if id >= self.block_entity_types_by_id.len() {
            return false;
        }
        self.block_entity_types_by_id[id] = block_entity_type;
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, BlockEntityTypeRef)> + '_ {
        self.block_entity_types_by_id
            .iter()
            .enumerate()
            .map(|(id, &block_entity_type)| (id, block_entity_type))
    }
}

impl Default for BlockEntityTypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry!(
    BlockEntityTypeRegistry,
    BlockEntityType,
    block_entity_types_by_id,
    block_entity_types_by_key,
    block_entity_types
);
