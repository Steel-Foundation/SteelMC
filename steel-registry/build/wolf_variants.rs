use crate::generator_functions::{
    generate_identifier, generate_spawn_condition_entry, read_variants_from_dir,
};
use crate::shared_structs::SpawnConditionEntry;
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
pub struct WolfVariantJson {
    assets: WolfAssetInfo,
    pub baby_assets: WolfAssetInfo,
    spawn_conditions: Vec<SpawnConditionEntry>,
}

#[derive(Deserialize, Debug)]
pub struct WolfAssetInfo {
    wild: Identifier,
    tame: Identifier,
    angry: Identifier,
}

pub(crate) fn build() -> TokenStream {
    let wolf_variants: Vec<(String, WolfVariantJson)> = read_variants_from_dir("wolf_variant");

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::wolf_variant::{
            WolfVariant, WolfVariantRegistry, WolfAssetInfo, SpawnConditionEntry, BiomeCondition,
        };
        use steel_utils::Identifier;
        use std::borrow::Cow;
    });

    // Generate static wolf variant definitions
    let mut register_stream = TokenStream::new();
    for (wolf_variant_name, wolf_variant) in &wolf_variants {
        let wolf_variant_ident =
            Ident::new(&wolf_variant_name.to_shouty_snake_case(), Span::call_site());
        let wolf_variant_name_str = wolf_variant_name.clone();

        let key = quote! { Identifier::vanilla_static(#wolf_variant_name_str) };
        let wild = generate_identifier(&wolf_variant.assets.wild);
        let tame = generate_identifier(&wolf_variant.assets.tame);
        let angry = generate_identifier(&wolf_variant.assets.angry);
        let baby_wild = generate_identifier(&wolf_variant.baby_assets.wild);
        let baby_tame = generate_identifier(&wolf_variant.baby_assets.tame);
        let baby_angry = generate_identifier(&wolf_variant.baby_assets.angry);

        let spawn_conditions: Vec<_> = wolf_variant
            .spawn_conditions
            .iter()
            .map(generate_spawn_condition_entry)
            .collect();

        stream.extend(quote! {
            pub static #wolf_variant_ident: WolfVariant = WolfVariant {
                key: #key,
                assets: WolfAssetInfo {
                    wild: #wild,
                    tame: #tame,
                    angry: #angry,
                },
                baby_assets: WolfAssetInfo {
                    wild: #baby_wild,
                    tame: #baby_tame,
                    angry: #baby_angry,
                },
                spawn_conditions: &[#(#spawn_conditions),*],
            };
        });

        register_stream.extend(quote! {
            registry.register(&#wolf_variant_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_wolf_variants(registry: &mut WolfVariantRegistry) {
            #register_stream
        }
    });

    stream
}
