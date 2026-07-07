//! Clientbound merchant offers

use std::io::{Result, Write};

use steel_macros::ClientPacket;
use steel_registry::RegistryEntry;
use steel_registry::item_stack::ItemStack;
use steel_registry::packets::play::C_MERCHANT_OFFERS;
use steel_utils::codec::VarInt;
use steel_utils::serial::WriteTo;

#[derive(Clone)]
pub struct MerchantOfferData {
    pub cost_a: ItemStack,
    pub result: ItemStack,
    pub cost_b: Option<ItemStack>,
    pub out_of_stock: bool,
    pub uses: i32,
    pub max_uses: i32,
    pub xp: i32,
    pub special_price: i32,
    pub price_multiplier: f32,
    pub demand: i32,
}

#[derive(ClientPacket, Clone)]
#[packet_id(Play = C_MERCHANT_OFFERS)]
pub struct CMerchantOffers {
    pub container_id: i32,
    pub offers: Vec<MerchantOfferData>,
    pub villager_level: i32,
    pub villager_xp: i32,
    pub show_progress: bool,
    pub can_restock: bool,
}

impl WriteTo for CMerchantOffers {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.container_id).write(writer)?;
        VarInt(self.offers.len() as i32).write(writer)?;
        for offer in &self.offers {
            write_offer(offer, writer)?;
        }
        VarInt(self.villager_level).write(writer)?;
        VarInt(self.villager_xp).write(writer)?;
        self.show_progress.write(writer)?;
        self.can_restock.write(writer)?;
        Ok(())
    }
}

fn write_offer(offer: &MerchantOfferData, writer: &mut impl Write) -> Result<()> {
    write_item_cost(&offer.cost_a, writer)?;
    offer.result.write(writer)?;
    write_optional_item_cost(offer.cost_b.as_ref(), writer)?;
    offer.out_of_stock.write(writer)?;
    offer.uses.write(writer)?;
    offer.max_uses.write(writer)?;
    offer.xp.write(writer)?;
    offer.special_price.write(writer)?;
    offer.price_multiplier.write(writer)?;
    offer.demand.write(writer)?;
    Ok(())
}

fn write_item_cost(stack: &ItemStack, writer: &mut impl Write) -> Result<()> {
    VarInt(stack.item().id() as i32).write(writer)?;
    VarInt(stack.count()).write(writer)?;
    VarInt(0).write(writer)?;
    Ok(())
}

fn write_optional_item_cost(cost: Option<&ItemStack>, writer: &mut impl Write) -> Result<()> {
    match cost {
        Some(stack) => {
            true.write(writer)?;
            write_item_cost(stack, writer)?;
        }
        None => false.write(writer)?,
    }
    Ok(())
}
