//! Vanilla chat messaging commands: `/say`, `/me`, and `/msg` (`/tell`, `/w`).
//!
//! Mirrors `SayCommand`, `MeCommand`, and `MessageCommand`. Command-generated
//! chat is unsigned, so it is delivered as disguised chat bound to the matching
//! vanilla chat type (`say_command`, `emote_command`, `msg_command_incoming`,
//! `msg_command_outgoing`). The client applies the chat-type decoration (for
//! example `[%s] %s` for `say_command`) from the bound chat type.

use std::sync::Arc;

use steel_protocol::packets::game::{CDisguisedChat, ChatTypeBound};
use steel_registry::RegistryEntry;
use steel_registry::chat_type::ChatTypeRef;
use steel_registry::vanilla_chat_types;
use steel_utils::Identifier;
use steel_utils::translations;
use text_components::TextComponent;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, CommandTextResolver, SteelArgumentType, SteelCommandContext,
        SteelCommandRuntime, argument, literal,
    },
    registration::CommandRegistration,
    sender::CommandSender,
};
use crate::entity::Entity;
use crate::player::Player;

/// `/say` is operator-only in vanilla (permission level 2).
pub(super) fn say_registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("say"), |_| say_command())
}

/// `/me` is usable by every player in vanilla (permission level 0).
pub(super) fn me_registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("me"), |_| me_command()).default_access()
}

/// `/msg` (aliases `/tell`, `/w`) is usable by every player in vanilla.
pub(super) fn msg_registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("msg"), |_| msg_command())
        .alias("tell")
        .alias("w")
        .default_access()
}

fn say_command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("say").then(argument("message", SteelArgumentType::message()).executes(say))
}

fn me_command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("me").then(argument("message", SteelArgumentType::message()).executes(me))
}

fn msg_command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("msg").then(
        argument("targets", SteelArgumentType::players())
            .then(argument("message", SteelArgumentType::message()).executes(msg)),
    )
}

fn say(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let (sender_name, message, recipients) =
        broadcast_chat_command(context, &vanilla_chat_types::SAY_COMMAND)?;
    let console_line = translations::CHAT_TYPE_ANNOUNCEMENT
        .message([sender_name, message])
        .component();
    CommandSender::Console.send_message(&console_line);
    command_count(recipients)
}

fn me(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let (sender_name, message, recipients) =
        broadcast_chat_command(context, &vanilla_chat_types::EMOTE_COMMAND)?;
    let console_line = translations::CHAT_TYPE_EMOTE
        .message([sender_name, message])
        .component();
    CommandSender::Console.send_message(&console_line);
    command_count(recipients)
}

fn msg(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    let message = resolve_message(context)?;
    let source = context.source();
    let sender_name = source_display_name(source);

    // Incoming whisper delivered to each target: "<sender> whispers to you: <message>".
    let incoming = CDisguisedChat {
        message: message.clone(),
        chat_type: ChatTypeBound {
            registry_id: vanilla_chat_types::MSG_COMMAND_INCOMING.id() as i32,
            sender_name,
            target_name: None,
        },
    };

    for target in &targets {
        target.send_packet(incoming.clone());
        echo_outgoing(source, target, &message);
    }

    command_count(targets.len())
}

/// Resolves the message once against the source and broadcasts it bound to
/// `chat_type`, returning the sender name, resolved message, and recipient count.
fn broadcast_chat_command(
    context: &SteelCommandContext<CommandSource>,
    chat_type: ChatTypeRef,
) -> Result<(TextComponent, TextComponent, usize), CommandSyntaxError> {
    let source = context.source();
    let message = resolve_message(context)?;
    let sender_name = source_display_name(source);
    let recipients = broadcast_disguised(source, chat_type, sender_name.clone(), &message);
    Ok((sender_name, message, recipients))
}

/// Broadcasts disguised chat bound to `chat_type` to every online player.
fn broadcast_disguised(
    source: &CommandSource,
    chat_type: ChatTypeRef,
    sender_name: TextComponent,
    message: &TextComponent,
) -> usize {
    let packet = CDisguisedChat {
        message: message.clone(),
        chat_type: ChatTypeBound {
            registry_id: chat_type.id() as i32,
            sender_name,
            target_name: None,
        },
    };
    let players = source.server().get_players();
    for player in &players {
        player.send_packet(packet.clone());
    }
    players.len()
}

