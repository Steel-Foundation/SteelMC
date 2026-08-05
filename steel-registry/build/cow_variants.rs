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
pub struct CowVariantJson {
    asset_id: Identifier,
    baby_asset_id: Identifier,
    #[serde(default)]
    model: String,
    spawn_conditions: Vec<SpawnConditionEntry>,
}

fn generate_cow_model_type(model: &str) -> TokenStream {
    match model {
        "cold" => quote! { CowModelType::Cold },
        "warm" => quote! { CowModelType::Warm },
        _ => quote! { CowModelType::Normal },
    }
}

pub(crate) fn build() -> TokenStream {
    let cow_variants: Vec<(String, CowVariantJson)> = read_variants_from_dir("cow_variant");

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::cow_variant::{
            CowVariant, CowVariantRegistry, CowModelType,
        };
        use steel_utils::Identifier;
        use std::borrow::Cow;
        use crate::shared_structs::{SpawnConditionEntry , BiomeCondition};
    });

    // Generate static cow variant definitions
    let mut register_stream = TokenStream::new();
    for (cow_variant_name, cow_variant) in &cow_variants {
        let cow_variant_ident =
            Ident::new(&cow_variant_name.to_shouty_snake_case(), Span::call_site());
        let cow_variant_name_str = cow_variant_name.clone();

        let key = quote! { Identifier::vanilla_static(#cow_variant_name_str) };
        let asset_id = generate_identifier(&cow_variant.asset_id);
        let baby_asset_id = generate_identifier(&cow_variant.baby_asset_id);
        let model = generate_cow_model_type(&cow_variant.model);

        let spawn_conditions: Vec<_> = cow_variant
            .spawn_conditions
            .iter()
            .map(generate_spawn_condition_entry)
            .collect();

        stream.extend(quote! {
            pub static #cow_variant_ident: CowVariant = CowVariant {
                key: #key,
                asset_id: #asset_id,
                baby_asset_id: #baby_asset_id,
                model: #model,
                spawn_conditions: &[#(#spawn_conditions),*],
            };
        });
        let cow_variant_ident =
            Ident::new(&cow_variant_name.to_shouty_snake_case(), Span::call_site());
        register_stream.extend(quote! {
            registry.register(&#cow_variant_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_cow_variants(registry: &mut CowVariantRegistry) {
            #register_stream
        }
    });

    stream
}
