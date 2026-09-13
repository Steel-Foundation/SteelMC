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
use steel_protocol::packets::game::{CPlayerChat, ChatTypeBound, FilterType};
use steel_registry::{RegistryEntry, vanilla_chat_types};
use steel_utils::Identifier;
use text_components::interactivity::{ClickEvent, HoverEvent};
use text_components::{Modifier, TextComponent};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("say"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("say").then(argument("message", SteelArgumentType::message()).executes(
        |ctx: &SteelCommandContext<CommandSource>| {
            let source = ctx.source();
            let message = ctx.message("message")?.to_string();

            let component_message: TextComponent = TextComponent::plain(message.clone());

            let (outgoing, chat_type) = match source.sender().get_player() {
                Some(player) => {
                    if !ctx.source().server().enforces_secure_chat() {
                        let chat_type = ChatTypeBound {
                            registry_id: vanilla_chat_types::SAY_COMMAND.id() as i32,
                            sender_name: TextComponent::plain(source.sender().to_string()), // Command sender implement Display
                            target_name: None,
                        };
                        (OutgoingChatMessage::Disguised {
                            content: component_message,
                        }, chat_type)
                    } else {
                        let signing_ctx = source.signing_context();
                        let raw_sig = signing_ctx.and_then(|sc| sc.get_argument_signature("message"));

                        let (timestamp, salt) = signing_ctx
                            .map(|sc| (sc.timestamp, sc.salt))
                            .unwrap_or_else(|| {
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_millis() as u64;
                                (now, 0)
                            });

                        let sender_last_seen = signing_ctx
                            .map(|sc| sc.last_seen.clone())
                            .unwrap_or_default();

                        let sig_array = raw_sig.and_then(|s| {
                            if s.len() == 256 {
                                let mut arr = [0u8; 256];
                                arr.copy_from_slice(s);
                                Some(arr)
                            } else {
                                None
                            }
                        });

                        let sender_name = player.gameprofile.name.clone(); // or player.gameprofile.name
                        let sender_uuid = player.gameprofile.id;
                        let sender_index = signing_ctx.map(|sc| sc.sender_index).unwrap_or(0);

                        let chat_type = ChatTypeBound {
                            registry_id: vanilla_chat_types::SAY_COMMAND.id() as i32,
                            sender_name: TextComponent::plain(sender_name.clone())
                                .insertion(sender_name.clone())
                                .click_event(ClickEvent::suggest_command(format!("/tell {sender_name} ")))
                                .hover_event(HoverEvent::show_entity(
                                    "minecraft:player",
                                    sender_uuid,
                                    Some(sender_name),
                                )),
                            target_name: None,
                        };

                        let packet = CPlayerChat::new(
                            0, // Replaced after in broadcast_chat
                            player.gameprofile.id,
                            sender_index,
                            sig_array.map(|s| Box::new(s) as Box<[u8]>),
                            message,
                            timestamp as i64,
                            salt,
                            Box::new([]),
                            None,
                            FilterType::PassThrough,
                            chat_type.clone(),
                        );

                        (OutgoingChatMessage::Player {
                            packet,
                            signature: sig_array,
                            sender_last_seen,
                        }, chat_type)
                    }
                }
                None => {
                    let chat_type = ChatTypeBound {
                        registry_id: vanilla_chat_types::SAY_COMMAND.id() as i32,
                        sender_name: TextComponent::plain(source.sender().to_string()), // Command sender implement Display
                        target_name: None,
                    };
                    (OutgoingChatMessage::Disguised {
                        content: component_message,
                    }, chat_type)
                },
            };

            for world in source.server().worlds.values() {
                world.broadcast_chat(&outgoing, &chat_type);
            }

            Ok(1)
        },
    ))
}
