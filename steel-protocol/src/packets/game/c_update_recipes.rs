//! Clientbound recipe sync (`ClientboundUpdateRecipesPacket`).

use std::io::{Result, Write};

use steel_macros::ClientPacket;
use steel_registry::RegistryEntry;
use steel_registry::items::ItemRef;
use steel_registry::packets::play::C_UPDATE_RECIPES;
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::serial::WriteTo;

const SMITHING_BASE: Identifier = Identifier::vanilla_static("smithing_base");
const SMITHING_TEMPLATE: Identifier = Identifier::vanilla_static("smithing_template");
const SMITHING_ADDITION: Identifier = Identifier::vanilla_static("smithing_addition");
const FURNACE_INPUT: Identifier = Identifier::vanilla_static("furnace_input");
const BLAST_FURNACE_INPUT: Identifier = Identifier::vanilla_static("blast_furnace_input");
const SMOKER_INPUT: Identifier = Identifier::vanilla_static("smoker_input");
const CAMPFIRE_INPUT: Identifier = Identifier::vanilla_static("campfire_input");

/// Recipe property sets and selectable recipe lists sent after login.
#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_UPDATE_RECIPES)]
pub struct CUpdateRecipes {
    smithing_template: Vec<ItemRef>,
    smithing_base: Vec<ItemRef>,
    smithing_addition: Vec<ItemRef>,
    furnace_input: Vec<ItemRef>,
}

impl CUpdateRecipes {
    /// Syncs smithing and furnace property sets. Stonecutter recipes are empty
    /// until that menu is implemented on this branch.
    #[must_use]
    pub fn from_registry() -> Self {
        Self {
            smithing_template: steel_registry::REGISTRY.recipes.smithing_template_items(),
            smithing_base: steel_registry::REGISTRY.recipes.smithing_base_items(),
            smithing_addition: steel_registry::REGISTRY.recipes.smithing_addition_items(),
            furnace_input: steel_registry::REGISTRY.recipes.furnace_input_items(),
        }
    }
}

impl WriteTo for CUpdateRecipes {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(7).write(writer)?;
        write_property_set(writer, &SMITHING_TEMPLATE, &self.smithing_template)?;
        write_property_set(writer, &SMITHING_BASE, &self.smithing_base)?;
        write_property_set(writer, &SMITHING_ADDITION, &self.smithing_addition)?;
        write_property_set(writer, &FURNACE_INPUT, &self.furnace_input)?;
        write_property_set(writer, &BLAST_FURNACE_INPUT, &[])?;
        write_property_set(writer, &SMOKER_INPUT, &[])?;
        write_property_set(writer, &CAMPFIRE_INPUT, &[])?;
        VarInt(0).write(writer)?;
        Ok(())
    }
}

fn write_property_set(writer: &mut impl Write, key: &Identifier, items: &[ItemRef]) -> Result<()> {
    key.write(writer)?;
    let count = i32::try_from(items.len())
        .map_err(|_| std::io::Error::other("recipe property set exceeds protocol range"))?;
    VarInt(count).write(writer)?;
    for item in items {
        let id = item.try_id().ok_or_else(|| {
            std::io::Error::other(format!(
                "unregistered item in recipe property set: {}",
                item.key
            ))
        })?;
        let id = i32::try_from(id)
            .map_err(|_| std::io::Error::other(format!("item id out of protocol range: {id}")))?;
        VarInt(id).write(writer)?;
    }
    Ok(())
}
