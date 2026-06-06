//! Handler for the "teleport" command.
use std::sync::Arc;

use glam::DVec3;
use steel_utils::{BlockPos, translations};
use text_components::TextComponent;

use crate::{
    command::{
        arguments::{player::PlayerArgument, rotation::RotationArgument, vector3::Vector3Argument},
        commands::{CommandHandlerBuilder, CommandHandlerDyn, argument},
        context::CommandContext,
        error::CommandError,
    },
    entity::Entity,
    player::Player,
    world::World,
};

type MultipleRotationArgs = ((((), Vec<Arc<Player>>), DVec3), (f32, f32));
type MultipleEntityArgs = (((), Vec<Arc<Player>>), Vec<Arc<Player>>);

/// Handler for the "teleport" command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["tp", "teleport"],
        "Teleports the target(s) to the given location.",
        "minecraft:command.teleport",
    )
    .then(
        argument("targets", PlayerArgument::multiple())
            .then(
                argument("position", Vector3Argument)
                    .executes(
                        |(((), targets), pos): (((), Vec<Arc<Player>>), DVec3),
                         context: &mut CommandContext| {
                            let player = context
                                .sender
                                .get_player()
                                .ok_or(CommandError::InvalidRequirement)?;

                            teleport_to_pos(&targets, pos, player.rotation(), context)
                        },
                    )
                    .then(argument("rotation", RotationArgument).executes(
                        |((((), targets), pos), rotation): MultipleRotationArgs,
                         context: &mut CommandContext| {
                            teleport_to_pos(&targets, pos, rotation, context)
                        },
                    )),
            )
            .then(argument("destination", PlayerArgument::one()).executes(
                |(((), targets), destination): MultipleEntityArgs, context: &mut CommandContext| {
                    teleport_to_player(&targets, &destination, context)
                },
            )),
    )
    .then(
        argument("location", Vector3Argument)
            .executes(|((), pos), context: &mut CommandContext| {
                let player = context
                    .player
                    .clone()
                    .ok_or(CommandError::InvalidRequirement)?;
                let rotation = player.rotation();

                teleport_to_pos(&[player], pos, rotation, context)
            })
            .then(argument("rotation", RotationArgument).executes(
                |(((), pos), rotation), context: &mut CommandContext| {
                    let player = context
                        .player
                        .clone()
                        .ok_or(CommandError::InvalidRequirement)?;

                    teleport_to_pos(&[player], pos, rotation, context)
                },
            )),
    )
}

fn teleport_to_pos(
    targets: &[Arc<Player>],
    pos: DVec3,
    rotation: (f32, f32),
    ctx: &mut CommandContext,
) -> Result<(), CommandError> {
    if !World::is_in_spawnable_bounds(BlockPos::from(pos)) {
        ctx.sender.send_message(
            &translations::COMMANDS_TELEPORT_INVALID_POSITION
                .message([] as [TextComponent; 0])
                .into(),
        );
        return Ok(());
    }

    let targets = current_players(targets, ctx)?;
    for player in &targets {
        teleport_player(player, pos.x, pos.y, pos.z, rotation.0, rotation.1)?;
    }

    if let [target] = targets.as_slice() {
        ctx.sender.send_message(
            &translations::COMMANDS_TELEPORT_SUCCESS_LOCATION_SINGLE
                .message([
                    TextComponent::from(target.gameprofile.name.clone()),
                    TextComponent::from(format!("{:.2}", pos.x)),
                    TextComponent::from(format!("{:.2}", pos.y)),
                    TextComponent::from(format!("{:.2}", pos.z)),
                ])
                .into(),
        );
    } else {
        ctx.sender.send_message(
            &translations::COMMANDS_TELEPORT_SUCCESS_LOCATION_MULTIPLE
                .message([
                    TextComponent::from(format!("{}", targets.len())),
                    TextComponent::from(format!("{:.2}", pos.x)),
                    TextComponent::from(format!("{:.2}", pos.y)),
                    TextComponent::from(format!("{:.2}", pos.z)),
                ])
                .into(),
        );
    }
    Ok(())
}

fn teleport_to_player(
    targets: &[Arc<Player>],
    destination: &[Arc<Player>],
    ctx: &mut CommandContext,
) -> Result<(), CommandError> {
    let Some(destination) = destination.first() else {
        return Err(no_player_found());
    };
    let destination = current_player(destination, ctx).ok_or_else(no_player_found)?;

    let pos = destination.position();
    let (yaw, pitch) = destination.rotation();

    let targets = current_players(targets, ctx)?;
    for player in &targets {
        teleport_player(player, pos.x, pos.y, pos.z, yaw, pitch)?;
    }

    if let [target] = targets.as_slice() {
        ctx.sender.send_message(
            &translations::COMMANDS_TELEPORT_SUCCESS_ENTITY_SINGLE
                .message([
                    TextComponent::from(target.gameprofile.name.clone()),
                    TextComponent::from(destination.gameprofile.name.clone()),
                ])
                .into(),
        );
    } else {
        ctx.sender.send_message(
            &translations::COMMANDS_TELEPORT_SUCCESS_ENTITY_MULTIPLE
                .message([
                    TextComponent::from(format!("{}", targets.len())),
                    TextComponent::from(destination.gameprofile.name.clone()),
                ])
                .into(),
        );
    }
    Ok(())
}

fn current_players(
    players: &[Arc<Player>],
    ctx: &CommandContext,
) -> Result<Vec<Arc<Player>>, CommandError> {
    let current_players = ctx.server.get_players();
    let players = players
        .iter()
        .filter_map(|player| {
            current_players
                .iter()
                .find(|current| current.uuid() == player.uuid())
                .cloned()
        })
        .collect::<Vec<_>>();
    if players.is_empty() {
        return Err(no_player_found());
    }
    Ok(players)
}

fn current_player(player: &Player, ctx: &CommandContext) -> Option<Arc<Player>> {
    ctx.server
        .get_players()
        .into_iter()
        .find(|current| current.uuid() == player.uuid())
}

fn no_player_found() -> CommandError {
    CommandError::CommandFailed(Box::new(TextComponent::const_plain("No player was found")))
}

fn teleport_player(
    player: &Player,
    x: f64,
    y: f64,
    z: f64,
    yaw: f32,
    pitch: f32,
) -> Result<(), CommandError> {
    player.teleport(x, y, z, yaw, pitch).map_err(|error| {
        CommandError::CommandFailed(Box::new(TextComponent::plain(format!(
            "Failed to teleport {}: {error}",
            player.gameprofile.name
        ))))
    })?;
    player.reset_flying_ticks();

    if !player.is_fall_flying() {
        let velocity = player.velocity();
        player.set_velocity(DVec3::new(velocity.x, 0.0, velocity.z));
        player.set_on_ground(true);
    }

    Ok(())
}
