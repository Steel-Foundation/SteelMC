use rustc_hash::FxHashMap;
use steel_utils::Identifier;

use crate::blocks::BlockRef;

/// Represents a block entity type in Minecraft.
/// Block entities are used for blocks that need to store additional data
/// beyond their block state, such as chests, furnaces, signs, etc.
#[derive(Debug)]
pub struct BlockEntityType {
    pub key: Identifier,
    /// Blocks for which vanilla accepts this block entity type.
    pub valid_blocks: &'static [BlockRef],
}

impl BlockEntityType {
    #[must_use]
    pub fn is_valid(&self, block: BlockRef) -> bool {
        self.valid_blocks
            .iter()
            .any(|valid_block| std::ptr::eq(*valid_block, block))
    }
}

pub type BlockEntityTypeRef = &'static BlockEntityType;

pub struct BlockEntityTypeRegistry {
    block_entity_types_by_id: Vec<BlockEntityTypeRef>,
    block_entity_types_by_key: FxHashMap<Identifier, usize>,
    blocks_with_block_entities: Vec<bool>,
    allows_registering: bool,
}

impl BlockEntityTypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            block_entity_types_by_id: Vec::new(),
            block_entity_types_by_key: FxHashMap::default(),
            blocks_with_block_entities: Vec::new(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, block_entity_type: BlockEntityTypeRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register block entity types after registry has been frozen"
        );

        let id = self.block_entity_types_by_id.len();
        self.block_entity_types_by_id.push(block_entity_type);
        self.block_entity_types_by_key
            .insert(block_entity_type.key.clone(), id);

        for block in block_entity_type.valid_blocks {
            let Some(block_id) = block.id.get().copied() else {
                panic!(
                    "block {} must be registered before block entity type {}",
                    block.key, block_entity_type.key
                );
            };
            if self.blocks_with_block_entities.len() <= block_id {
                self.blocks_with_block_entities.resize(block_id + 1, false);
            }
            self.blocks_with_block_entities[block_id] = true;
        }

        id
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, BlockEntityTypeRef)> + '_ {
        self.block_entity_types_by_id.iter().copied().enumerate()
    }

    /// Returns whether at least one block entity type accepts `block`.
    ///
    /// Vanilla does not require those accepted-block sets to be globally disjoint, so creation
    /// remains the owning block behavior's responsibility.
    #[must_use]
    pub fn has_block_entity(&self, block: BlockRef) -> bool {
        let Some(block_id) = block.id.get().copied() else {
            return false;
        };
        self.blocks_with_block_entities
            .get(block_id)
            .copied()
            .unwrap_or(false)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_support::init_test_registry, vanilla_block_entity_types, vanilla_blocks};

    #[test]
    fn overlapping_type_memberships_preserve_structural_presence() {
        init_test_registry();
        let valid_blocks = Box::leak(vec![&vanilla_blocks::BARREL].into_boxed_slice());
        let alternate = Box::leak(Box::new(BlockEntityType {
            key: Identifier::new_static("test", "alternate_barrel"),
            valid_blocks,
        }));
        let mut registry = BlockEntityTypeRegistry::new();

        registry.register(&vanilla_block_entity_types::BARREL);
        registry.register(alternate);

        assert!(registry.has_block_entity(&vanilla_blocks::BARREL));
        assert_eq!(registry.iter().count(), 2);
    }
}
