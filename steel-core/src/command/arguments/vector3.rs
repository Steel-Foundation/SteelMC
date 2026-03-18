//! A vector3 argument.
use glam::DVec3;
use steel_protocol::packets::game::{ArgumentType, SuggestionType};

use crate::command::arguments::{CommandArgument, Helper};
use crate::command::context::CommandContext;

/// A vector3 argument.
pub struct Vector3Argument;

impl CommandArgument for Vector3Argument {
    type Output = DVec3;

    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)> {
        let x = Helper::parse_relative_coordinate::<false>(arg.first()?, Some(context.position.x))?;
        let y = Helper::parse_relative_coordinate::<true>(arg.get(1)?, Some(context.position.y))?;
        let z = Helper::parse_relative_coordinate::<false>(arg.get(2)?, Some(context.position.z))?;

        Some((&arg[3..], DVec3::new(x, y, z)))
    }

    fn usage(&self) -> (ArgumentType, Option<SuggestionType>) {
        (ArgumentType::Vec3, None)
    }
}
