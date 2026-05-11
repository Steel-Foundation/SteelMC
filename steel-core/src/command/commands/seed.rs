//! Handler for the "seed" command.
use crate::command::commands::{CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use steel_utils::translations;
use text_components::format::Color;
use text_components::interactivity::{ClickEvent, HoverEvent};
use text_components::{Modifier, TextComponent};

/// Handler for the "seed" command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["seed"],
        "Displays the world seed.",
        "minecraft:command.seed",
    )
    .executes(SeedCommandExecutor)
}

struct SeedCommandExecutor;

impl CommandExecutor<()> for SeedCommandExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let seed = context.world.seed().to_string();
        context.sender.send_message(
            &translations::COMMANDS_SEED_SUCCESS
                .message([TextComponent::from(seed.clone())
                    .color(Color::Green)
                    .hover_event(HoverEvent::show_text(&translations::CHAT_COPY_CLICK))
                    .click_event(ClickEvent::CopyToClipboard { value: seed.into() })])
                .component(),
        );
        Ok(())
    }
}
