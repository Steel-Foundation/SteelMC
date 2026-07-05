use steel_registry::blocks::BlockRef;
use steel_registry::feature;
use steel_registry::shared_structs;
use steel_registry::{Registry, RegistryExt};
use steel_utils::BlockStateId;

/// Resolves vanilla JSON/NBT block-state data to Steel block-state ids.
pub struct WorldgenStateResolver;

fn map_block_name(name: &steel_utils::Identifier) -> steel_utils::Identifier {
    let namespace = name.namespace.as_ref();
    let path = name.path.as_ref();
    if namespace == "minecraft" {
        match path {
            "grass" => steel_utils::Identifier::vanilla_static("short_grass"),
            "grass_path" => steel_utils::Identifier::vanilla_static("dirt_path"),
            "chain" => steel_utils::Identifier::vanilla_static("iron_chain"),
            _ => name.clone(),
        }
    } else {
        name.clone()
    }
}

impl WorldgenStateResolver {
    /// Resolves a block state from data.
    ///
    /// # Panics
    /// Panics if the block is not in the registry or if the state properties are invalid.
    #[must_use]
    pub fn block_state_from_data(
        registry: &Registry,
        data: &shared_structs::BlockStateData,
        context: &str,
    ) -> BlockStateId {
        let name = map_block_name(&data.name);
        let Some(block) = registry.blocks.by_key(&name) else {
            println!(
                "CRITICAL: WorldgenStateResolver references unknown block: {:?}",
                name
            );
            panic!("{context} references unknown block {}", name);
        };
        Self::block_state_from_parts(
            registry,
            block,
            &name,
            data.properties
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
            context,
        )
    }

    /// Resolves a feature block state from data.
    ///
    /// # Panics
    /// Panics if the state properties are invalid.
    #[must_use]
    pub fn feature_block_state_from_data(
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
