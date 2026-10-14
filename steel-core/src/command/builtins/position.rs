//! Position helpers shared by commands that move players.

use glam::DVec3;
use steel_math::wrap_degrees;
use steel_utils::{BlockPos, translations};
use text_components::TextComponent;

use super::super::brigadier::CommandSyntaxError;
use crate::world::World;

pub(super) fn ensure_spawnable_position(position: DVec3) -> Result<(), CommandSyntaxError> {
    if World::is_in_spawnable_bounds(BlockPos::from(position)) {
        return Ok(());
    }
    Err(CommandSyntaxError::dynamic(TextComponent::from(
        &translations::COMMANDS_TELEPORT_INVALID_POSITION,
    )))
}

pub(super) fn wrap_rotation((yaw, pitch): (f32, f32)) -> (f32, f32) {
    (wrap_degrees(yaw), wrap_degrees(pitch))
}
