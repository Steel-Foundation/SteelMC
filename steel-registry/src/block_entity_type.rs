use steel_utils::Identifier;

/// Represents a block entity type in Minecraft.
/// Block entities are used for blocks that need to store additional data
/// beyond their block state, such as chests, furnaces, signs, etc.
#[derive(Debug)]
pub struct BlockEntityType {
    pub key: Identifier,
}

crate::define_registry!(
    BlockEntityTypeRegistry,
    BlockEntityType,
    stem: block_entity_types,
);
