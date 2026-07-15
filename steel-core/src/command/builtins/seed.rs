use steel_utils::{Identifier, translations};
use text_components::{
    Modifier, TextComponent,
    format::Color,
    interactivity::{ClickEvent, HoverEvent},
};

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{CommandSource, SteelCommandContext, SteelCommandRuntime, literal},
    registration::CommandRegistration,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("seed"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("seed").executes(send_seed)
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Command executors use a shared fallible callback signature."
)]
fn send_seed(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let seed = context.source().world().seed();
    let seed_text = seed.to_string();
    let message = translations::COMMANDS_SEED_SUCCESS
        .message([TextComponent::from(seed_text.clone())
            .color(Color::Green)
            .hover_event(HoverEvent::show_text(&translations::CHAT_COPY_CLICK))
            .click_event(ClickEvent::CopyToClipboard {
                value: seed_text.into(),
            })])
        .component();
    context.source().send_success(&message, false);
    Ok(seed as i32)
}
