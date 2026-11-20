use std::io::{self, Write};

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::{C_DISGUISED_CHAT, C_PLAYER_CHAT};
use steel_registry::{RegistryEntry, vanilla_chat_types};
use steel_utils::{
    codec::{BitSet, VarInt},
    serial::{PrefixedWrite, WriteTo},
};
use text_components::{TextComponent, resolving::TextResolutor};
use uuid::Uuid;

use crate::packet_traits::ClientPacket;

/// Represents Minecraft's ChatType.Bound structure
/// Contains a registry holder + sender name + optional target name
#[derive(Clone, Debug, WriteTo)]
pub struct ChatTypeBound {
    /// Registry holder ID - written as (id + 1) per Minecraft's holder format
    #[write(as = RegistryHolder)]
    pub registry_id: i32,
    /// Sender name as NBT Component
    pub sender_name: TextComponent,
    /// Optional target name as NBT Component (bool-prefixed)
    pub target_name: Option<TextComponent>,
}

impl ChatTypeBound {
    /// binds a chat type with a sender and no target
    #[must_use]
    pub const fn new(registry_id: i32, sender_name: TextComponent) -> Self {
        Self {
            registry_id,
            sender_name,
            target_name: None,
        }
    }

    /// Set the target name
    pub fn with_target_name(mut self, target_name: TextComponent) -> Self {
        self.target_name = Some(target_name);
        self
    }
}

impl Default for ChatTypeBound {
    fn default() -> Self {
        Self::new(vanilla_chat_types::CHAT.id() as i32, TextComponent::new())
    }
}

/// A fixed width RSA signature for player msg
#[derive(WriteTo, Clone, Debug)]
pub struct MessageSignature(pub [u8; MessageSignature::BYTES]);

impl MessageSignature {
    /// the signature width in the chat protocol
    pub const BYTES: usize = 256;
}

#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_PLAYER_CHAT)]
pub struct CPlayerChat {
    pub global_index: i32,
    pub sender: Uuid,
    pub index: i32,
    pub message_signature: Option<MessageSignature>,
    pub message: String,
    pub timestamp: i64,
    pub salt: i64,
    pub previous_messages: Box<[PreviousMessage]>,
    pub unsigned_content: Option<TextComponent>,
    pub filter_type: FilterType,
    pub chat_type: ChatTypeBound,
}

impl CPlayerChat {
    #[must_use]
    #[expect(clippy::too_many_arguments)]
    pub const fn new(
        sender: Uuid,
        index: i32,
        message_signature: Option<MessageSignature>,
        message: String,
        timestamp: i64,
        salt: i64,
        previous_messages: Box<[PreviousMessage]>,
        unsigned_content: Option<TextComponent>,
        chat_type: ChatTypeBound,
    ) -> Self {
        Self {
            global_index: 0, // Assigned when sending the message
            sender,
            index,
            message_signature,
            message,
            timestamp,
            salt,
            previous_messages,
            unsigned_content,
            filter_type: FilterType::PassThrough, // Change only on Realms
            chat_type,
        }
    }
    /// uses recipientspecific fields while borrowing the shared msg
    #[must_use]
    pub fn for_recipient<'a>(
        &'a self,
        global_index: i32,
        previous_messages: &'a [PreviousMessage],
        chat_type: &'a ChatTypeBound,
    ) -> impl ClientPacket + 'a {
        RecipientChat {
            packet: self,
            global_index,
            previous_messages,
            chat_type,
        }
    }

    fn write_with(
        &self,
        writer: &mut impl Write,
        global_index: i32,
        previous_messages: &[PreviousMessage],
        chat_type: &ChatTypeBound,
    ) -> io::Result<()> {
        VarInt(global_index).write(writer)?;
        self.sender.write(writer)?;
        VarInt(self.index).write(writer)?;

        self.message_signature.write(writer)?;
        self.message.write_prefixed::<VarInt>(writer)?;
        self.timestamp.write(writer)?;
        self.salt.write(writer)?;

        VarInt(previous_messages.len() as i32).write(writer)?;
        for msg in previous_messages {
            VarInt(msg.id).write(writer)?;
            if msg.id == PreviousMessage::FULL_SIGNATURE_ID {
                let Some(signature) = &msg.signature else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Missing full previous message signature",
                    ));
                };

                writer.write_all(signature)?;
            }
        }

        self.unsigned_content.write(writer)?;
        match &self.filter_type {
            FilterType::PassThrough => VarInt(0).write(writer)?,
            FilterType::FullyFiltered => VarInt(1).write(writer)?,
            FilterType::PartiallyFiltered(mask) => {
                VarInt(2).write(writer)?;
                mask.write(writer)?;
            }
        }

        chat_type.write(writer)
    }
}

impl WriteTo for CPlayerChat {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        self.write_with(
            writer,
            self.global_index,
            &self.previous_messages,
            &self.chat_type,
        )
    }
}

#[derive(ClientPacket)]
#[packet_id(Play = C_PLAYER_CHAT)]
struct RecipientChat<'a> {
    packet: &'a CPlayerChat,
    global_index: i32,
    previous_messages: &'a [PreviousMessage],
    chat_type: &'a ChatTypeBound,
}

impl WriteTo for RecipientChat<'_> {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        self.packet.write_with(
            writer,
            self.global_index,
            self.previous_messages,
            self.chat_type,
        )
    }
}

#[derive(Clone, Debug)]
pub struct PreviousMessage {
    pub id: i32,
    pub signature: Option<Box<[u8]>>,
}

impl PreviousMessage {
    /// the wire ID followed by a full signature instead of a cache ref
    pub const FULL_SIGNATURE_ID: i32 = 0;
}

#[derive(Clone, Debug)]
pub enum FilterType {
    PassThrough,
    FullyFiltered,
    PartiallyFiltered(BitSet),
}

/// Clientbound packet for unsigned/disguised chat messages
/// This is sent when the server doesn't have a signed message
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_DISGUISED_CHAT)]
pub struct CDisguisedChat {
    pub message: TextComponent,
    pub chat_type: ChatTypeBound,
}

impl CDisguisedChat {
    pub fn new<T: TextResolutor>(
        message: &TextComponent,
        chat_type: ChatTypeBound,
        player: &T,
    ) -> Self {
        Self {
            message: message.resolve(player),
            chat_type,
        }
    }
}
