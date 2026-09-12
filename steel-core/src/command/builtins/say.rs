//! Vanilla message broadcasting command.

use steel_protocol::packets::game::CSystemChat;
use steel_utils::Identifier;

use super::super::{
    brigadier::{ArgumentType, CommandNodeBuilder, CommandSyntaxError},
    execution::{CommandSource, SteelCommandContext, SteelCommandRuntime, argument, literal},
    registration::CommandRegistration,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("say"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("say").then(argument("message", ArgumentType::greedy_string()).executes(say_message))
}

fn say_message(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let message = context.string("message")?.to_owned();
    let server = context.source().server().clone();

    server.broadcast_to_online(CSystemChat {
        content: message.into(),
        overlay: false,
    });

    Ok(1)
}
