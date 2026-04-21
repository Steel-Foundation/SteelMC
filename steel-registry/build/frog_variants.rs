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
pub struct FrogVariantJson {
    asset_id: Identifier,
    spawn_conditions: Vec<SpawnConditionEntry>,
}

pub(crate) fn build() -> TokenStream {
    let frog_variants: Vec<(String, FrogVariantJson)> = read_variants_from_dir("frog_variant");

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::frog_variant::{
            FrogVariant, FrogVariantRegistry,
        };
        use steel_utils::Identifier;
        use std::borrow::Cow;
        use crate::shared_structs::{SpawnConditionEntry , BiomeCondition};
    });

    // Generate static frog variant definitions
    let mut register_stream = TokenStream::new();
    for (frog_variant_name, frog_variant) in &frog_variants {
        let frog_variant_ident =
            Ident::new(&frog_variant_name.to_shouty_snake_case(), Span::call_site());
        let frog_variant_name_str = frog_variant_name.clone();

        let key = quote! { Identifier::vanilla_static(#frog_variant_name_str) };
        let asset_id = generate_identifier(&frog_variant.asset_id);

        let spawn_conditions: Vec<_> = frog_variant
            .spawn_conditions
            .iter()
            .map(generate_spawn_condition_entry)
            .collect();

        stream.extend(quote! {
            pub static #frog_variant_ident: FrogVariant = FrogVariant {
                key: #key,
                asset_id: #asset_id,
                spawn_conditions: &[#(#spawn_conditions),*],
            };
        });

        register_stream.extend(quote! {
            registry.register(&#frog_variant_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_frog_variants(registry: &mut FrogVariantRegistry) {
            #register_stream
        }
    });

    stream
}
