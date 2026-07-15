use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PotionJson {
    id: usize,
    name: String,
    effects: Vec<PotionEffectJson>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PotionEffectJson {
    effect: String,
    duration: i32,
    amplifier: i32,
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/potions.json");
    let potions: Vec<PotionJson> = serde_json::from_str(
        &fs::read_to_string("build_assets/potions.json")
            .expect("missing extracted potion registry"),
    )
    .expect("invalid extracted potion registry");

    let mut definitions = TokenStream::new();
    let mut registrations = TokenStream::new();
    for (expected_id, potion) in potions.iter().enumerate() {
        assert_eq!(potion.id, expected_id, "potion registry IDs must be dense");
        let ident = Ident::new(&potion.name.to_shouty_snake_case(), Span::call_site());
        let effects_ident = Ident::new(
            &format!("{}_EFFECTS", potion.name.to_shouty_snake_case()),
            Span::call_site(),
        );
        let name = &potion.name;
        let effects = potion.effects.iter().map(|effect| {
            let effect_ident = Ident::new(&effect.effect.to_shouty_snake_case(), Span::call_site());
            let duration = effect.duration;
            let amplifier = effect.amplifier;
            quote! {
                PotionEffect {
                    effect: &vanilla_mob_effects::#effect_ident,
                    duration: #duration,
                    amplifier: #amplifier,
                }
            }
        });
        definitions.extend(quote! {
            static #effects_ident: &[PotionEffect] = &[#(#effects),*];
            pub static #ident: Potion = Potion::new(
                Identifier::vanilla_static(#name),
                #effects_ident,
            );
        });
        registrations.extend(quote! { registry.register(&#ident); });
    }

    quote! {
        use crate::potion::{Potion, PotionEffect, PotionRegistry};
        use crate::vanilla_mob_effects;
        use steel_utils::Identifier;

        #definitions

        pub fn register_potions(registry: &mut PotionRegistry) {
            #registrations
        }
    }
}
