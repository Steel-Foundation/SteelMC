use std::io::{Result, Write};

use steel_macros::ClientPacket;
use steel_registry::{REGISTRY, menu_type::MenuTypeRef, packets::play::C_OPEN_SCREEN};
use steel_utils::{codec::VarInt, serial::WriteTo};
use text_components::{TextComponent, resolving::TextResolutor};

#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_OPEN_SCREEN)]
pub struct COpenScreen {
    pub container_id: i32,
    pub menu_type: MenuTypeRef,
    pub title: TextComponent,
}

impl COpenScreen {
    pub fn new<T: TextResolutor>(
        container_id: i32,
        menu_type: MenuTypeRef,
        title: &TextComponent,
        player: &T,
    ) -> Self {
        Self {
            container_id,
            menu_type,
            title: title.resolve(player),
        }
    }
}

impl WriteTo for COpenScreen {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.container_id).write(writer)?;
        let menu_type_id = *REGISTRY.menu_types.get_id(self.menu_type);
        VarInt(menu_type_id as i32).write(writer)?;
        self.title.write(writer)?;
        Ok(())
    }
}