/// Delivers the "You whisper to <target>: <message>" echo back to the source.
///
/// The `msg_command_outgoing` decoration is keyed on `[target, message]`, so the
/// recipient's display name is bound as the chat type's target. Player sources
/// receive a chat packet; non-player sources (console) get a logged line.
fn echo_outgoing(source: &CommandSource, target: &Arc<Player>, message: &TextComponent) {
    let target_name = target.display_name();
    if let Some(player) = source.player() {
        let outgoing = CDisguisedChat {
            message: message.clone(),
            chat_type: ChatTypeBound {
                registry_id: vanilla_chat_types::MSG_COMMAND_OUTGOING.id() as i32,
                sender_name: source_display_name(source),
                target_name: Some(target_name),
            },
        };
        player.send_packet(outgoing);
    } else {
        let console_line = translations::COMMANDS_MESSAGE_DISPLAY_OUTGOING
            .message([target_name, message.clone()])
            .component();
        CommandSender::Console.send_message(&console_line);
    }
}

/// Vanilla `CommandSourceStack.getDisplayName()` used to bind the chat sender.
fn source_display_name(source: &CommandSource) -> TextComponent {
    source.player().map_or_else(
        || TextComponent::plain(source.sender().to_string()),
        |player| player.display_name(),
    )
}

fn resolve_message(
    context: &SteelCommandContext<CommandSource>,
) -> Result<TextComponent, CommandSyntaxError> {
    let Some(message) = context.message("message") else {
        return Err(CommandSyntaxError::dynamic(
            "Parsed message argument is missing from the command context",
        ));
    };
    message.try_resolve(&CommandTextResolver::for_source(context.source()))
}

fn command_count(recipients: usize) -> Result<i32, CommandSyntaxError> {
    i32::try_from(recipients).map_err(|_| {
        CommandSyntaxError::dynamic("Message recipient count exceeds the command result range")
    })
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;

    use super::super::create_dispatcher;
    use crate::command::brigadier::{CommandDispatcher, NodeId};
    use crate::command::execution::{CommandSource, SteelArgumentType, SteelCommandRuntime};

    type TestDispatcher = CommandDispatcher<CommandSource, SteelCommandRuntime>;

    fn root_named(dispatcher: &TestDispatcher, name: &str) -> Option<NodeId> {
        dispatcher.children(dispatcher.root()).and_then(|children| {
            children.iter().copied().find(|child| {
                dispatcher
                    .node(*child)
                    .is_some_and(|node| node.name() == name)
            })
        })
    }

    #[test]
    fn say_is_restricted_and_takes_a_component_message() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let Some(say) = root_named(&dispatcher, "say") else {
            panic!("say root should exist");
        };
        let Some(root) = dispatcher.node(say) else {
            panic!("say root node should exist");
        };
        assert!(root.is_restricted());

        let Some(message) = dispatcher
            .children(say)
            .and_then(|children| children.first())
            .copied()
        else {
            panic!("say message should exist");
        };
        let Some(message) = dispatcher.node(message) else {
            panic!("say message node should exist");
        };
        // Expect the message argument (vanilla message argument)
        assert_eq!(message.argument_type(), Some(&SteelArgumentType::message()));
        assert!(message.is_executable());
    }

    #[test]
    fn me_is_available_to_everyone_and_takes_a_component_message() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let Some(me) = root_named(&dispatcher, "me") else {
            panic!("me root should exist");
        };
        let Some(root) = dispatcher.node(me) else {
            panic!("me root node should exist");
        };
        assert!(!root.is_restricted());

        let Some(message) = dispatcher
            .children(me)
            .and_then(|children| children.first())
            .copied()
        else {
            panic!("me message should exist");
        };
        let Some(message) = dispatcher.node(message) else {
            panic!("me message node should exist");
        };
        // Expect the message argument (vanilla message argument)
        assert_eq!(message.argument_type(), Some(&SteelArgumentType::message()));
        assert!(message.is_executable());
    }

    #[test]
    fn msg_targets_players_and_exposes_tell_and_w_aliases() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let Some(msg) = root_named(&dispatcher, "msg") else {
            panic!("msg root should exist");
        };
        let Some(root) = dispatcher.node(msg) else {
            panic!("msg root node should exist");
        };
        assert!(!root.is_restricted());

        // Aliases are registered as additional roots pointing at the same tree.
        assert!(root_named(&dispatcher, "tell").is_some());
        assert!(root_named(&dispatcher, "w").is_some());

        let Some(targets) = dispatcher
            .children(msg)
            .and_then(|children| children.first())
            .copied()
        else {
            panic!("msg targets should exist");
        };
        assert_eq!(
            dispatcher
                .node(targets)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::players())
        );

        let Some(message) = dispatcher
            .children(targets)
            .and_then(|children| children.first())
            .copied()
        else {
            panic!("msg message should exist");
        };
        let Some(message) = dispatcher.node(message) else {
            panic!("msg message node should exist");
        };
        // Expect the message argument (vanilla message argument)
        assert_eq!(message.argument_type(), Some(&SteelArgumentType::message()));
        assert!(message.is_executable());
    }
}
