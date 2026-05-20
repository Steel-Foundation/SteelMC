//! Handler for the "domain" command.

use crate::command::arguments::domain::DomainArgument;
use crate::command::commands::{CommandHandlerBuilder, CommandHandlerDyn, argument};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use text_components::TextComponent;

/// Handler for switching to another configured domain.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["domain"],
        "Switches to another configured domain.",
        "minecraft:command.domain",
    )
    .then(argument("domain", DomainArgument).executes(
        |((), domain): ((), String), context: &mut CommandContext| -> Result<(), CommandError> {
            let player = context
                .sender
                .get_player()
                .cloned()
                .ok_or(CommandError::InvalidRequirement)?;
            let server = context.server.clone();
            server
                .queue_domain_switch(player, domain.clone())
                .map_err(|error| {
                    CommandError::CommandFailed(Box::new(TextComponent::plain(error)))
                })?;

            context.sender.send_message(&TextComponent::plain(format!(
                "Switching to domain {domain}"
            )));
            Ok(())
        },
    ))
}
