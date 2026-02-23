//! Nether portal destination calculation with coordinate scaling.

use std::ptr;
use std::sync::Arc;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_dimension_types::{OVERWORLD, THE_NETHER};
use steel_utils::BlockPos;
use steel_utils::math::{Axis, Vector3};

use crate::portal::TeleportTransition;
use crate::portal::portal_forcer::NetherPortalForcer;
use crate::server::Server;
use crate::world::World;

/// Default portal cooldown in ticks (15 seconds).
const PORTAL_COOLDOWN_TICKS: i32 = 300;

/// Calculates the destination for a nether portal transition.
///
/// Handles coordinate scaling between overworld (1:1) and nether (1:8).
pub fn calculate_destination(
    server: &Server,
    source_world: &World,
    source_pos: BlockPos,
) -> Option<TeleportTransition> {
    // Determine target dimension (overworld <-> nether toggle)
    let target_world = match source_world.dimension {
        d if ptr::eq(d, OVERWORLD) => server.nether(),
        d if ptr::eq(d, THE_NETHER) => Some(server.overworld()),
        _ => return None, // Nether portals only work in overworld/nether
    }?;

    // Scale coordinates using coordinate_scale
    let scale = source_world.dimension.coordinate_scale / target_world.dimension.coordinate_scale;
    let target_x = f64::from(source_pos.x()) * scale;
    let target_z = f64::from(source_pos.z()) * scale;

    // Clamp Y to target dimension's valid range
    let target_y = source_pos.y().clamp(
        target_world.dimension.min_y,
        target_world.dimension.min_y + target_world.dimension.logical_height - 1,
    );

    // Find or create a portal at the destination
    let target_block = BlockPos::new(target_x as i32, target_y, target_z as i32);
    let search_radius = if ptr::eq(target_world.dimension, THE_NETHER) {
        16
    } else {
        128
    };
    // Searches for an existing nether portal near the target position.
    // If none is found, a new one is created at the target position.
    let exit_pos = NetherPortalForcer::find_portal(target_world, target_block, search_radius)
        .unwrap_or_else(|| {
            NetherPortalForcer::create_portal(
                target_world,
                target_block,
                source_world
                    .get_block_state(&source_pos)
                    .try_get_value(&BlockStateProperties::HORIZONTAL_AXIS)
                    .unwrap_or_else(|| Axis::X),
            )
        });

    Some(TeleportTransition {
        target_world: Arc::clone(target_world),
        position: Vector3::new(
            f64::from(exit_pos.x()) + 0.5,
            f64::from(exit_pos.y()),
            f64::from(exit_pos.z()) + 0.5,
        ),
        rotation: (0.0, 0.0),
        portal_cooldown: PORTAL_COOLDOWN_TICKS,
    })
}
