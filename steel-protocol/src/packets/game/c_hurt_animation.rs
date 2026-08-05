use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_HURT_ANIMATION;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_HURT_ANIMATION)]
pub struct CHurtAnimation {
    /// The ID of the entity taking damage
    #[write(as = VarInt)]
    pub entity_id: i32,
    /// The direction the damage is coming from in relation to the entity
    pub yaw: f32,
}
