//! A rotation argument.
use steel_protocol::packets::game::{ArgumentType, SuggestionType};

use crate::command::arguments::CommandArgument;
use crate::command::context::CommandContext;

/// A rotation argument.
pub struct RotationArgument;

impl CommandArgument for RotationArgument {
    type Output = (f32, f32);

    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)> {
        let (origin_yaw, origin_pitch) = context.rotation.unwrap_or((0.0, 0.0));
        let yaw = parse_rotation_coordinate(arg.first()?, origin_yaw)?;
        let pitch = parse_rotation_coordinate(arg.get(1)?, origin_pitch)?;

        Some((&arg[2..], normalize_rotation((yaw, pitch))))
    }

    fn usage(&self) -> (ArgumentType, Option<SuggestionType>) {
        (ArgumentType::Rotation, None)
    }
}

fn parse_rotation_coordinate(value: &str, origin: f32) -> Option<f32> {
    if value.starts_with('^') {
        return None;
    }

    if let Some(offset) = value.strip_prefix('~') {
        if offset.is_empty() {
            Some(origin)
        } else {
            Some(origin + offset.parse::<f32>().ok()?)
        }
    } else {
        value.parse::<f32>().ok()
    }
}

fn normalize_rotation((mut yaw, mut pitch): (f32, f32)) -> (f32, f32) {
    yaw = yaw.rem_euclid(360.0);
    if yaw >= 180.0 {
        yaw -= 360.0;
    }
    pitch = pitch.rem_euclid(360.0);
    if pitch >= 180.0 {
        pitch -= 360.0;
    }

    (yaw, pitch)
}

#[cfg(test)]
mod tests {
    use super::{normalize_rotation, parse_rotation_coordinate};

    #[test]
    fn relative_rotation_coordinates_resolve_from_origin() {
        assert_eq!(parse_rotation_coordinate("~", 45.0), Some(45.0));
        assert_eq!(parse_rotation_coordinate("~15", 45.0), Some(60.0));
        assert_eq!(parse_rotation_coordinate("~-90", 45.0), Some(-45.0));
    }

    #[test]
    fn rotation_coordinates_reject_local_prefix() {
        assert!(parse_rotation_coordinate("^", 45.0).is_none());
        assert!(parse_rotation_coordinate("^1", 45.0).is_none());
    }

    #[test]
    fn rotation_parser_keeps_existing_wrapping_behavior() {
        assert_eq!(normalize_rotation((181.0, -181.0)), (-179.0, 179.0));
        assert_eq!(normalize_rotation((90.0, 45.0)), (90.0, 45.0));
    }
}
