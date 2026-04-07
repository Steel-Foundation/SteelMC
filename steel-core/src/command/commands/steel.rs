//! Steel server commands: /steel tp <targets> <dimension>

use std::sync::Arc;

use text_components::TextComponent;

use crate::command::arguments::dimension::DimensionArgument;
use crate::command::arguments::player::PlayerArgument;
use crate::command::commands::{CommandHandlerBuilder, CommandHandlerDyn, argument, literal};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use crate::entity::SharedEntity;
use crate::player::Player;
use crate::portal::{DimensionChangeRequest, TeleportTransition};
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
            argument("dimension", DimensionArgument).executes(
                |(((), targets), world): (((), Vec<Arc<Player>>), Arc<World>),
                 context: &mut CommandContext|
                 -> Result<(), CommandError> {
                    let dim_name = &world.dimension.key;
                    let count = targets.len();

                    for target in &targets {
                        let pos = *target.position.lock();
                        let rot = target.rotation.load();
                        context.server.queue_dimension_change(
                            target.clone() as SharedEntity,
                            DimensionChangeRequest::Computed(TeleportTransition {
                                target_world: world.clone(),
                                position: pos,
                                rotation: rot,
                                portal_cooldown: 0,
                            }),
                        );
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
