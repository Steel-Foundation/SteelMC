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
pub struct PigVariantJson {
    asset_id: Identifier,
    baby_asset_id: Identifier,
    #[serde(default)]
    model: String,
    spawn_conditions: Vec<SpawnConditionEntry>,
}

fn generate_pig_model_type(model: &str) -> TokenStream {
    match model {
        "cold" => quote! { PigModelType::Cold },
        _ => quote! { PigModelType::Normal },
    }
}

pub(crate) fn build() -> TokenStream {
    let pig_variants: Vec<(String, PigVariantJson)> = read_variants_from_dir("pig_variant");

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::pig_variant::{
            PigVariant, PigVariantRegistry, PigModelType, SpawnConditionEntry, BiomeCondition,
        };
        use steel_utils::Identifier;
        use std::borrow::Cow;
    });

    // Generate static pig variant definitions
    let mut register_stream = TokenStream::new();
    for (pig_variant_name, pig_variant) in &pig_variants {
        let pig_variant_ident =
            Ident::new(&pig_variant_name.to_shouty_snake_case(), Span::call_site());
        let pig_variant_name_str = pig_variant_name.clone();

        let key = quote! { Identifier::vanilla_static(#pig_variant_name_str) };
        let asset_id = generate_identifier(&pig_variant.asset_id);
        let baby_asset_id = generate_identifier(&pig_variant.baby_asset_id);
        let model = generate_pig_model_type(&pig_variant.model);

        let spawn_conditions: Vec<_> = pig_variant
            .spawn_conditions
            .iter()
            .map(generate_spawn_condition_entry)
            .collect();

        stream.extend(quote! {
            pub static #pig_variant_ident: PigVariant = PigVariant {
                key: #key,
                asset_id: #asset_id,
                baby_asset_id: #baby_asset_id,
                model: #model,
                spawn_conditions: &[#(#spawn_conditions),*],
            };
        });

        register_stream.extend(quote! {
            registry.register(&#pig_variant_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_pig_variants(registry: &mut PigVariantRegistry) {
            #register_stream
        }
    });

    stream
}
