//! Vanilla damage entity command.

use super::super::{
    brigadier::CommandNodeBuilder,
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use log::info;
use steel_protocol::packets::game::CDisguisedChat;
use steel_utils::Identifier;
use text_components::TextComponent;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("say"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("say").then(argument("message", SteelArgumentType::message()).executes(
        |ctx: &SteelCommandContext<CommandSource>| {
            let message = ctx.message("message")?.to_string();
            if let Some(signed_arg) = ctx.source().signing_context() {
                info!("Signing message context : {:?}", signed_arg);
            }

            /*for world in ctx.source().server().worlds.values() {
                world.broadcast_chat(&outgoing, &chat_type);
            }*/

            for player in ctx.source().server().get_players() {
                player.send_message(&TextComponent::plain(message.clone()));
            }

            Ok(1)
        },
    ))
}
