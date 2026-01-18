use rustc_hash::FxHashMap;
use steel_utils::Identifier;

use crate::RegistryExt;

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

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<BlockEntityTypeRef> {
        self.block_entity_types_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, block_entity_type: BlockEntityTypeRef) -> &usize {
        self.block_entity_types_by_key
            .get(&block_entity_type.key)
            .expect("Block entity type not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<BlockEntityTypeRef> {
        self.block_entity_types_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, BlockEntityTypeRef)> + '_ {
        self.block_entity_types_by_id
            .iter()
            .enumerate()
            .map(|(id, &block_entity_type)| (id, block_entity_type))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.block_entity_types_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.block_entity_types_by_id.is_empty()
    }
}

impl RegistryExt for BlockEntityTypeRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for BlockEntityTypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}
