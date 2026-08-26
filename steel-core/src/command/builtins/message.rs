//! Vanilla chat messaging commands: `/say`, `/me`, and `/msg` (`/tell`, `/w`).
//!
//! Mirrors `SayCommand`, `MeCommand`, and `MessageCommand`, all of which resolve their
//! message through `MessageArgument.resolveChatMessage` and deliver it bound to the matching
//! vanilla chat type (`say_command`, `emote_command`, `msg_command_incoming`,
//! `msg_command_outgoing`). The client applies the chat-type decoration (for example
//! `[%s] %s` for `say_command`) from the bound chat type.
//!
//! When the client signed the message argument (`ServerboundChatCommandSignedPacket`) the
//! verified signature is bound to the command source, and the message goes out as signed
//! player chat so recipients can verify it. Otherwise — console, RCON, `/execute`, or an
//! unsigned command packet — it goes out as disguised chat, exactly as vanilla does for an
//! unsigned `PlayerChatMessage`.

use std::sync::Arc;

use steel_protocol::packets::game::{CDisguisedChat, CPlayerChat, ChatTypeBound, FilterType};
use steel_registry::RegistryEntry;
use steel_registry::chat_type::ChatTypeRef;
use steel_registry::vanilla_chat_types;
use steel_utils::Identifier;
use steel_utils::translations;
use text_components::TextComponent;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SignedArgument, SteelArgumentType, SteelCommandContext, SteelCommandRuntime,
        argument, literal,
    },
    registration::CommandRegistration,
    sender::CommandSender,
};
use crate::entity::Entity;
use crate::player::Player;

/// The Brigadier name of the signable message argument shared by all three commands.
const MESSAGE_ARGUMENT: &str = "message";

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
    literal("say").then(argument(MESSAGE_ARGUMENT, SteelArgumentType::message()).executes(say))
}

fn me_command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("me").then(argument(MESSAGE_ARGUMENT, SteelArgumentType::message()).executes(me))
}

fn msg_command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("msg").then(
        argument("targets", SteelArgumentType::players())
            .then(argument(MESSAGE_ARGUMENT, SteelArgumentType::message()).executes(msg)),
    )
}

fn say(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let (sender_name, message) = broadcast_chat_command(context, &vanilla_chat_types::SAY_COMMAND)?;
    let console_line = translations::CHAT_TYPE_ANNOUNCEMENT
        .message([sender_name, message])
        .component();
    CommandSender::Console.send_message(&console_line);
    // Vanilla `SayCommand` returns 1, not the recipient count.
    Ok(1)
}

fn me(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let (sender_name, message) =
        broadcast_chat_command(context, &vanilla_chat_types::EMOTE_COMMAND)?;
    let console_line = translations::CHAT_TYPE_EMOTE
        .message([sender_name, message])
        .component();
    CommandSender::Console.send_message(&console_line);
    // Vanilla `EmoteCommands` returns 1, not the recipient count.
    Ok(1)
}

fn msg(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    let message = resolve_chat_message(context)?;
    let source = context.source();
    let sender_name = source_display_name(source);

    // Incoming whisper delivered to each target: "<sender> whispers to you: <message>".
    let incoming = ChatTypeBound {
        registry_id: vanilla_chat_types::MSG_COMMAND_INCOMING.id() as i32,
        sender_name,
        target_name: None,
    };

    for target in &targets {
        send_chat_message(source, target, &message, incoming.clone());
        echo_outgoing(source, target, &message);
    }

    command_count(targets.len())
}

/// Delivers one command message to `recipient`, signed when the argument carried a
/// verified signature and disguised otherwise.
fn send_chat_message(
    source: &CommandSource,
    recipient: &Arc<Player>,
    message: &ChatMessage,
    chat_type: ChatTypeBound,
) {
    if let (Some(signed), Some(sender)) = (&message.signed, source.player()) {
        let mut packet = signed_chat_packet(sender, signed, &message.component, chat_type);
        packet.global_index = recipient.get_and_increment_messages_received();
        recipient.send_packet(packet);
        recipient.track_incoming_signed_message(signed.last_seen(), signed.signature());
    } else {
        recipient.send_packet(CDisguisedChat {
            message: message.component.clone(),
            chat_type,
        });
    }
}

