//! Handler for the "list" command.
use crate::command::commands::literal;
use crate::command::{
    commands::{CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn},
    context::CommandContext,
    error::CommandError
    ,
};
use steel_utils::translations::COMMANDS_LIST_PLAYERS;

/// Handler for the "list" command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["list"],
        "Lists players on the server.",
        "minecraft:command.list",
    )
    .executes(ListExecutor)
        .then(literal("uuids").executes(ListWithUuidExecutor))
}

struct ListExecutor;

impl CommandExecutor<()> for ListExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let player_number = context.server.player_count();
        let max_player = context.server.config.max_players;
        let formatted_player_list: String = context.server.get_players()
            .iter()
            .map(|player| player.gameprofile.name.clone())
            .collect::<Vec<String>>()
            .join(", ");

        context.sender.send_message(
            &COMMANDS_LIST_PLAYERS
                .message([player_number.to_string(), max_player.to_string(), formatted_player_list])
                .into(),
        );

        Ok(())
    }
}

struct ListWithUuidExecutor;

impl CommandExecutor<()> for ListWithUuidExecutor {
    fn execute(
        &self,
        _args: (),
        _context: &mut CommandContext,
    ) -> Result<(), CommandError> {

        Ok(())
    }
}
