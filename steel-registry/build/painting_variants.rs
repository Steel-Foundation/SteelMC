use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
pub struct PaintingVariantJson {
    width: i32,
    height: i32,
    asset_id: Identifier,
    #[serde(default)]
    title: Option<TextComponentJson>,
    #[serde(default)]
    author: Option<TextComponentJson>,
}

#[derive(Deserialize, Debug)]
pub struct TextComponentJson {
    translate: String,
    #[serde(default)]
    color: Option<String>,
}

fn generate_identifier(resource: &Identifier) -> TokenStream {
    let namespace = resource.namespace.as_ref();
    let path = resource.path.as_ref();
    quote! { Identifier { namespace: Cow::Borrowed(#namespace), path: Cow::Borrowed(#path) } }
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

fn parse_color(color_str: &str) -> TokenStream {
    // Parse named colors at build time
    match color_str {
        "black" => quote! { Color::Named(NamedColor::Black) },
        "dark_blue" => quote! { Color::Named(NamedColor::DarkBlue) },
        "dark_green" => quote! { Color::Named(NamedColor::DarkGreen) },
        "dark_aqua" => quote! { Color::Named(NamedColor::DarkAqua) },
        "dark_red" => quote! { Color::Named(NamedColor::DarkRed) },
        "dark_purple" => quote! { Color::Named(NamedColor::DarkPurple) },
        "gold" => quote! { Color::Named(NamedColor::Gold) },
        "gray" => quote! { Color::Named(NamedColor::Gray) },
        "dark_gray" => quote! { Color::Named(NamedColor::DarkGray) },
        "blue" => quote! { Color::Named(NamedColor::Blue) },
        "green" => quote! { Color::Named(NamedColor::Green) },
        "aqua" => quote! { Color::Named(NamedColor::Aqua) },
        "red" => quote! { Color::Named(NamedColor::Red) },
        "light_purple" => quote! { Color::Named(NamedColor::LightPurple) },
        "yellow" => quote! { Color::Named(NamedColor::Yellow) },
        "white" => quote! { Color::Named(NamedColor::White) },
        _ => panic!("Unknown color: {}", color_str),
    }
}

fn generate_text_component(component: &TextComponentJson) -> TokenStream {
    let translate = component.translate.as_str();

    if let Some(color_str) = &component.color {
        let color = parse_color(color_str.as_str());
        // Generate code that creates a TextComponent with color
        quote! {
            TextComponent::const_translate_with_color(#translate, #color)
        }
    } else {
        quote! {
            TextComponent::const_translate(#translate)
        }
    }
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/painting_variant/"
    );

    let painting_variant_dir =
        "build_assets/builtin_datapacks/minecraft/data/minecraft/painting_variant";
    let mut painting_variants = Vec::new();

    // Read all painting variant JSON files
    for entry in fs::read_dir(painting_variant_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let painting_variant_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let painting_variant: PaintingVariantJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", painting_variant_name, e));

            painting_variants.push((painting_variant_name, painting_variant));
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::painting_variant::{
            PaintingVariant, PaintingVariantRegistry,
        };
        use steel_utils::Identifier;
        use steel_utils::text::TextComponent;
        use steel_utils::text::color::{Color, NamedColor};
        use std::borrow::Cow;
    });

    // Generate static painting variant definitions
    for (painting_variant_name, painting_variant) in &painting_variants {
        let painting_variant_ident = Ident::new(
            &painting_variant_name.to_shouty_snake_case(),
            Span::call_site(),
        );
        let painting_variant_name_str = painting_variant_name.clone();

        let key = quote! { Identifier::vanilla_static(#painting_variant_name_str) };
        let asset_id = generate_identifier(&painting_variant.asset_id);
        let width = painting_variant.width;
        let height = painting_variant.height;
        let title = generate_option(&painting_variant.title, generate_text_component);
        let author = generate_option(&painting_variant.author, generate_text_component);

        stream.extend(quote! {
            pub static #painting_variant_ident: &PaintingVariant = &PaintingVariant {
                key: #key,
                width: #width,
                height: #height,
                asset_id: #asset_id,
                title: #title,
                author: #author,
            };
        });
    }

    // Generate registration function
    let mut register_stream = TokenStream::new();
    for (painting_variant_name, _) in &painting_variants {
        let painting_variant_ident = Ident::new(
            &painting_variant_name.to_shouty_snake_case(),
            Span::call_site(),
        );
        register_stream.extend(quote! {
            registry.register(#painting_variant_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_painting_variants(registry: &mut PaintingVariantRegistry) {
            #register_stream
        }
    });

    stream
}
