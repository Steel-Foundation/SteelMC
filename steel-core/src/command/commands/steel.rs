//! Steel server commands: /steel tp overworld, /steel tp nether, /steel tp end

use crate::command::commands::{
    CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn, literal,
};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use crate::entity::SharedEntity;
use crate::portal::{DimensionChangeRequest, TeleportTransition};

/// Handler for the "steel" command group.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["steel"],
        "Steel server commands.",
        "minecraft:command.steel",
    )
    .then(
        literal("tp")
            .then(literal("overworld").executes(DimensionExecutor::Overworld))
            .then(literal("nether").executes(DimensionExecutor::Nether))
            .then(literal("end").executes(DimensionExecutor::End)),
    )
}

enum DimensionExecutor {
    Overworld,
    Nether,
    End,
}

impl CommandExecutor<()> for DimensionExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let player = context
            .player
            .as_ref()
            .ok_or_else(|| {
                CommandError::CommandFailed(Box::new(
                    "This command can only be used by players".into(),
                ))
            })?
            .clone();

        let mut pos = *player.position.lock();
        let rot = player.rotation.load();

        let target = match self {
            DimensionExecutor::Overworld => context.server.overworld().clone(),
            DimensionExecutor::Nether => {
                pos.y = 10.0;
                context.server.nether().cloned().ok_or_else(|| {
                    CommandError::CommandFailed(Box::new("Nether is not available.".into()))
                })?
            }
            DimensionExecutor::End => {
                pos.y = 10.0;
                context.server.the_end().cloned().ok_or_else(|| {
                    CommandError::CommandFailed(Box::new("The End is not available.".into()))
                })?
            }
        };

        context.server.queue_dimension_change(
            player as SharedEntity,
            DimensionChangeRequest::Computed(TeleportTransition {
                target_world: target,
                position: pos,
                rotation: rot,
                portal_cooldown: 0,
            }),
        );

        Ok(())
    }
}
