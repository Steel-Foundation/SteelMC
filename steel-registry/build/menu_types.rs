use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/menutypes.json");

    let menu_types_file = "build_assets/menutypes.json";
    let content = fs::read_to_string(menu_types_file).unwrap();
    let menu_types: Vec<String> = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse menutypes.json: {}", e));

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::menu_type::{
            MenuType, MenuTypeRegistry,
        };
        use steel_utils::Identifier;
    });

    // Generate static menu type definitions
    let mut register_stream = TokenStream::new();
    for menu_type_name in &menu_types {
        let menu_type_ident = Ident::new(&menu_type_name.to_shouty_snake_case(), Span::call_site());
        let menu_type_name_str = menu_type_name.clone();

        let key = quote! { Identifier::vanilla_static(#menu_type_name_str) };

        stream.extend(quote! {
            pub static #menu_type_ident: &MenuType = &MenuType {
                key: #key,
            };
        });

        register_stream.extend(quote! {
            registry.register(#menu_type_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_menu_types(registry: &mut MenuTypeRegistry) {
            #register_stream
        }
    });

    stream
}
