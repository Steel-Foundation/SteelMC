//! End portal destination calculation.

use std::sync::Arc;

use glam::DVec3;
use steel_protocol::packets::game::RelativeMovement;
use steel_utils::{BlockPos, ChunkPos, Direction, SectionPos};

use crate::{
    entity::Entity,
    level_data::RespawnData,
    portal::{PortalTicketTarget, TeleportPostTransition, TeleportTransition},
    world::World,
};

/// Vanilla `ServerLevel.END_SPAWN_POINT`.
pub(crate) const END_SPAWN_POINT: BlockPos = BlockPos::new(100, 50, 0);

const END_PLATFORM_PREWARM_CHUNK_RADIUS: u8 = 1;

/// Returns the chunks Steel prewarms before creating the End spawn platform.
#[must_use]
pub(crate) const fn end_platform_prewarm_center() -> ChunkPos {
    prewarm_center(END_SPAWN_POINT)
}

/// Returns the chunk square radius that covers the vanilla 5x5 End platform.
#[must_use]
pub(crate) const fn end_platform_prewarm_chunk_radius() -> u8 {
    END_PLATFORM_PREWARM_CHUNK_RADIUS
}

/// Returns the chunk centered on a block position for End portal prewarming.
#[must_use]
pub(crate) const fn prewarm_center(pos: BlockPos) -> ChunkPos {
    ChunkPos::new(
        SectionPos::block_to_section_coord(pos.x()),
        SectionPos::block_to_section_coord(pos.z()),
    )
}

/// Calculates vanilla's non-End -> End portal transition.
#[must_use]
pub(crate) fn calculate_entry_transition(
    target_world: &Arc<World>,
    entity: &dyn Entity,
) -> Option<TeleportTransition> {
    if !target_world.create_end_platform(end_platform_origin()) {
        log::error!("Unable to create End platform at {}", target_world.key);
        return None;
    }

    Some(TeleportTransition {
        target_world: target_world.clone(),
        position: end_entry_position(entity.as_player().is_some()),
        rotation: (Direction::West.to_yaw(), 0.0),
        velocity: DVec3::ZERO,
        relatives: RelativeMovement::DELTA.union(RelativeMovement::new(RelativeMovement::X_ROT)),
        portal_cooldown: entity.dimension_changing_delay(),
        as_passenger: false,
        post_transition: portal_sound_then_destination_ticket(),
    })
}

/// Calculates vanilla's End -> respawn-world portal transition for non-player entities.
#[must_use]
pub(crate) fn calculate_entity_return_transition(
    target_world: &Arc<World>,
    entity: &dyn Entity,
    respawn_data: &RespawnData,
) -> TeleportTransition {
    let spawn_pos = target_world.adjust_spawn_location(respawn_data.pos());
    TeleportTransition {
        target_world: target_world.clone(),
        position: block_bottom_center(spawn_pos),
        rotation: (respawn_data.yaw, respawn_data.pitch),
        velocity: DVec3::ZERO,
        relatives: RelativeMovement::DELTA.union(RelativeMovement::ROTATION),
        portal_cooldown: entity.dimension_changing_delay(),
        as_passenger: false,
        post_transition: portal_sound_then_destination_ticket(),
    }
}

/// Calculates the currently supported End -> respawn-world transition for players.
///
/// Vanilla delegates to `ServerPlayer.findRespawnPositionAndUseSpawnBlock`.
///
/// TODO(respawn): replace this with the vanilla personal bed/anchor respawn path once Steel has
/// that player respawn foundation. This currently covers only the default respawn branch.
#[must_use]
pub(crate) fn calculate_player_return_transition(
    target_world: &Arc<World>,
    entity: &dyn Entity,
    position: DVec3,
    rotation: (f32, f32),
) -> TeleportTransition {
    TeleportTransition {
        target_world: target_world.clone(),
        position,
        rotation,
        velocity: DVec3::ZERO,
        relatives: RelativeMovement::NONE,
        portal_cooldown: entity.dimension_changing_delay(),
        as_passenger: false,
        post_transition: TeleportPostTransition::do_nothing(),
    }
}

const fn end_platform_origin() -> BlockPos {
    BlockPos::new(
        END_SPAWN_POINT.x(),
        END_SPAWN_POINT.y() - 1,
        END_SPAWN_POINT.z(),
    )
}

fn end_entry_position(is_player: bool) -> DVec3 {
    let mut position = block_bottom_center(END_SPAWN_POINT);
    if is_player {
        position.y -= 1.0;
    }
    position
}

fn portal_sound_then_destination_ticket() -> TeleportPostTransition {
    TeleportPostTransition::play_portal_sound().then(TeleportPostTransition::place_portal_ticket(
        PortalTicketTarget::Destination,
    ))
}

fn block_bottom_center(pos: BlockPos) -> DVec3 {
    let (x, y, z) = pos.get_bottom_center();
    DVec3::new(x, y, z)
}

#[cfg(test)]
mod tests {
    use super::{
        END_SPAWN_POINT, block_bottom_center, end_entry_position, end_platform_origin,
        end_platform_prewarm_center, end_platform_prewarm_chunk_radius,
    };
    use glam::DVec3;
    use steel_utils::{BlockPos, ChunkPos};

    #[test]
    fn end_platform_origin_is_below_vanilla_end_spawn() {
        assert_eq!(END_SPAWN_POINT, BlockPos::new(100, 50, 0));
        assert_eq!(end_platform_origin(), BlockPos::new(100, 49, 0));
    }

    #[test]
    fn end_entry_player_position_is_one_block_below_spawn_center() {
        assert_eq!(
            end_entry_position(false),
            block_bottom_center(BlockPos::new(100, 50, 0))
        );
        assert_eq!(end_entry_position(true), DVec3::new(100.5, 49.0, 0.5));
    }

    #[test]
    fn end_platform_prewarm_covers_negative_z_edge() {
        assert_eq!(end_platform_prewarm_center(), ChunkPos::new(6, 0));
        assert_eq!(end_platform_prewarm_chunk_radius(), 1);
    }
}
