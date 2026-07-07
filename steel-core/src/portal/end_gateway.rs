//! End gateway destination calculation.

use std::sync::Arc;

use glam::DVec3;
use steel_protocol::packets::game::RelativeMovement;
use steel_registry::vanilla_entities;
use steel_utils::{BlockPos, ChunkPos, SectionPos};

use crate::{
    block_entity::entities::EndGatewayBlockEntity,
    entity::Entity,
    portal::{PortalTicketTarget, TeleportPostTransition, TeleportTransition},
    world::World,
};

const GATEWAY_HEIGHT_ABOVE_SURFACE: i32 = 10;
const EXIT_PORTAL_SEARCH_DISTANCE: f64 = 1024.0;
const EXIT_PORTAL_SEARCH_STEP: f64 = 16.0;
const EXIT_PORTAL_SEARCH_LIMIT: i32 = 16;
const EXIT_POSITION_SEARCH_RADIUS: i32 = 5;
const VALID_TELEPORT_SEARCH_RADIUS: i32 = 16;
const GENERATED_ISLAND_Y: i32 = 75;

/// Initial chunk preparation needed before resolving an End gateway transition.
pub(crate) enum EndGatewayChunkPreparation {
    /// Requested chunks are enough to calculate the transition after they load.
    Ready(Vec<ChunkPos>),
    /// Requested chunks only cover vanilla's tentative outer-island search.
    SearchPath(Vec<ChunkPos>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GatewayExitState {
    Stored { exit: BlockPos, exact: bool },
    Missing { exact: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GatewayTeleportAnchor {
    Existing(BlockPos),
    NeedsIsland(BlockPos),
}

impl GatewayTeleportAnchor {
    const fn pos(self) -> BlockPos {
        match self {
            Self::Existing(pos) | Self::NeedsIsland(pos) => pos,
        }
    }
}

/// Returns the first chunks that must be ready before an End gateway transition can be resolved.
#[must_use]
pub(crate) fn initial_chunks(
    world: &World,
    portal_pos: BlockPos,
    source_is_end: bool,
) -> Option<EndGatewayChunkPreparation> {
    match gateway_exit_state(world, portal_pos)? {
        GatewayExitState::Stored { exit, exact: true } => Some(EndGatewayChunkPreparation::Ready(
            chunks_for_block_square(exit, 0),
        )),
        GatewayExitState::Stored { exit, exact: false } => Some(EndGatewayChunkPreparation::Ready(
            chunks_for_block_square(exit.offset(0, 2, 0), EXIT_POSITION_SEARCH_RADIUS),
        )),
        GatewayExitState::Missing { .. } if source_is_end => Some(
            EndGatewayChunkPreparation::SearchPath(exit_search_candidate_chunks(portal_pos)),
        ),
        GatewayExitState::Missing { .. } => None,
    }
}

/// Returns final chunks needed after the tentative outer-island search chunks are ready.
#[must_use]
pub(crate) fn final_chunks_after_search(
    world: &World,
    portal_pos: BlockPos,
    source_is_end: bool,
) -> Option<Vec<ChunkPos>> {
    match gateway_exit_state(world, portal_pos)? {
        GatewayExitState::Stored { exit, exact: true } => Some(chunks_for_block_square(exit, 0)),
        GatewayExitState::Stored { exit, exact: false } => Some(chunks_for_block_square(
            exit.offset(0, 2, 0),
            EXIT_POSITION_SEARCH_RADIUS,
        )),
        GatewayExitState::Missing { .. } if source_is_end => {
            let anchor = find_teleport_anchor(world, portal_pos)?;
            Some(chunks_for_block_square(
                anchor.pos(),
                VALID_TELEPORT_SEARCH_RADIUS,
            ))
        }
        GatewayExitState::Missing { .. } => None,
    }
}

/// Calculates vanilla's End gateway transition after the required chunks are available.
#[must_use]
pub(crate) fn calculate_transition(
    world: &Arc<World>,
    entity: &dyn Entity,
    portal_pos: BlockPos,
    source_is_end: bool,
) -> Option<TeleportTransition> {
    let (exit, exact) = match gateway_exit_state(world, portal_pos)? {
        GatewayExitState::Stored { exit, exact } => (exit, exact),
        GatewayExitState::Missing { exact } if source_is_end => {
            let exit = find_or_create_valid_teleport_pos(world, portal_pos)?
                .above_n(GATEWAY_HEIGHT_ABOVE_SURFACE);
            if !world.create_end_gateway_portal(exit, portal_pos, false) {
                log::error!("Unable to create End gateway portal at {}", world.key);
                return None;
            }
            if !set_gateway_exit_position(world, portal_pos, exit, exact) {
                return None;
            }
            (exit, exact)
        }
        GatewayExitState::Missing { .. } => return None,
    };

    let destination = if exact {
        exit
    } else {
        find_exit_position(world, exit)
    };
    Some(gateway_transition(world, entity, destination))
}

fn gateway_exit_state(world: &World, portal_pos: BlockPos) -> Option<GatewayExitState> {
    let block_entity = world.get_block_entity(portal_pos)?;
    let block_entity = block_entity.lock();
    let gateway = block_entity
        .as_any()
        .downcast_ref::<EndGatewayBlockEntity>()?;
    Some(match gateway.exit_portal() {
        Some(exit) => GatewayExitState::Stored {
            exit,
            exact: gateway.exact_teleport(),
        },
        None => GatewayExitState::Missing {
            exact: gateway.exact_teleport(),
        },
    })
}

fn set_gateway_exit_position(
    world: &World,
    portal_pos: BlockPos,
    exit: BlockPos,
    exact: bool,
) -> bool {
    let Some(block_entity) = world.get_block_entity(portal_pos) else {
        return false;
    };
    let mut block_entity = block_entity.lock();
    let Some(gateway) = block_entity
        .as_any_mut()
        .downcast_mut::<EndGatewayBlockEntity>()
    else {
        return false;
    };
    gateway.set_exit_position(exit, exact);
    true
}

fn find_exit_position(world: &World, exit_portal: BlockPos) -> BlockPos {
    world
        .find_end_gateway_tallest_block(
            exit_portal.offset(0, 2, 0),
            EXIT_POSITION_SEARCH_RADIUS,
            false,
        )
        .above()
}

fn find_or_create_valid_teleport_pos(
    world: &Arc<World>,
    gateway_pos: BlockPos,
) -> Option<BlockPos> {
    let anchor = find_teleport_anchor(world, gateway_pos)?;
    if let GatewayTeleportAnchor::NeedsIsland(pos) = anchor
        && !world.create_end_island(pos)
    {
        log::error!("Unable to create End island at {}", world.key);
        return None;
    }

    Some(world.find_end_gateway_tallest_block(anchor.pos(), VALID_TELEPORT_SEARCH_RADIUS, true))
}

fn find_teleport_anchor(world: &World, gateway_pos: BlockPos) -> Option<GatewayTeleportAnchor> {
    let tentative = find_exit_portal_xz_pos_tentative(world, gateway_pos)?;
    let chunk = chunk_for_xz_vec(tentative);
    if let Some(pos) = world.find_end_gateway_valid_spawn_in_chunk(chunk) {
        return Some(GatewayTeleportAnchor::Existing(pos));
    }

    Some(GatewayTeleportAnchor::NeedsIsland(BlockPos::new(
        (tentative.x + 0.5).floor() as i32,
        GENERATED_ISLAND_Y,
        (tentative.z + 0.5).floor() as i32,
    )))
}

fn find_exit_portal_xz_pos_tentative(world: &World, gateway_pos: BlockPos) -> Option<DVec3> {
    let direction = xz_direction(gateway_pos);
    let mut tentative = direction * EXIT_PORTAL_SEARCH_DISTANCE;

    let mut remaining = EXIT_PORTAL_SEARCH_LIMIT;
    while !is_chunk_empty(world, tentative)? && remaining > 0 {
        remaining -= 1;
        tentative -= direction * EXIT_PORTAL_SEARCH_STEP;
    }

    let mut remaining = EXIT_PORTAL_SEARCH_LIMIT;
    while is_chunk_empty(world, tentative)? && remaining > 0 {
        remaining -= 1;
        tentative += direction * EXIT_PORTAL_SEARCH_STEP;
    }

    Some(tentative)
}

fn is_chunk_empty(world: &World, xz_pos: DVec3) -> Option<bool> {
    world.is_end_gateway_chunk_empty(chunk_for_xz_vec(xz_pos))
}

fn gateway_transition(
    world: &Arc<World>,
    entity: &dyn Entity,
    destination: BlockPos,
) -> TeleportTransition {
    let is_ender_pearl = entity.entity_type() == &vanilla_entities::ENDER_PEARL;
    TeleportTransition {
        target_world: world.clone(),
        position: block_bottom_center(destination),
        rotation: (0.0, 0.0),
        velocity: DVec3::ZERO,
        relatives: if is_ender_pearl {
            RelativeMovement::NONE
        } else {
            RelativeMovement::DELTA.union(RelativeMovement::ROTATION)
        },
        portal_cooldown: entity.dimension_changing_delay(),
        as_passenger: false,
        post_transition: TeleportPostTransition::place_portal_ticket(
            PortalTicketTarget::Destination,
        ),
    }
}

fn exit_search_candidate_chunks(gateway_pos: BlockPos) -> Vec<ChunkPos> {
    let direction = xz_direction(gateway_pos);
    let start = direction * EXIT_PORTAL_SEARCH_DISTANCE;
    let mut chunks = Vec::with_capacity((EXIT_PORTAL_SEARCH_LIMIT * 2 + 1) as usize);
    for step in -EXIT_PORTAL_SEARCH_LIMIT..=EXIT_PORTAL_SEARCH_LIMIT {
        chunks.push(chunk_for_xz_vec(
            start + direction * (f64::from(step) * EXIT_PORTAL_SEARCH_STEP),
        ));
    }
    chunks
}

fn chunks_for_block_square(center: BlockPos, block_radius: i32) -> Vec<ChunkPos> {
    let min_chunk_x = SectionPos::block_to_section_coord(center.x() - block_radius);
    let max_chunk_x = SectionPos::block_to_section_coord(center.x() + block_radius);
    let min_chunk_z = SectionPos::block_to_section_coord(center.z() - block_radius);
    let max_chunk_z = SectionPos::block_to_section_coord(center.z() + block_radius);
    let mut chunks = Vec::with_capacity(
        ((max_chunk_x - min_chunk_x + 1) * (max_chunk_z - min_chunk_z + 1)) as usize,
    );

    for chunk_z in min_chunk_z..=max_chunk_z {
        for chunk_x in min_chunk_x..=max_chunk_x {
            chunks.push(ChunkPos::new(chunk_x, chunk_z));
        }
    }
    chunks
}

fn chunk_for_xz_vec(pos: DVec3) -> ChunkPos {
    ChunkPos::new((pos.x / 16.0).floor() as i32, (pos.z / 16.0).floor() as i32)
}

fn xz_direction(pos: BlockPos) -> DVec3 {
    let vector = DVec3::new(f64::from(pos.x()), 0.0, f64::from(pos.z()));
    let length = vector.length();
    if length < 1.0E-4 {
        DVec3::ZERO
    } else {
        vector / length
    }
}

fn block_bottom_center(pos: BlockPos) -> DVec3 {
    let (x, y, z) = pos.get_bottom_center();
    DVec3::new(x, y, z)
}

#[cfg(test)]
mod tests {
    use super::{
        EXIT_PORTAL_SEARCH_LIMIT, chunks_for_block_square, exit_search_candidate_chunks,
        xz_direction,
    };
    use glam::DVec3;
    use steel_utils::{BlockPos, ChunkPos};

    #[test]
    fn zero_gateway_position_has_zero_search_direction() {
        assert_eq!(xz_direction(BlockPos::ZERO), DVec3::ZERO);
    }

    #[test]
    fn exit_search_candidates_cover_vanilla_probe_range() {
        let chunks = exit_search_candidate_chunks(BlockPos::new(1, 70, 0));

        assert_eq!(chunks.len(), (EXIT_PORTAL_SEARCH_LIMIT * 2 + 1) as usize);
        assert!(chunks.contains(&ChunkPos::new(48, 0)));
        assert!(chunks.contains(&ChunkPos::new(64, 0)));
        assert!(chunks.contains(&ChunkPos::new(80, 0)));
    }

    #[test]
    fn block_square_chunks_cover_radius_across_chunk_edges() {
        let chunks = chunks_for_block_square(BlockPos::new(16, 70, 16), 5);

        assert_eq!(
            chunks,
            vec![
                ChunkPos::new(0, 0),
                ChunkPos::new(1, 0),
                ChunkPos::new(0, 1),
                ChunkPos::new(1, 1),
            ]
        );
    }
}
