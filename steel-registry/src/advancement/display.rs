use crate::ItemStackTemplate;
use crate::advancement::registry::AdvancementRef;
use serde::{Deserialize, Serialize};
use std::io::Write;
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::serial::WriteTo;
use steel_utils::translations::{
    CHAT_TYPE_ADVANCEMENT_CHALLENGE, CHAT_TYPE_ADVANCEMENT_GOAL, CHAT_TYPE_ADVANCEMENT_TASK,
};
use text_components::TextComponent;
use text_components::format::Color;
use text_components::translation::Translation;

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
}

impl DisplayInfo {
    #[must_use]
    pub const fn has_background(&self) -> bool {
        self.background.is_some()
    }
}

impl WriteTo for DisplayInfo {
    fn write(&self, writer: &mut impl Write) -> std::io::Result<()> {
        self.title.write(writer)?;
        self.description.write(writer)?;
        self.icon.write(writer)?;
        VarInt(self.frame_type as i32).write(writer)?;
        let flags = i32::from(self.has_background())
            | i32::from(self.show_toast) << 1
            | i32::from(self.hidden) << 2;
        flags.write(writer)?;
        if let Some(background) = &self.background {
            background.write(writer)?;
        }
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

impl AdvancementType {
    #[must_use]
    fn get_translated_text(&self) -> &Translation<2usize> {
        match self {
            Self::Task => &CHAT_TYPE_ADVANCEMENT_TASK,
            Self::Challenge => &CHAT_TYPE_ADVANCEMENT_CHALLENGE,
            Self::Goal => &CHAT_TYPE_ADVANCEMENT_GOAL,
        }
    }

    #[must_use]
    pub const fn color(&self) -> Color {
        match self {
            Self::Task => Color::Green,
            Self::Challenge => Color::DarkPurple,
            Self::Goal => Color::Green,
        }
    }

    #[must_use]
    pub fn create_announcement(
        &self,
        advancement: AdvancementRef,
        player_name: TextComponent,
    ) -> TextComponent {
        TextComponent::translated(
            self.get_translated_text()
                .message([player_name, advancement.name()]),
        )
    }
}

impl WriteTo for AdvancementType {
    fn write(&self, writer: &mut impl Write) -> std::io::Result<()> {
        VarInt(*self as i32).write(writer)
    }
}
