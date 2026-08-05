use crate::generator_functions::{generate_identifier, generate_text_component, read_json_asset};
use crate::shared_structs::TextComponentJson;
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use rustc_hash::FxHashMap;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct TrimMaterialJson {
    asset_name: String,
    description: TextComponentJson,
    #[serde(default)]
    override_armor_assets: FxHashMap<Identifier, String>,
}

fn generate_overrides(map: &FxHashMap<Identifier, String>) -> TokenStream {
    if map.is_empty() {
        return quote! { FxHashMap::default() };
    }
    let mut entries = map.iter().collect::<Vec<_>>();
    entries.sort_by_key(|(equipment_asset, _)| equipment_asset.to_string());
    let entries = entries.into_iter().map(|(equipment_asset, suffix)| {
        validate_asset_suffix(suffix);
        let equipment_asset = generate_identifier(equipment_asset);
        quote! {
            (
                #equipment_asset,
                MaterialAssetInfo::from_validated_suffix(#suffix.to_owned()),
            )
        }
    });
    quote! { FxHashMap::from_iter([#(#entries),*]) }
}

fn validate_asset_suffix(suffix: &str) {
    assert!(
        Identifier::validate_path(suffix),
        "invalid trim material asset suffix in extracted data: {suffix:?}"
    );
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
        validate_asset_suffix(&material.asset_name);
        let ident = Ident::new(&name.to_shouty_snake_case(), Span::call_site());
        let key = quote! { Identifier::vanilla_static(#name) };
        let asset_name = material.asset_name;
        let overrides = generate_overrides(&material.override_armor_assets);
        let description = generate_text_component(&material.description);

        definitions.extend(quote! {
            pub static #ident: LazyLock<TrimMaterial> = LazyLock::new(|| {
                TrimMaterial::new(
                    #key,
                    TrimMaterialValue::new(
                        MaterialAssetGroup::new(
                            MaterialAssetInfo::from_validated_suffix(#asset_name.to_owned()),
                            #overrides,
                        ),
                        #description,
                    ),
                )
            });
        });
        registrations.extend(quote! {
            registry.register(&*#ident);
        });
    }

    quote! {
        use crate::trim_material::{
            MaterialAssetGroup, MaterialAssetInfo, TrimMaterial, TrimMaterialRegistry,
            TrimMaterialValue,
        };
        use rustc_hash::FxHashMap;
        use steel_utils::Identifier;
        use std::{borrow::Cow, sync::LazyLock};
        use text_components::{TextComponent, translation::TranslatedMessage};

        #definitions

        pub fn register_trim_materials(registry: &mut TrimMaterialRegistry) {
            #registrations
        }
    }
}
