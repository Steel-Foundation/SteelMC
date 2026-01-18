use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
pub struct InstrumentJson {
    sound_event: Identifier,
    use_duration: f32,
    range: f32,
    description: TextComponentJson,
}

#[derive(Deserialize, Debug)]
pub struct TextComponentJson {
    translate: String,
}

fn generate_identifier(resource: &Identifier) -> TokenStream {
    let namespace = resource.namespace.as_ref();
    let path = resource.path.as_ref();
    quote! { Identifier { namespace: Cow::Borrowed(#namespace), path: Cow::Borrowed(#path) } }
}

fn generate_text_component(component: &TextComponentJson) -> TokenStream {
    let translate = component.translate.as_str();
    quote! {
        TextComponent::const_translate(#translate)
    }
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/instrument/"
    );

    let instrument_dir = "build_assets/builtin_datapacks/minecraft/data/minecraft/instrument";
    let mut instruments = Vec::new();

    // Read all instrument JSON files
    for entry in fs::read_dir(instrument_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let instrument_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let instrument: InstrumentJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", instrument_name, e));

            instruments.push((instrument_name, instrument));
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::instrument::{
            Instrument, InstrumentRegistry,
        };
        use steel_utils::Identifier;
        use steel_utils::text::TextComponent;
        use std::borrow::Cow;
    });

    // Generate static instrument definitions
    for (instrument_name, instrument) in &instruments {
        let instrument_ident =
            Ident::new(&instrument_name.to_shouty_snake_case(), Span::call_site());
        let instrument_name_str = instrument_name.clone();

        let key = quote! { Identifier::vanilla_static(#instrument_name_str) };
        let sound_event = generate_identifier(&instrument.sound_event);
        let use_duration = instrument.use_duration;
        let range = instrument.range;
        let description = generate_text_component(&instrument.description);

        stream.extend(quote! {
            pub static #instrument_ident: &Instrument = &Instrument {
                key: #key,
                sound_event: #sound_event,
                use_duration: #use_duration,
                range: #range,
                description: #description,
            };
        });
    }

    // Generate registration function
    let mut register_stream = TokenStream::new();
    for (instrument_name, _) in &instruments {
        let instrument_ident =
            Ident::new(&instrument_name.to_shouty_snake_case(), Span::call_site());
        register_stream.extend(quote! {
            registry.register(#instrument_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_instruments(registry: &mut InstrumentRegistry) {
            #register_stream
        }
    });

    stream
}
