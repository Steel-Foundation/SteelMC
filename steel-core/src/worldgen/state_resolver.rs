use steel_registry::blocks::BlockRef;
use steel_registry::feature;
use steel_registry::shared_structs;
use steel_registry::{Registry, RegistryExt};
use steel_utils::BlockStateId;

/// Resolves vanilla JSON/NBT block-state data to Steel block-state ids.
pub(crate) struct WorldgenStateResolver;

impl WorldgenStateResolver {
    pub(crate) fn block_state_from_data(
        registry: &Registry,
        data: &shared_structs::BlockStateData,
        context: &str,
    ) -> BlockStateId {
        let Some(block) = registry.blocks.by_key(&data.name) else {
            panic!("{context} references unknown block {}", data.name);
        };
        Self::block_state_from_parts(
            registry,
            block,
            &data.name,
            data.properties
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
            context,
        )
    }

    pub(crate) fn feature_block_state_from_data(
        registry: &Registry,
        data: &feature::BlockStateData,
        context: &str,
    ) -> BlockStateId {
        Self::block_state_from_parts(
            registry,
            data.block,
            &data.block.key,
            data.properties.iter().copied(),
            context,
        )
    }

    fn block_state_from_parts<'a>(
        registry: &Registry,
        block: BlockRef,
        block_name: &steel_utils::Identifier,
        data_properties: impl IntoIterator<Item = (&'a str, &'a str)>,
        context: &str,
    ) -> BlockStateId {
        let Some(state) = registry
            .blocks
            .state_id_from_block_defaulted_properties(block, data_properties)
        else {
            panic!("{context} references unknown or invalid state {block_name}");
        };
        state
    }
}
