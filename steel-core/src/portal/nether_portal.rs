//! Nether portal destination calculation.

use std::sync::Arc;

use glam::DVec3;
use steel_protocol::packets::game::RelativeMovement;
use steel_registry::{
    blocks::{block_state_ext::BlockStateExt, properties::BlockStateProperties},
    dimension_type::DimensionType,
};
use steel_utils::{
    BlockPos, ChunkPos, SectionPos,
    axis::Axis,
    block_util::{FoundRectangle, get_largest_rectangle_around},
};

use crate::{
    entity::Entity,
    portal::{
        PortalTicketTarget, TeleportPostTransition, TeleportTransition, portal_shape::PortalShape,
    },
    world::World,
};

const NETHER_TARGET_PORTAL_SEARCH_RADIUS: i32 = 16;
const OVERWORLD_TARGET_PORTAL_SEARCH_RADIUS: i32 = 128;
const PORTAL_RECTANGLE_SCAN_LIMIT: i32 = 21;

/// Returns vanilla's target portal search radius for Nether portal dimension changes.
#[must_use]
pub(crate) const fn search_radius(to_nether: bool) -> i32 {
    if to_nether {
        NETHER_TARGET_PORTAL_SEARCH_RADIUS
    } else {
        OVERWORLD_TARGET_PORTAL_SEARCH_RADIUS
    }
}

/// Returns the full-chunk square radius Steel prewarms before resolving a Nether portal exit.
#[must_use]
pub(crate) const fn prewarm_chunk_radius(to_nether: bool) -> u8 {
    let chunk_radius = search_radius(to_nether) / 16 + 2;
    chunk_radius as u8
}

/// Returns the chunk centered on a block position for portal prewarming.
#[must_use]
pub(crate) const fn prewarm_center(pos: BlockPos) -> ChunkPos {
    ChunkPos::new(
        SectionPos::block_to_section_coord(pos.x()),
        SectionPos::block_to_section_coord(pos.z()),
    )
}

/// Returns vanilla's scaled and world-border-clamped approximate exit position.
#[must_use]
pub(crate) fn approximate_exit_position(
    source_world: &World,
    target_world: &World,
    entity_position: DVec3,
) -> BlockPos {
    let scale = DimensionType::get_teleportation_scale(
        source_world.dimension_type,
        target_world.dimension_type,
    );
    target_world.clamp_to_world_border(
        entity_position.x * scale,
        entity_position.y,
        entity_position.z * scale,
    )
}

/// Calculates a Nether portal teleport transition after the target chunks are available.
#[must_use]
pub(crate) fn calculate_transition(
    source_world: &Arc<World>,
    target_world: &Arc<World>,
    entity: &dyn Entity,
    portal_entry_pos: BlockPos,
    approximate_exit_pos: BlockPos,
    to_nether: bool,
) -> Option<TeleportTransition> {
    let exit_portal_pos =
        target_world.find_closest_nether_portal_position(approximate_exit_pos, to_nether);
    let (exit_portal, ticket_target) = if let Some(pos) = exit_portal_pos {
        (
            largest_portal_rectangle_at(target_world, pos)?,
            PortalTicketTarget::Block(pos),
        )
    } else {
        let source_portal_axis = source_world
            .get_block_state(portal_entry_pos)
            .try_get_value(&BlockStateProperties::HORIZONTAL_AXIS)
            .unwrap_or(Axis::X);
        let Some(created) =
            target_world.create_nether_portal(approximate_exit_pos, source_portal_axis)
        else {
            log::error!("Unable to create a portal, likely target out of world border");
            return None;
        };
        (created, PortalTicketTarget::Destination)
    };
    let post_transition = TeleportPostTransition::play_portal_sound()
        .then(TeleportPostTransition::place_portal_ticket(ticket_target));

    Some(dimension_transition_from_exit(
        source_world,
        target_world,
        entity,
        portal_entry_pos,
        exit_portal,
        post_transition,
    ))
}

