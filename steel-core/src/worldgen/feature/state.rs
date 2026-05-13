use super::prelude::*;
use super::runner::FeatureDecorationRunner;

impl FeatureDecorationRunner {
    pub(super) fn block_matches_identifier_list(
        registry: &Registry,
        block: BlockRef,
        identifiers: &IdentifierList,
    ) -> bool {
        identifiers.0.iter().any(|block_key| {
            let Some(candidate) = registry.blocks.by_key(block_key) else {
                panic!("configured feature references unknown block {block_key}");
            };
            block == candidate
        })
    }

    pub(super) fn block_state_from_data(
        registry: &Registry,
        data: &BlockStateData,
    ) -> steel_utils::BlockStateId {
        let Some(block) = registry.blocks.by_key(&data.name) else {
            panic!(
                "block state provider references unknown block {}",
                data.name
            );
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
                    "block state provider references unknown property {key} on {}",
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
                "block state provider references unknown or invalid state {}",
                data.name
            );
        };
        state
    }

    pub(super) fn fluid_state_from_data(registry: &Registry, data: &FluidStateData) -> FluidState {
        let Some(fluid) = registry.fluids.by_key(&data.name) else {
            panic!(
                "fluid state provider references unknown fluid {}",
                data.name
            );
        };

        let mut amount = Self::default_fluid_amount(fluid);
        let mut falling = false;

        for (property, value) in &data.properties {
            match property.as_str() {
                "falling" if !fluid.is_empty => {
                    falling = Self::parse_fluid_bool_property(&data.name, property, value);
                }
                "level" if !fluid.is_empty && !fluid.is_source => {
                    amount = Self::parse_flowing_fluid_level(&data.name, value);
                }
                _ => {
                    panic!(
                        "fluid state provider references unknown property {property} on {}",
                        data.name
                    );
                }
            }
        }

        FluidState::new(fluid, amount, falling)
    }

    pub(super) const fn default_fluid_amount(fluid: FluidRef) -> u8 {
        if fluid.is_empty {
            0
        } else if fluid.is_source {
            8
        } else {
            1
        }
    }

    pub(super) fn parse_fluid_bool_property(
        fluid_name: &steel_utils::Identifier,
        property: &str,
        value: &str,
    ) -> bool {
        match value {
            "true" => true,
            "false" => false,
            _ => panic!(
                "fluid state provider references invalid boolean value {value} for property {property} on {fluid_name}"
            ),
        }
    }

    pub(super) fn parse_flowing_fluid_level(
        fluid_name: &steel_utils::Identifier,
        value: &str,
    ) -> u8 {
        let Ok(level) = value.parse::<u8>() else {
            panic!("fluid state provider references invalid flowing level {value} on {fluid_name}");
        };
        assert!(
            (1..=8).contains(&level),
            "fluid state provider references flowing level {level} outside 1..=8 on {fluid_name}"
        );
        level
    }

    pub(super) fn legacy_block_from_fluid_state(
        registry: &Registry,
        fluid_state: FluidState,
    ) -> BlockStateId {
        let Some(block) = registry.blocks.by_key(&fluid_state.fluid_id.block) else {
            panic!(
                "fluid {} references unknown legacy block {}",
                fluid_state.fluid_id.key, fluid_state.fluid_id.block
            );
        };

        let mut state = registry.blocks.get_default_state_id(block);
        if registry
            .blocks
            .try_get_property(state, &BlockStateProperties::LEVEL)
            .is_some()
        {
            state = Self::set_int_property_by_name(
                registry,
                state,
                "level",
                i32::from(Self::legacy_fluid_block_level(fluid_state)),
            );
        }
        state
    }

    pub(super) fn legacy_fluid_block_level(fluid_state: FluidState) -> u8 {
        if fluid_state.fluid_id.is_source {
            return 0;
        }

        let amount = fluid_state.amount.min(8);
        if fluid_state.falling {
            8 + (8 - amount)
        } else {
            8 - amount
        }
    }
}
