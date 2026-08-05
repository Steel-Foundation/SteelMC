use steel_macros::{ReadFrom, ServerPacket};
use uuid::Uuid;

#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SHello {
    #[read(as = Prefixed(VarInt), bound = 16)]
    pub name: String,
    pub profile_id: Uuid,
}
