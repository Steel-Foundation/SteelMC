use crate::ItemStackTemplate;
use steel_utils::Identifier;
use text_components::TextComponent;

pub struct DisplayInfo {
    pub title: TextComponent,
    pub description: TextComponent,
    pub icon: ItemStackTemplate,
    pub background: Option<Identifier>,
    pub frame_type: AdvancementType,
    pub show_toast: bool,
    pub announce_chat: bool,
    pub hidden: bool,
    pub x: f32,
    pub y: f32,
}

pub enum AdvancementType {
    Task,
    Challenge,
    Goal,
}