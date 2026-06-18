use super::prelude::*;
use super::runner::FeatureDecorationRunner;
use steel_worldgen::state_resolver::WorldgenStateResolver;

impl FeatureDecorationRunner {
    pub(super) fn block_matches_holder_set(block: BlockRef, blocks: &BlockHolderSet) -> bool {
        match blocks {
            BlockHolderSet::Tag(tag) => block.has_tag(tag),
            BlockHolderSet::Entries(entries) => entries.contains(&block),
        }
    }

    pub(super) fn block_state_from_data(
        registry: &Registry,
        data: &BlockStateData,
    ) -> steel_utils::BlockStateId {
        WorldgenStateResolver::feature_block_state_from_data(registry, data, "block state provider")
    }

    pub(super) fn fluid_state_from_data(data: &FluidStateData) -> FluidState {
        let mut amount = Self::default_fluid_amount(data.fluid);
        let mut falling = false;

        for &(property, value) in data.properties {
            match property {
                "falling" if !data.fluid.is_empty => {
                    falling = Self::parse_fluid_bool_property(&data.fluid.key, property, value);
                }
                "level" if !data.fluid.is_empty && !data.fluid.is_source => {
                    amount = Self::parse_flowing_fluid_level(&data.fluid.key, value);
                }
                _ => {
                    panic!(
                        "fluid state provider references unknown property {property} on {}",
                        data.fluid.key
                    );
                }
            }
        }

        FluidState::new(data.fluid, amount, falling)
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
