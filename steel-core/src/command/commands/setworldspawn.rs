//! Handler for the `setworldspawn` command.

use std::borrow::Cow;

use text_components::TextComponent;
use text_components::translation::TranslatedMessage;

use crate::command::{
    arguments::block_pos::BlockPosArgument,
    arguments::rotation::RotationArgument,
    commands::{CommandHandlerBuilder, CommandHandlerDyn, argument},
    context::CommandContext,
    error::CommandError,
};
use crate::level_data::RespawnData;
use crate::world::World;
use steel_utils::BlockPos;

type PositionRotationArgs = (((), BlockPos), (f32, f32));

/// Handler for the `setworldspawn` command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["setworldspawn"],
        "Sets the world spawn.",
        "minecraft:command.setworldspawn",
    )
    .executes(|(), context: &mut CommandContext| {
        set_spawn(context, BlockPos::from(context.position), (0.0, 0.0))
    })
    .then(
        argument("pos", BlockPosArgument)
            .executes(|((), pos), context: &mut CommandContext| set_spawn(context, pos, (0.0, 0.0)))
            .then(argument("rotation", RotationArgument).executes(
                |(((), pos), rotation): PositionRotationArgs, context: &mut CommandContext| {
                    set_spawn(context, pos, rotation)
                },
            )),
    )
}

fn set_spawn(
    context: &mut CommandContext,
    pos: BlockPos,
    rotation: (f32, f32),
) -> Result<(), CommandError> {
    if !World::is_in_spawnable_bounds(pos) {
        return Err(CommandError::CommandFailed(Box::new(translated(
            "argument.pos.outofbounds",
            [],
        ))));
    }

    let respawn_data = RespawnData::of(context.world.key.clone(), pos, rotation.0, rotation.1);
    context
        .server
        .set_respawn_data(respawn_data.clone())
        .map_err(command_failed)?;

    context.sender.send_message(&translated(
        "commands.setworldspawn.success",
        [
            TextComponent::from(pos.x().to_string()),
            TextComponent::from(pos.y().to_string()),
            TextComponent::from(pos.z().to_string()),
            TextComponent::from(respawn_data.yaw.to_string()),
            TextComponent::from(respawn_data.pitch.to_string()),
            TextComponent::from(context.world.key.to_string()),
        ],
    ));

    Ok(())
}

fn command_failed(error: String) -> CommandError {
    CommandError::CommandFailed(Box::new(TextComponent::from(error)))
}

fn translated<const N: usize>(key: &'static str, args: [TextComponent; N]) -> TextComponent {
    TranslatedMessage {
        key: Cow::Borrowed(key),
        fallback: None,
        args: Some(Box::new(args)),
    }
    .component()
}
