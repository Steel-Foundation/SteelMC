//! This module contains types and utilities for parsing command arguments.
pub mod anchor;
pub mod block_pos;
pub mod bool;
pub mod domain;
pub mod enchantment;
pub mod entity;
pub mod entity_type;
pub mod float;
pub mod gamemode;
pub mod integer;
pub mod item;
pub mod player;
pub mod rotation;
pub mod structure;
pub mod text_component;
pub mod time;
pub mod vector2;
pub mod vector3;
pub mod world;

use std::{f32::consts::PI, sync::Arc};

use glam::DVec3;
use steel_protocol::packets::game::{ArgumentType, SuggestionEntry, SuggestionType};

use crate::{
    command::context::{CommandContext, EntityAnchor},
    entity::Entity,
    server::Server,
    world::World,
};

/// Context passed to suggestion methods containing previously parsed arguments.
#[derive(Clone)]
pub struct SuggestionContext {
    /// Previously parsed argument values stored by name.
    /// Used for context-dependent suggestions (e.g., gamerule value depends on rule type).
    parsed_values: Vec<(&'static str, ParsedValue)>,
    /// The server where the suggestion is needed.
    pub server: Arc<Server>,
    /// The world the command sender is currently in.
    pub world: Arc<World>,
}

/// A parsed value that can be stored in suggestion context.
#[derive(Clone, Debug)]
pub enum ParsedValue {
    /// A string value (e.g., game rule name).
    String(String),
    /// A boolean value.
    Bool(bool),
    /// An integer value.
    Int(i32),
}

impl SuggestionContext {
    /// Creates a new empty suggestion context.
    #[must_use]
    pub const fn new(server: Arc<Server>, world: Arc<World>) -> Self {
        Self {
            parsed_values: vec![],
            server,
            world,
        }
    }

    /// Stores a parsed value with its argument name.
    pub fn set(&mut self, name: &'static str, value: ParsedValue) {
        self.parsed_values.push((name, value));
    }

    /// Gets a parsed string value by argument name.
    #[must_use]
    pub fn get_string(&self, name: &str) -> Option<&str> {
        self.parsed_values.iter().find_map(|(n, v)| {
            if *n == name
                && let ParsedValue::String(s) = v
            {
                return Some(s.as_str());
            }
            None
        })
    }
}

/// A trait that defines a command argument parser.
pub trait CommandArgument: Send + Sync {
    /// The type of the parsed output.
    type Output;

    /// Parses from the given arguments the expected type and returns the remaining unconsumed arguments and the parsed output.
    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)>;

    /// Returns the parser ID associated with this argument.
    fn usage(&self) -> (ArgumentType, Option<SuggestionType>);

    /// Returns suggestions for this argument based on the current input prefix.
    /// Only needs to be implemented for arguments using `SuggestionType::AskServer`.
    /// `prefix` is the partial text being typed for this argument.
    /// `suggestion_ctx` contains previously parsed arguments for context-dependent suggestions.
    /// Default implementation returns no suggestions.
    fn suggest(&self, _prefix: &str, _suggestion_ctx: &SuggestionContext) -> Vec<SuggestionEntry> {
        Vec::new()
    }

    /// Returns the value to store in `SuggestionContext` after parsing.
    /// This allows downstream arguments to make context-dependent suggestions.
    /// Returns `None` by default (don't store anything).
    fn parsed_value(&self, _args: &[&str], _context: &mut CommandContext) -> Option<ParsedValue> {
        None
    }
}

struct Helper;

impl Helper {
    pub fn parse_relative_coordinate<const IS_Y: bool>(
        s: &str,
        origin: Option<f64>,
    ) -> Option<f64> {
        if let Some(s) = s.strip_prefix('~') {
            let origin = origin?;
            let offset = if s.is_empty() { 0.0 } else { s.parse().ok()? };
            Some(origin + offset)
        } else {
            let mut v = s.parse().ok()?;

            // set position to block center if no decimal place is given
            if !IS_Y && !s.contains('.') {
                v += 0.5;
            }

            Some(v)
        }
    }

    pub fn parse_local_coordinates(arg: &[&str], context: &CommandContext) -> Option<DVec3> {
        let (left, up, forwards) = Self::parse_local_coordinate_triplet(arg)?;
        Some(Self::local_coordinates_to_position(
            left, up, forwards, context,
        ))
    }

