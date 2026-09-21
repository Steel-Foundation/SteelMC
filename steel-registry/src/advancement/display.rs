use crate::ItemStackTemplate;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::ops::Deref;
use steel_utils::Identifier;
use steel_utils::locks::SyncRwLock;
use steel_utils::serial::WriteTo;
use text_components::TextComponent;

#[derive(Debug, Default)]
pub struct DisplayInfo {
    pub title: TextComponent,
    pub description: TextComponent,
    pub icon: ItemStackTemplate,
    pub background: Option<Identifier>,
    pub frame_type: AdvancementType,
    pub show_toast: bool,
    pub announce_chat: bool,
    pub hidden: bool,
    pub location: SyncRwLock<(f32, f32)>,
}

impl DisplayInfo {
    pub fn position(&self) -> (f32, f32) {
        *self.location.read().deref()
    }

    pub fn has_background(&self) -> bool {
        self.background.is_some()
    }
}

impl WriteTo for DisplayInfo {
    fn write(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.title.write(writer)?;
        self.description.write(writer)?;
        self.icon.write(writer)?;
        self.frame_type.write(writer)?;
        let flags = (self.has_background() as i32)
            | ((self.show_toast as i32) << 1)
            | ((self.hidden as i32) << 2);
        flags.write(writer)?;
        self.background.write(writer)?;
        self.location.read().write(writer)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
pub enum AdvancementType {
    #[default]
    Task,
    Challenge,
    Goal,
}

impl WriteTo for AdvancementType {
    fn write(&self, writer: &mut impl Write) -> std::io::Result<()> {
        (*self as i32).write(writer)
    }
}