/// Resolves the message once against the source and broadcasts it bound to
/// `chat_type`, returning the sender name and resolved message for the console line.
fn broadcast_chat_command(
    context: &SteelCommandContext<CommandSource>,
    chat_type: ChatTypeRef,
) -> Result<(TextComponent, TextComponent), CommandSyntaxError> {
    let source = context.source();
    let message = resolve_chat_message(context)?;
    let sender_name = source_display_name(source);
    let bound = ChatTypeBound {
        registry_id: chat_type.id() as i32,
        sender_name: sender_name.clone(),
        target_name: None,
    };

    if let (Some(signed), Some(sender)) = (&message.signed, source.player()) {
        let packet = signed_chat_packet(sender, signed, &message.component, bound);
        for world in source.server().worlds.values() {
            world.broadcast_chat(
                packet.clone(),
                Arc::clone(sender),
                signed.last_seen().clone(),
                signed.signature(),
            );
        }
    } else {
        let packet = CDisguisedChat {
            message: message.component.clone(),
            chat_type: bound,
        };
        for player in &source.server().get_players() {
            player.send_packet(packet.clone());
        }
    }

    Ok((sender_name, message.component))
}

/// Builds the signed `CPlayerChat` for a verified signed command argument.
///
/// The signed payload is the raw argument text the client signed; the selector-resolved
/// component travels as `unsigned_content` so recipients still render the resolved names,
/// matching vanilla's `PlayerChatMessage.withUnsignedContent`.
fn signed_chat_packet(
    sender: &Arc<Player>,
    signed: &SignedArgument,
    resolved: &TextComponent,
    chat_type: ChatTypeBound,
) -> CPlayerChat {
    let index = sender.next_chat_message_index();
    CPlayerChat::new(
        0,
        sender.gameprofile.id,
        index,
        signed
            .signature()
            .map(|signature| Box::new(*signature) as Box<[u8]>),
        signed.content().to_owned(),
        signed.timestamp(),
        signed.salt(),
        Box::new([]),
        Some(resolved.clone()),
        FilterType::PassThrough,
        chat_type,
    )
}

