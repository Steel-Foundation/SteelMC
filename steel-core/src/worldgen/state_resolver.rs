use steel_registry::shared_structs::BlockStateData;
use steel_registry::{Registry, RegistryExt};
use steel_utils::BlockStateId;

/// Resolves vanilla JSON/NBT block-state data to Steel block-state ids.
pub(crate) struct WorldgenStateResolver;

impl WorldgenStateResolver {
    pub(crate) fn block_state_from_data(
        registry: &Registry,
        data: &BlockStateData,
        context: &str,
    ) -> BlockStateId {
        let Some(block) = registry.blocks.by_key(&data.name) else {
            panic!("{context} references unknown block {}", data.name);
        };

        let mut properties = registry
            .blocks
            .get_properties(registry.blocks.get_default_state_id(block))
            .into_iter()
            .map(|(key, value)| (key as &str, value as &str))
            .collect::<Vec<_>>();

        for (key, value) in &data.properties {
            let Some((_, property_value)) = properties
                .iter_mut()
                .find(|(property_key, _)| *property_key == key)
            else {
                panic!(
                    "{context} references unknown property {key} on {}",
                    data.name
                );
            };
            *property_value = value.as_str();
        }

        let Some(state) = registry
            .blocks
            .state_id_from_properties(&data.name, &properties)
        else {
            panic!(
                "{context} references unknown or invalid state {}",
                data.name
            );
        };
        state
    }
}
