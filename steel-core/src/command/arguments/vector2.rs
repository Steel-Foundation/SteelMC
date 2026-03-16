//! A vector2 argument.
use glam::DVec2;
use steel_protocol::packets::game::{ArgumentType, SuggestionType};

use crate::command::arguments::{CommandArgument, Helper};
use crate::command::context::CommandContext;

/// A vector2 argument.
pub struct Vector2Argument;

impl CommandArgument for Vector2Argument {
    type Output = DVec2;

    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)> {
        let x = Helper::parse_relative_coordinate::<false>(arg.first()?, Some(context.position.x))?;
        let z = Helper::parse_relative_coordinate::<false>(arg.get(1)?, Some(context.position.z))?;

        Some((&arg[2..], DVec2::new(x, z)))
    }

    fn usage(&self) -> (ArgumentType, Option<SuggestionType>) {
        (ArgumentType::Vec2, None)
    }
}
