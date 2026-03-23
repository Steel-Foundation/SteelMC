//! Steel server commands: /steel tp <targets> <dimension>

use std::sync::Arc;

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
                    for target in targets {
                        let pos = *target.position.lock();
                        let rot = target.rotation.load();
                        context.server.queue_dimension_change(
                            target as SharedEntity,
                            DimensionChangeRequest::Computed(TeleportTransition {
                                target_world: world.clone(),
                                position: pos,
                                rotation: rot,
                                portal_cooldown: 0,
                            }),
                        );
                    }
                    Ok(())
                },
            ),
        )),
    )
}
