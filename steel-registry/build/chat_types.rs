use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct ChatTypeJson {
    chat: ChatTypeDecoration,
    narration: ChatTypeDecoration,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ChatTypeDecoration {
    translation_key: String,
    parameters: Vec<String>,

    #[serde(default)]
    style: Option<ChatStyle>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ChatStyle {
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    bold: Option<bool>,
    #[serde(default)]
    italic: Option<bool>,
    #[serde(default)]
    underlined: Option<bool>,
    #[serde(default)]
    strikethrough: Option<bool>,
    #[serde(default)]
    obfuscated: Option<bool>,
}

fn generate_option<T, F>(opt: &Option<T>, f: F) -> TokenStream
where
    F: FnOnce(&T) -> TokenStream,
{
    match opt {
        Some(val) => {
            let inner = f(val);
            quote! { Some(#inner) }
        }
        None => quote! { None },
    }
}

fn generate_chat_style(style: &ChatStyle) -> TokenStream {
    let color = generate_option(&style.color, |s| {
        let val = s.as_str();
        quote! { #val }
    });
    let bold = generate_option(&style.bold, |&v| quote! { #v });
    let italic = generate_option(&style.italic, |&v| quote! { #v });
    let underlined = generate_option(&style.underlined, |&v| quote! { #v });
    let strikethrough = generate_option(&style.strikethrough, |&v| quote! { #v });
    let obfuscated = generate_option(&style.obfuscated, |&v| quote! { #v });

    quote! {
        ChatStyle {
            color: #color,
            bold: #bold,
            italic: #italic,
            underlined: #underlined,
            strikethrough: #strikethrough,
            obfuscated: #obfuscated,
        }
    }
}

fn generate_chat_type_decoration(decoration: &ChatTypeDecoration) -> TokenStream {
    let translation_key = &decoration.translation_key;
    let parameters: Vec<_> = decoration
        .parameters
        .iter()
        .map(|p| quote! { #p })
        .collect();
    let style = generate_option(&decoration.style, generate_chat_style);

    quote! {
        ChatTypeDecoration {
            translation_key: #translation_key,
            parameters: &[#(#parameters),*],
            style: #style,
        }
    }
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/chat_type/"
    );

    let chat_type_dir = "build_assets/builtin_datapacks/minecraft/data/minecraft/chat_type";
    let mut chat_types = Vec::new();

    // Read all chat type JSON files
    for entry in fs::read_dir(chat_type_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let chat_type_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let chat_type: ChatTypeJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", chat_type_name, e));

            chat_types.push((chat_type_name, chat_type));
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::chat_type::{
            ChatType, ChatTypeDecoration, ChatTypeRegistry, ChatStyle,
        };
        use steel_utils::Identifier;
    });

    // Generate static chat type definitions
    for (chat_type_name, chat_type) in &chat_types {
        let chat_type_ident = Ident::new(&chat_type_name.to_shouty_snake_case(), Span::call_site());
        let chat_type_name_str = chat_type_name.clone();

        let key = quote! { Identifier::vanilla_static(#chat_type_name_str) };
        let chat = generate_chat_type_decoration(&chat_type.chat);
        let narration = generate_chat_type_decoration(&chat_type.narration);

        stream.extend(quote! {
            pub static #chat_type_ident: &ChatType = &ChatType {
                key: #key,
                chat: #chat,
                narration: #narration,
            };
        });
    }

    // Generate registration function
    let mut register_stream = TokenStream::new();
    for (chat_type_name, _) in &chat_types {
        let chat_type_ident = Ident::new(&chat_type_name.to_shouty_snake_case(), Span::call_site());
        register_stream.extend(quote! {
            registry.register(#chat_type_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_chat_types(registry: &mut ChatTypeRegistry) {
            #register_stream
        }
    });

    stream
}
