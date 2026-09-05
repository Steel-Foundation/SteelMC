//! Generates vanilla flower suspicious-stew effects extracted from `SuspiciousEffectHolder`.

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

use crate::generator_functions::read_json_asset;

#[derive(Deserialize)]
struct EffectHolderEntry {
    item: String,
    effects: Vec<EffectEntry>,
}

#[derive(Deserialize)]
struct EffectEntry {
    effect: String,
    duration: i32,
}

pub(crate) fn build() -> TokenStream {
    const ASSET: &str = "build_assets/suspicious_stew_effects.json";

    let effect_holders: Vec<EffectHolderEntry> = read_json_asset(ASSET);
    let mut cases = TokenStream::new();

    for holder in effect_holders {
        let item = vanilla_ident(&holder.item, "item");
        let effects = holder.effects.iter().map(|entry| {
            let effect = vanilla_ident(&entry.effect, "mob effect");
            let duration = entry.duration;
            quote! { SuspiciousStewEffect::new(&vanilla_mob_effects::#effect, #duration) }
        });
        cases.extend(quote! {
            if std::ptr::eq(item, &*vanilla_items::#item) {
                return Some(SuspiciousStewEffects::new(vec![#(#effects),*]));
            }
        });
    }

    quote! {
        use crate::data_components::components::{SuspiciousStewEffect, SuspiciousStewEffects};
        use crate::items::ItemRef;
        use crate::{vanilla_items, vanilla_mob_effects};

        /// Returns the vanilla suspicious-stew effects supplied by a flower item.
        #[must_use]
        pub fn from_item(item: ItemRef) -> Option<SuspiciousStewEffects> {
            #cases
            None
        }
    }
}

fn vanilla_ident(identifier: &str, kind: &str) -> Ident {
    let Some(path) = identifier.strip_prefix("minecraft:") else {
        panic!("extracted {kind} must use the minecraft namespace: {identifier}");
    };
    Ident::new(&path.to_shouty_snake_case(), Span::call_site())
}
