//! Steel server commands: /steel tp <targets> <world>

use std::sync::Arc;

use text_components::TextComponent;

use crate::command::arguments::player::PlayerArgument;
use crate::command::arguments::world::WorldArgument;
use crate::command::commands::{CommandHandlerBuilder, CommandHandlerDyn, argument, literal};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use crate::entity::SharedEntity;
use crate::player::Player;
use crate::portal::WorldChangeRequest;
use crate::world::World;

/// Handler for the "steel" command group.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["steel"],
        "Steel server commands.",
        "minecraft:command.steel",
    )
    .then(
        literal("tp").then(argument("targets", PlayerArgument::multiple()).then(
            argument("world", WorldArgument).executes(
                |(((), targets), world): (((), Vec<Arc<Player>>), Arc<World>),
                 context: &mut CommandContext|
                 -> Result<(), CommandError> {
                    let dim_name = &world.key;
                    let count = targets.len();

                    for target in &targets {
                        if target.is_domain_switching() {
                            return Err(CommandError::CommandFailed(Box::new(
                                TextComponent::plain(format!(
                                    "{} is already switching domains",
                                    target.gameprofile.name
                                )),
                            )));
                        }
                    }

                    for target in &targets {
                        let current_world = target.get_world();
                        if current_world.domain() == world.domain() {
                            context.server.queue_world_change(
                                target.clone() as SharedEntity,
                                WorldChangeRequest::WorldSpawn {
                                    target_world: world.clone(),
                                },
                            );
                        } else {
                            context
                                .server
                                .queue_domain_switch_to_world(target.clone(), world.clone())
                                .map_err(|error| {
                                    CommandError::CommandFailed(Box::new(TextComponent::plain(
                                        error,
                                    )))
                                })?;
                        }
                    }

                    let msg = if count == 1 {
                        format!(
                            "Teleporting {} to {}",
                            targets[0].gameprofile.name, dim_name
                        )
                    } else {
                        format!("Teleporting {count} players to {dim_name}")
                    };
                    context.sender.send_message(&TextComponent::from(msg));

                    Ok(())
                },
            ),
        )),
    )
}
