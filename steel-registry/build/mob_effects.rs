use crate::generator_functions::{read_json_asset, sort_contiguous_registry_entries};
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Deserialize)]
struct MobEffectEntry {
    id: u16,
    name: String,
    category: MobEffectCategoryEntry,
    color: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum MobEffectCategoryEntry {
    Beneficial,
    Harmful,
    Neutral,
}

impl MobEffectCategoryEntry {
    fn token(&self) -> TokenStream {
        match self {
            Self::Beneficial => quote! { MobEffectCategory::Beneficial },
            Self::Harmful => quote! { MobEffectCategory::Harmful },
            Self::Neutral => quote! { MobEffectCategory::Neutral },
        }
    }
}

pub(crate) fn build() -> TokenStream {
    const ASSET: &str = "build_assets/mob_effects.json";

    let mut effects: Vec<MobEffectEntry> = read_json_asset(ASSET);
    sort_contiguous_registry_entries(&mut effects, ASSET, |effect| usize::from(effect.id));

    let mut constants = TokenStream::new();
    let mut registrations = TokenStream::new();

    for effect in &effects {
        let ident = Ident::new(&effect.name.to_shouty_snake_case(), Span::call_site());
        let key = Literal::string(&effect.name);
        let category = effect.category.token();
        let color = effect.color;

        constants.extend(quote! {
            pub static #ident: &MobEffect = &MobEffect {
                key: Identifier::vanilla_static(#key),
                category: #category,
                color: #color,
            };
        });

        registrations.extend(quote! {
            registry.register(#ident);
        });
    }

    quote! {
        use crate::mob_effect::{MobEffect, MobEffectCategory, MobEffectRegistry};
        use steel_utils::Identifier;

        #constants

        pub fn register_mob_effects(registry: &mut MobEffectRegistry) {
            #registrations
        }
    }
}
