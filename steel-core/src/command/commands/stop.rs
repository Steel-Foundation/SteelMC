//! Handler for the "stop" command.
use crate::command::commands::{CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;

/// Handler for the "stop" command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(&["stop"], "Stops the server.", "minecraft:command.stop")
        .executes(StopCommandExecutor)
}

struct StopCommandExecutor;
impl CommandExecutor<()> for StopCommandExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        context.server.cancel_token.cancel();
        Ok(())
    }
}
