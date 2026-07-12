use steel_utils::{Identifier, translations::COMMANDS_STOP_STOPPING};
use text_components::TextComponent;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{CommandSource, SteelCommandContext, SteelCommandRuntime, literal},
    registration::CommandRegistration,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("stop"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("stop").executes(stop_server)
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Command executors use a shared fallible callback signature."
)]
fn stop_server(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    context
        .source()
        .send_success(&TextComponent::from(&COMMANDS_STOP_STOPPING), true);
    context.source().server().cancel_token.cancel();
    Ok(1)
}