/// Delivers the "You whisper to <target>: <message>" echo back to the source.
///
/// The `msg_command_outgoing` decoration is keyed on `[target, message]`, so the
/// recipient's display name is bound as the chat type's target. Player sources
/// receive a chat packet; non-player sources (console) get a logged line.
fn echo_outgoing(source: &CommandSource, target: &Arc<Player>, message: &ChatMessage) {
    let target_name = target.display_name();
    if let Some(player) = source.player() {
        send_chat_message(
            source,
            player,
            message,
            ChatTypeBound {
                registry_id: vanilla_chat_types::MSG_COMMAND_OUTGOING.id() as i32,
                sender_name: source_display_name(source),
                target_name: Some(target_name),
            },
        );
    } else {
        let console_line = translations::COMMANDS_MESSAGE_DISPLAY_OUTGOING
            .message([target_name, message.component.clone()])
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

/// A command message ready for delivery: the selector-resolved component plus the verified
/// signature the client bound to the `message` argument, when there is one.
///
/// Equivalent to what vanilla's `MessageArgument.resolveChatMessage` hands to its callback.
struct ChatMessage {
    component: TextComponent,
    signed: Option<SignedArgument>,
}

fn resolve_chat_message(
    context: &SteelCommandContext<CommandSource>,
) -> Result<ChatMessage, CommandSyntaxError> {
    let source = context.source();
    Ok(ChatMessage {
        component: context.message(MESSAGE_ARGUMENT)?.resolve(source)?,
        signed: source.signed_argument(MESSAGE_ARGUMENT).cloned(),
    })
}

fn command_count(targets: usize) -> Result<i32, CommandSyntaxError> {
    i32::try_from(targets).map_err(|_| {
        CommandSyntaxError::dynamic("Message target count exceeds the command result range")
    })
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use super::super::create_dispatcher;
    use super::command_count;
    use crate::command::brigadier::{CommandDispatcher, NodeId};
    use crate::command::execution::{CommandSource, SteelArgumentType, SteelCommandRuntime};

    type TestDispatcher = CommandDispatcher<CommandSource, SteelCommandRuntime>;

    fn dispatcher() -> TestDispatcher {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        dispatcher
    }

    fn root_named(dispatcher: &TestDispatcher, name: &str) -> NodeId {
        let Some(node) = dispatcher.children(dispatcher.root()).and_then(|children| {
            children.iter().copied().find(|child| {
                dispatcher
                    .node(*child)
                    .is_some_and(|node| node.name() == name)
            })
        }) else {
            panic!("{name} root should exist");
        };
        node
    }

    /// Returns the single child of `node`, panicking if the shape is not a lone argument.
    fn only_child(dispatcher: &TestDispatcher, node: NodeId, label: &str) -> NodeId {
        let Some(child) = dispatcher
            .children(node)
            .and_then(|children| children.first())
            .copied()
        else {
            panic!("{label} should have exactly one child");
        };
        child
    }

    fn assert_message_leaf(dispatcher: &TestDispatcher, node: NodeId, label: &str) {
        let Some(message) = dispatcher.node(node) else {
            panic!("{label} node should exist");
        };
        // The dedicated vanilla message argument, never the generic component argument.
        assert_eq!(
            message.argument_type(),
            Some(&SteelArgumentType::message()),
            "{label}"
        );
        assert_ne!(
            message.argument_type(),
            Some(&SteelArgumentType::component()),
            "{label}"
        );
        assert!(message.is_executable(), "{label}");
    }

    #[test]
    fn say_is_restricted_and_takes_a_message_argument() {
        let dispatcher = dispatcher();
        let say = root_named(&dispatcher, "say");
        let Some(root) = dispatcher.node(say) else {
            panic!("say root node should exist");
        };
        // Vanilla `/say` requires permission level 2.
        assert!(root.is_restricted());

        assert_message_leaf(
            &dispatcher,
            only_child(&dispatcher, say, "say"),
            "say message",
        );
    }

    #[test]
    fn me_is_available_to_everyone_and_takes_a_message_argument() {
        let dispatcher = dispatcher();
        let me = root_named(&dispatcher, "me");
        let Some(root) = dispatcher.node(me) else {
            panic!("me root node should exist");
        };
        // Vanilla `/me` is permission level 0.
        assert!(!root.is_restricted());

        assert_message_leaf(&dispatcher, only_child(&dispatcher, me, "me"), "me message");
    }

    #[test]
    fn msg_and_its_aliases_target_players_and_take_a_message_argument() {
        let dispatcher = dispatcher();

        // `/msg`, `/tell`, and `/w` all register the same tree at permission level 0.
        for name in ["msg", "tell", "w"] {
            let root = root_named(&dispatcher, name);
            let Some(node) = dispatcher.node(root) else {
                panic!("{name} root node should exist");
            };
            assert!(!node.is_restricted(), "{name}");

            let targets = only_child(&dispatcher, root, name);
            assert_eq!(
                dispatcher
                    .node(targets)
                    .and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::players()),
                "{name} targets"
            );

            assert_message_leaf(
                &dispatcher,
                only_child(&dispatcher, targets, name),
                &format!("{name} message"),
            );
        }
    }

    #[test]
    fn msg_returns_its_target_count() {
        // Vanilla `/msg` returns the number of selected targets, unlike `/say` and `/me`
        // which always return 1.
        assert_eq!(command_count(0), Ok(0));
        assert_eq!(command_count(1), Ok(1));
        assert_eq!(command_count(37), Ok(37));

        // A target count that cannot be a command result is reported rather than truncated.
        assert!(command_count(usize::MAX).is_err());
    }
}
