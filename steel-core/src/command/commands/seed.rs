//! Handler for the "seed" command.
use crate::command::commands::{CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use crate::config::STEEL_CONFIG;
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
        context.sender.send_message(
            &translations::COMMANDS_SEED_SUCCESS
                .message([TextComponent::plain(&STEEL_CONFIG.seed)
                    .color(Color::Green)
                    .hover_event(HoverEvent::show_text(&translations::CHAT_COPY_CLICK))
                    .click_event(ClickEvent::CopyToClipboard {
                        value: (&STEEL_CONFIG.seed).into(),
                    })])
                .component(),
        );
        Ok(())
    }
}
