use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_MERCHANT_OFFERS;

use super::merchant::MerchantOfferPacket;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_MERCHANT_OFFERS)]
pub struct CMerchantOffers {
    #[write(as = VarInt)]
    pub container_id: i32,
    pub offers: Vec<MerchantOfferPacket>,
    #[write(as = VarInt)]
    pub villager_level: i32,
    #[write(as = VarInt)]
    pub villager_xp: i32,
    pub show_progress: bool,
    pub can_restock: bool,
}
