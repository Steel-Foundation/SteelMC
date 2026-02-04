//! Handler for the "tellraw" command.
use crate::command::arguments::player::PlayerArgument;
use crate::command::arguments::text_component::TextComponentArgument;
use crate::command::commands::{
    CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn, argument,
};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use crate::command::sender::CommandSender;
use crate::player::Player;
use std::sync::Arc;
use text_components::TextComponent;

/// Handler for the "tellraw" command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["tellraw"],
        "Sends a JSON message to players.",
        "minecraft:command.tellraw",
    )
    .then(
        argument("targets", PlayerArgument::new())
            .then(argument("message", TextComponentArgument).executes(TellrawCommandExecutor)),
    )
}

struct TellrawCommandExecutor;

impl CommandExecutor<(((), Vec<Arc<Player>>), TextComponent)> for TellrawCommandExecutor {
    fn execute(
        &self,
        args: (((), Vec<Arc<Player>>), TextComponent),
        context: &mut CommandContext,
    ) -> Result<(), CommandError> {
        let sender = match &context.sender {
            CommandSender::Player(player) => &player.gameprofile.name,
            CommandSender::Console => "Console",
            CommandSender::Rcon => "Rcon",
        };
        log::info!("{}'s tellraw: {:p}", sender, args.1);
        for player in args.0.1 {
            player.send_message(&args.1);
        }
        Ok(())
    }
}
