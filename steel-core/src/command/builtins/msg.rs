//! Vanilla msg | tell | w command.

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
use steel_utils::{Identifier, translations};
use text_components::Modifier;
use text_components::format::Color;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("msg"), |_| command())
        .alias("tell")
        .alias("w")
        .default_access()
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

    let incoming_chat_type =
        source.bind_chat_type(vanilla_chat_types::MSG_COMMAND_INCOMING.id() as i32);

    let outgoing_message = OutgoingChatMessage::from_command(source, "message", message.clone());

    for player in &targets {
        let outgoing_chat_type = source
            .bind_chat_type(vanilla_chat_types::MSG_COMMAND_OUTGOING.id() as i32)
            .with_target_name(player.display_name());

        if let Some(source_player) = source.player() {
            outgoing_message.send_to_player(source_player, &outgoing_chat_type);
        } else {
            source.send_system_message(
                &translations::COMMANDS_MESSAGE_DISPLAY_OUTGOING
                    .message([player.display_name(), message.clone()])
                    .component()
                    .color(Color::Gray)
                    .italic(true),
            );
        }

        outgoing_message.send_to_player(player, &incoming_chat_type);
    }

    Ok(targets.len() as i32)
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use super::super::create_dispatcher;
    use super::*;
    use crate::command::brigadier::{CommandDispatcher, NodeId};

    type Dispatcher = CommandDispatcher<CommandSource, SteelCommandRuntime>;

    fn child(dispatcher: &Dispatcher, parent: NodeId, name: &str) -> NodeId {
        let Some(children) = dispatcher.children(parent) else {
            panic!("parent node should exist");
        };
        let Some(child) = children.iter().copied().find(|child| {
            dispatcher
                .node(*child)
                .is_some_and(|node| node.name() == name)
        }) else {
            panic!("child {name} should exist");
        };
        child
    }

    #[test]
    fn msg_graph_matches_vanilla_argument_shape() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };

        for root_name in ["msg", "tell", "w"] {
            let root = child(&dispatcher, dispatcher.root(), root_name);

            let targets = child(&dispatcher, root, "targets");
            assert_eq!(
                dispatcher
                    .node(targets)
                    .and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::players())
            );

            let message = child(&dispatcher, targets, "message");
            assert_eq!(
                dispatcher
                    .node(message)
                    .and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::message())
            );
            assert!(matches!(
                dispatcher.node(message),
                Some(node) if node.is_executable()
            ));
        }
    }
}
