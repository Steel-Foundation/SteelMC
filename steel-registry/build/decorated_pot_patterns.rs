use crate::generator_functions::{generate_identifier, read_json_asset};
use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use steel_utils::Identifier;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct DecoratedPotPatternJson {
    asset_id: Identifier,
}

pub(crate) fn build() -> TokenStream {
    // DecoratedPotPatterns.bootstrap defines registry insertion order in
    // Vanilla; it matches the alphabetical file order of the datapack
    // registry entries (`minecraft:blank` is a code constant, not a
    // datapack entry, and is excluded here).
    const VANILLA_ORDER: &[&str] = &[
        "angler",
        "archer",
        "arms_up",
        "blade",
        "brewer",
        "burn",
        "danger",
        "explorer",
        "flow",
        "friend",
        "guster",
        "heart",
        "heartbreak",
        "howl",
        "miner",
        "mourner",
        "plenty",
        "prize",
        "scrape",
        "sheaf",
        "shelter",
        "skull",
        "snort",
    ];

    let patterns = VANILLA_ORDER.iter().map(|name| {
        let path = format!(
            "../steel-utils/build_assets/builtin_datapacks/minecraft/decorated_pot_pattern/{name}.json"
        );
        (*name, read_json_asset::<DecoratedPotPatternJson>(&path))
    });

    let mut definitions = TokenStream::new();
    let mut registrations = TokenStream::new();
    for (name, pattern) in patterns {
        let ident = Ident::new(&name.to_shouty_snake_case(), Span::call_site());
        let key = quote! { Identifier::vanilla_static(#name) };
        let asset_id = generate_identifier(&pattern.asset_id);

        definitions.extend(quote! {
            pub static #ident: DecoratedPotPattern = DecoratedPotPattern::new(
                #key,
                DecoratedPotPatternValue::new(#asset_id),
            );
        });
        registrations.extend(quote! {
            registry.register(&#ident);
        });
    }

    quote! {
        use crate::decorated_pot_pattern::{
            DecoratedPotPattern, DecoratedPotPatternRegistry, DecoratedPotPatternValue,
        };
        use std::borrow::Cow;
        use steel_utils::Identifier;

        #definitions

        pub fn register_decorated_pot_patterns(registry: &mut DecoratedPotPatternRegistry) {
            #registrations
        }
    }
}