fn largest_portal_rectangle_at(world: &World, pos: BlockPos) -> Option<FoundRectangle> {
    let portal_state = world.get_block_state(pos);
    let axis = portal_state.try_get_value(&BlockStateProperties::HORIZONTAL_AXIS)?;
    Some(get_largest_rectangle_around(
        pos,
        axis,
        PORTAL_RECTANGLE_SCAN_LIMIT,
        Axis::Y,
        PORTAL_RECTANGLE_SCAN_LIMIT,
        |block_pos| world.get_block_state(block_pos) == portal_state,
    ))
}

fn dimension_transition_from_exit(
    source_world: &World,
    target_world: &Arc<World>,
    entity: &dyn Entity,
    portal_entry_pos: BlockPos,
    exit_portal: FoundRectangle,
    post_transition: TeleportPostTransition,
) -> TeleportTransition {
    let source_portal_state = source_world.get_block_state(portal_entry_pos);
    let (source_axis, offset) = if let Some(axis) =
        source_portal_state.try_get_value(&BlockStateProperties::HORIZONTAL_AXIS)
    {
        let portal_area = get_largest_rectangle_around(
            portal_entry_pos,
            axis,
            PORTAL_RECTANGLE_SCAN_LIMIT,
            Axis::Y,
            PORTAL_RECTANGLE_SCAN_LIMIT,
            |pos| source_world.get_block_state(pos) == source_portal_state,
        );
        (axis, entity.get_relative_portal_position(axis, portal_area))
    } else {
        (Axis::X, DVec3::new(0.5, 0.0, 0.0))
    };

    create_dimension_transition(
        target_world,
        exit_portal,
        source_axis,
        offset,
        entity,
        post_transition,
    )
}

fn create_dimension_transition(
    target_world: &Arc<World>,
    found_rectangle: FoundRectangle,
    portal_axis: Axis,
    offset: DVec3,
    entity: &dyn Entity,
    post_transition: TeleportPostTransition,
) -> TeleportTransition {
    let bottom_left = found_rectangle.min_corner;
    let target_axis = target_world
        .get_block_state(bottom_left)
        .try_get_value(&BlockStateProperties::HORIZONTAL_AXIS)
        .unwrap_or(Axis::X);
    let width = f64::from(found_rectangle.axis1_size);
    let height = f64::from(found_rectangle.axis2_size);
    let dimensions = entity.dimensions_for_pose(entity.pose());
    let entity_width = f64::from(dimensions.width);
    let entity_height = f64::from(dimensions.height);
    let output_rotation = if portal_axis == target_axis {
        0.0
    } else {
        90.0
    };
    let offset_right = entity_width / 2.0 + (width - entity_width) * offset.x;
    let offset_up = (height - entity_height) * offset.y;
    let offset_forward = 0.5 + offset.z;
    let x_aligned = target_axis == Axis::X;
    let target_pos = DVec3::new(
        f64::from(bottom_left.x())
            + if x_aligned {
                offset_right
            } else {
                offset_forward
            },
        f64::from(bottom_left.y()) + offset_up,
        f64::from(bottom_left.z())
            + if x_aligned {
                offset_forward
            } else {
                offset_right
            },
    );
    let collision_free_pos =
        PortalShape::find_collision_free_position(target_pos, target_world, entity, dimensions);

    TeleportTransition {
        target_world: target_world.clone(),
        position: collision_free_pos,
        rotation: (output_rotation, 0.0),
        velocity: DVec3::ZERO,
        relatives: RelativeMovement::DELTA.union(RelativeMovement::ROTATION),
        portal_cooldown: entity.dimension_changing_delay(),
        as_passenger: false,
        post_transition,
    }
}

#[cfg(test)]
mod tests {
    use super::{prewarm_chunk_radius, search_radius};

    #[test]
    fn search_radius_matches_vanilla_portal_forcer_targets() {
        assert_eq!(search_radius(true), 16);
        assert_eq!(search_radius(false), 128);
    }

    #[test]
    fn prewarm_radius_covers_poi_search_and_exit_rectangle_edges() {
        assert_eq!(prewarm_chunk_radius(true), 3);
        assert_eq!(prewarm_chunk_radius(false), 10);
    }
}
