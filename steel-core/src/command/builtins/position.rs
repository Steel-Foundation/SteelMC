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

/// Keeps `own` for a component that is not finite, since vanilla discards such a rotation.
pub(super) const fn finite_rotation(
    (yaw, pitch): (f32, f32),
    (own_yaw, own_pitch): (f32, f32),
) -> (f32, f32) {
    (
        if yaw.is_finite() { yaw } else { own_yaw },
        if pitch.is_finite() { pitch } else { own_pitch },
    )
}

pub(super) fn wrap_rotation((yaw, pitch): (f32, f32)) -> (f32, f32) {
    (wrap_degrees(yaw), wrap_degrees(pitch))
}