    fn parse_local_coordinate_triplet(arg: &[&str]) -> Option<(f64, f64, f64)> {
        let left = Self::parse_local_coordinate(arg.first()?)?;
        let up = Self::parse_local_coordinate(arg.get(1)?)?;
        let forwards = Self::parse_local_coordinate(arg.get(2)?)?;
        Some((left, up, forwards))
    }

    fn parse_local_coordinate(value: &str) -> Option<f64> {
        let offset = value.strip_prefix('^')?;
        if offset.is_empty() {
            Some(0.0)
        } else {
            offset.parse::<f64>().ok()
        }
    }

    fn local_coordinates_to_position(
        left: f64,
        up: f64,
        forwards: f64,
        context: &CommandContext,
    ) -> DVec3 {
        let (yaw, pitch) = context.rotation.unwrap_or((0.0, 0.0));
        Self::local_coordinates_to_anchor_position(
            Self::anchor_position(context),
            (yaw, pitch),
            left,
            up,
            forwards,
        )
    }

    fn local_coordinates_to_anchor_position(
        source: DVec3,
        rotation: (f32, f32),
        left: f64,
        up: f64,
        forwards: f64,
    ) -> DVec3 {
        let (yaw, pitch) = rotation;
        let y_cos = ((yaw + 90.0) * PI / 180.0).cos();
        let y_sin = ((yaw + 90.0) * PI / 180.0).sin();
        let x_cos = (-pitch * PI / 180.0).cos();
        let x_sin = (-pitch * PI / 180.0).sin();
        let x_cos_up = ((-pitch + 90.0) * PI / 180.0).cos();
        let x_sin_up = ((-pitch + 90.0) * PI / 180.0).sin();
        let forwards_axis = DVec3::new(
            f64::from(y_cos * x_cos),
            f64::from(x_sin),
            f64::from(y_sin * x_cos),
        );
        let up_axis = DVec3::new(
            f64::from(y_cos * x_cos_up),
            f64::from(x_sin_up),
            f64::from(y_sin * x_cos_up),
        );
        let left_axis = -forwards_axis.cross(up_axis);

        source + left_axis * left + up_axis * up + forwards_axis * forwards
    }

    fn anchor_position(context: &CommandContext) -> DVec3 {
        if matches!(context.anchor, EntityAnchor::Eyes)
            && let Some(player) = &context.player
        {
            return DVec3::new(context.position.x, player.get_eye_y(), context.position.z);
        }

        context.position
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;

    use super::Helper;

    fn local_position(
        position: DVec3,
        rotation: (f32, f32),
        left: f64,
        up: f64,
        forwards: f64,
    ) -> DVec3 {
        Helper::local_coordinates_to_anchor_position(position, rotation, left, up, forwards)
    }

    #[test]
    fn incomplete_local_coordinates_are_rejected_before_context_use() {
        assert!(Helper::parse_local_coordinate_triplet(&["^"]).is_none());
        assert!(Helper::parse_local_coordinate_triplet(&["^", "^"]).is_none());
    }

    #[test]
    fn mixed_local_and_world_coordinates_are_rejected() {
        assert!(Helper::parse_local_coordinate_triplet(&["^", "^", "0"]).is_none());
        assert!(Helper::parse_local_coordinate_triplet(&["^", "~", "^"]).is_none());
    }

    #[test]
    fn local_coordinates_use_vanilla_yaw_axes() {
        let position = DVec3::new(10.0, 64.0, 20.0);

        assert!(
            (local_position(position, (0.0, 0.0), 0.0, 0.0, 2.0) - DVec3::new(10.0, 64.0, 22.0))
                .length()
                < 1.0e-5
        );
        assert!(
            (local_position(position, (90.0, 0.0), 0.0, 0.0, 2.0) - DVec3::new(8.0, 64.0, 20.0))
                .length()
                < 1.0e-5
        );
        assert!(
            (local_position(position, (0.0, 0.0), 2.0, 0.0, 0.0) - DVec3::new(12.0, 64.0, 20.0))
                .length()
                < 1.0e-5
        );
    }
}
