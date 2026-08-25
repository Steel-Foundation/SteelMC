//! The vanilla `/setidletimeout` command.

use crate::command::brigadier::{ArgumentType, CommandNodeBuilder, CommandSyntaxError};
use crate::command::execution::{
    CommandSource, SteelCommandContext, SteelCommandRuntime, argument, literal,
};
use crate::command::registration::CommandRegistration;
use std::sync::atomic::Ordering;
use steel_utils::Identifier;
use steel_utils::translations::{
    COMMANDS_SETIDLETIMEOUT_SUCCESS, COMMANDS_SETIDLETIMEOUT_SUCCESS_DISABLED,
};
use text_components::TextComponent;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("setidletimeout"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("setidletimeout")
        .then(argument("minutes", ArgumentType::integer(0, i32::MAX)).executes(set_idle_timeout))
}

fn set_idle_timeout(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let time = context.integer("minutes")?;

    context
        .source()
        .server()
        .player_idle_timeout
        .store(time, Ordering::Relaxed);

    if time > 0 {
        context.source().send_success(
            &COMMANDS_SETIDLETIMEOUT_SUCCESS
                .message([TextComponent::plain(time.to_string())])
                .component(),
            true,
        );
    } else {
        context.source().send_success(
            &TextComponent::from(&COMMANDS_SETIDLETIMEOUT_SUCCESS_DISABLED),
            true,
        );
    }

    Ok(time)
}
