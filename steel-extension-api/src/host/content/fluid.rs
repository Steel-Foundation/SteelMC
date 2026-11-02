//! Builds a registry fluid registration from an extension's definition.

use steel_registry::{
    Registry,
    client_content::{FluidRegistration, FluidSpread, FluidTint as NativeTint},
    fluid::FluidRef,
    vanilla_fluids,
};
use steel_utils::Identifier;

use crate::BlockState;
use crate::content::{FluidDefinition, RegisteredFluid};

fn key(text: &str) -> Result<Identifier, String> {
    text.parse::<Identifier>().map_err(ToString::to_string)
}

fn client_source(text: &str) -> Result<FluidRef, String> {
    let key = key(text)?;
    if key == vanilla_fluids::WATER.key {
        Ok(&vanilla_fluids::WATER)
    } else if key == vanilla_fluids::LAVA.key {
        Ok(&vanilla_fluids::LAVA)
    } else {
        Err(format!(
            "client fluid {key} must be minecraft:water or minecraft:lava"
        ))
    }
}

pub(crate) fn register_fluid(
    registry: &mut Registry,
    definition: FluidDefinition,
) -> Result<RegisteredFluid, String> {
    let bucket = definition
        .bucket
        .clone()
        .match_owned(|item| Some(super::prepare_item(registry, item)), || None)
        .transpose()?;
    let tint = definition.tint.match_owned(
        |tint| {
            Some(NativeTint {
                water_color: tint.water_color,
                water_fog_color: tint.water_fog_color,
            })
        },
        || None,
    );
    let entry = registry.register_client_fluid(FluidRegistration {
        source_key: key(definition.key.as_str())?,
        flowing_key: key(definition.flowing_key.as_str())?,
        block_key: key(definition.block_key.as_str())?,
        client_source: client_source(definition.client.as_str())?,
        tint,
        spread: FluidSpread {
            tick_delay: definition.tick_delay,
            drop_off: definition.drop_off,
            slope_find_distance: definition.slope_find_distance,
            converts_to_source: definition.converts_to_source,
        },
        explosion_resistance: definition.explosion_resistance,
        randomly_ticking: definition.randomly_ticking,
        bucket,
    })?;
    Ok(RegisteredFluid {
        source: BlockState {
            id: registry.blocks.get_default_state_id(entry.block).0,
        },
    })
}
