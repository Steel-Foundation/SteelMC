use crate::ItemStackTemplate;
use std::ops::Deref;
use std::sync::RwLock;
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
    pub location: RwLock<(f32, f32)>,
}

impl DisplayInfo {
    pub fn position(&self) -> (f32, f32) {
        self.location.read().unwrap().deref().clone()
    }
}

pub enum AdvancementType {
    Task,
    Challenge,
    Goal,
}