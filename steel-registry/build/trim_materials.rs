use crate::generator_functions::{generate_identifier, generate_text_component, read_json_asset};
use crate::shared_structs::TextComponentJson;
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct TrimMaterialJson {
    palette_id: Identifier,
    description: TextComponentJson,
}

pub(crate) fn build() -> TokenStream {
    // TrimMaterials.bootstrap defines registry insertion order in Vanilla.
    const VANILLA_ORDER: &[&str] = &[
        "quartz",
        "iron",
        "netherite",
        "redstone",
        "copper",
        "gold",
        "emerald",
        "diamond",
        "lapis",
        "amethyst",
        "resin",
    ];

    let trim_materials = VANILLA_ORDER.iter().map(|name| {
        let path = format!(
            "../steel-utils/build_assets/builtin_datapacks/minecraft/trim_material/{name}.json"
        );
        (*name, read_json_asset::<TrimMaterialJson>(&path))
    });

    let mut definitions = TokenStream::new();
    let mut registrations = TokenStream::new();
    for (name, material) in trim_materials {
        let ident = Ident::new(&name.to_shouty_snake_case(), Span::call_site());
        let key = quote! { Identifier::vanilla_static(#name) };
        let palette_id = generate_identifier(&material.palette_id);
        let description = generate_text_component(&material.description);

        definitions.extend(quote! {
            pub static #ident: LazyLock<TrimMaterial> = LazyLock::new(|| {
                TrimMaterial::new(
                    #key,
                    TrimMaterialValue::new(#palette_id, #description),
                )
            });
        });
        registrations.extend(quote! {
            registry.register(&*#ident);
        });
    }

    quote! {
        use crate::trim_material::{TrimMaterial, TrimMaterialRegistry, TrimMaterialValue};
        use steel_utils::Identifier;
        use std::{borrow::Cow, sync::LazyLock};
        use text_components::{TextComponent, translation::TranslatedMessage};

        #definitions

        pub fn register_trim_materials(registry: &mut TrimMaterialRegistry) {
            #registrations
        }
    }
}
