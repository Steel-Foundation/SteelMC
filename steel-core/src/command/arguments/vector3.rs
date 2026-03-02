//! A vector3 argument.
use steel_protocol::packets::game::{ArgumentType, SuggestionType};
use steel_utils::math::Vector3;

use crate::command::arguments::{CommandArgument, Helper};
use crate::command::context::CommandContext;

/// A vector3 argument.
pub struct Vector3Argument;

impl CommandArgument for Vector3Argument {
    type Output = Vector3<f64>;

    fn parse<'a>(
        &self,
        arg: &'a [&'a str],
        context: &mut CommandContext,
    ) -> Option<(&'a [&'a str], Self::Output)> {
        let x = Helper::parse_relative_coordinate::<false>(arg.first()?, Some(context.position.x))?;
        let y = Helper::parse_relative_coordinate::<true>(arg.get(1)?, Some(context.position.y))?;
        let z = Helper::parse_relative_coordinate::<false>(arg.get(2)?, Some(context.position.z))?;

        Some((&arg[3..], Vector3::new(x, y, z)))
    }

    fn usage(&self) -> (ArgumentType, Option<SuggestionType>) {
        (ArgumentType::Vec3, None)
    }
}
