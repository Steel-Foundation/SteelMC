use crate::generator_functions::{
    generate_identifier, generate_item_stack_template, generate_option, generate_text_component,
};
use crate::shared_structs::{ItemStackTemplateJson, TextComponentJson};
use proc_macro2::{Ident, Span, TokenStream};
use quote::{ToTokens, quote};
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize)]
pub(crate) struct AdvancementDisplayJson {
    title: TextComponentJson,
    description: TextComponentJson,
    #[serde(rename = "icon")]
    item_icon: ItemStackTemplateJson,
    #[serde(default, rename = "frame")]
    frame_type: FrameTypeJson,
    #[serde(default, rename = "background")]
    background_texture: Option<Identifier>,
    #[serde(default = "default_true")]
    show_toast: bool,
    #[serde(default)]
    hidden: bool,
    #[serde(default = "default_true")]
    announce_to_chat: bool,
}

#[derive(Deserialize, Default, Debug, Clone)]
#[serde(rename_all = "lowercase")]
enum FrameTypeJson {
    #[default]
    Task,
    Challenge,
    Goal,
}

impl ToTokens for FrameTypeJson {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let name = match self {
            FrameTypeJson::Task => "Task",
            FrameTypeJson::Challenge => "Challenge",
            FrameTypeJson::Goal => "Goal",
        };
        let ident = Ident::new(name, Span::call_site());
        tokens.extend(quote! {
            AdvancementType::#ident
        });
    }
}

const fn default_true() -> bool {
    true
}

pub(crate) fn parse_display(display: &AdvancementDisplayJson) -> TokenStream {
    let title = generate_text_component(&display.title);
    let description = generate_text_component(&display.description);
    let frame_type = &display.frame_type;
    let background = generate_option(&display.background_texture, generate_identifier);
    let icon = generate_item_stack_template(&display.item_icon);
    let show_toast = &display.show_toast;
    let announce_to_chat = &display.announce_to_chat;
    let hidden = &display.hidden;
    quote! {
        DisplayInfo {
            title: #title,
            description: #description,
            icon : #icon,
            background: #background,
            frame_type: #frame_type,
            show_toast: #show_toast,
            announce_chat: #announce_to_chat,
            hidden: #hidden,
            location: SyncRwLock::new((0f32,0f32)),
        }
    }
}
