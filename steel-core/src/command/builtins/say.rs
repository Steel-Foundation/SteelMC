//! Vanilla damage entity command.

use super::super::{
    brigadier::CommandNodeBuilder,
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::player::chat::OutgoingChatMessage;
use steel_registry::{RegistryEntry, vanilla_chat_types};
use steel_utils::Identifier;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("say"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("say").then(argument("message", SteelArgumentType::message()).executes(
        |ctx: &SteelCommandContext<CommandSource>| {
            let source = ctx.source();

            let message = ctx.message("message")?.to_string();

            let chat_type = source.bind_chat_type(vanilla_chat_types::SAY_COMMAND.id() as i32);
            let outgoing = OutgoingChatMessage::from_command(source, "message", message);

            for world in source.server().worlds.values() {
                world.broadcast_chat(&outgoing, &chat_type);
            }

            Ok(1)
        },
    ))
}
