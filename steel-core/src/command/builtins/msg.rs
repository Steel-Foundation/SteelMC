//! Vanilla player inventory clearing command.

use super::super::{
    brigadier::CommandNodeBuilder,
    execution::{CommandSource, SteelArgumentType, SteelCommandRuntime, argument, literal},
    registration::CommandRegistration,
};
use crate::command::brigadier::CommandSyntaxError;
use crate::command::execution::SteelCommandContext;
use crate::entity::Entity;
use crate::player::chat::OutgoingChatMessage;
use steel_registry::{RegistryEntry, vanilla_chat_types};
use steel_utils::Identifier;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("msg"), |_| command())
        .alias("tell")
        .alias("w")
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("msg").then(
        argument("targets", SteelArgumentType::players())
            .then(argument("message", SteelArgumentType::message()).executes(send_message)),
    )
}

fn send_message(ctx: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let source = ctx.source();

    let targets = ctx.players("targets")?;
    let message = ctx.message("message")?.resolve_component(source)?;

    // message to send to the target
    let incoming_chat_type =
        source.bind_chat_type(vanilla_chat_types::MSG_COMMAND_INCOMING.id() as i32);

    let outgoing = OutgoingChatMessage::from_command(source, "message", message);

    for player in &targets {
        // Message to send to the source
        let outcoming_chat_type = source
            .bind_chat_type(vanilla_chat_types::MSG_COMMAND_OUTGOING.id() as i32)
            .with_target_name(player.display_name());

        if let Some(source_player) = source.player() {
            outgoing.send_to_player(source_player, &outcoming_chat_type);
        }

        outgoing.send_to_player(player, &incoming_chat_type);
    }

    Ok(targets.len() as i32)
}
