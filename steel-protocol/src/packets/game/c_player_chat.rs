use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_PLAYER_CHAT;
use steel_utils::{
    codec::{BitSet, VarInt},
    serial::PrefixedWrite,
};
use text_components::TextComponent;
use uuid::Uuid;

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

#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_PLAYER_CHAT)]
pub struct CPlayerChat {
    pub global_index: i32,
    pub sender: Uuid,
    pub index: i32,
    pub message_signature: Option<Box<[u8]>>,
    pub message: String,
    pub timestamp: i64,
    pub salt: i64,
    pub previous_messages: Box<[PreviousMessage]>,
    pub unsigned_content: Option<TextComponent>,
    pub filter_type: FilterType,
    pub chat_type: ChatTypeBound,
}

impl CPlayerChat {
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        global_index: i32,
        sender: Uuid,
        index: i32,
        message_signature: Option<Box<[u8]>>,
        message: String,
        timestamp: i64,
        salt: i64,
        previous_messages: Box<[PreviousMessage]>,
        unsigned_content: Option<TextComponent>,
        filter_type: FilterType,
        chat_type: ChatTypeBound,
    ) -> Self {
        Self {
            global_index,
            sender,
            index,
            message_signature,
            message,
            timestamp,
            salt,
            previous_messages,
            unsigned_content,
            filter_type,
            chat_type,
        }
    }
}

impl steel_utils::serial::WriteTo for CPlayerChat {
    fn write(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        VarInt(self.global_index).write(writer)?;
        self.sender.write(writer)?;
        VarInt(self.index).write(writer)?;

        match &self.message_signature {
            Some(sig) => {
                true.write(writer)?;
                writer.write_all(sig)?;
            }
            None => false.write(writer)?,
        }

        self.message.write_prefixed::<VarInt>(writer)?;
        self.timestamp.write(writer)?;
        self.salt.write(writer)?;

        VarInt(self.previous_messages.len() as i32).write(writer)?;
        for msg in self.previous_messages.iter() {
            // Write ID. In Minecraft's packed format:
            // - If id is 0: write 0 (VarInt(0)), then write full signature (256 bytes)
            // - If id is N > 0: write N (VarInt(N)), no signature bytes
            // Our id field already contains the correct value (0 for full, cache_index+1 for referenced)
            VarInt(msg.id).write(writer)?;
            // Only write signature if id is 0 (full signature)
            if msg.id == 0 {
                if let Some(sig) = &msg.signature {
                    writer.write_all(sig)?;
                } else {
                    // This should never happen - id=0 means full signature must be present
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "PreviousMessage with id=0 must have signature",
                    ));
                }
            }
        }

        match &self.unsigned_content {
            Some(content) => {
                true.write(writer)?;
                content.write(writer)?;
            }
            None => false.write(writer)?,
        }

        VarInt(match self.filter_type {
            FilterType::PassThrough => 0,
            FilterType::FullyFiltered => 1,
            FilterType::PartiallyFiltered(_) => 2,
        })
        .write(writer)?;

        self.chat_type.write(writer)?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct PreviousMessage {
    pub id: i32,
    pub signature: Option<Box<[u8]>>,
}

#[derive(Clone, Debug)]
pub enum FilterType {
    PassThrough,
    FullyFiltered,
    PartiallyFiltered(BitSet),
}
