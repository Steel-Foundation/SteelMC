//! Code generation for item behaviors.

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ItemClass {
    pub name: String,
    pub class: String,
    #[serde(default)]
    pub block: Option<String>,
}

/// Items use lowercase field names (`vanilla_items::ITEMS.stone`)
fn to_item_field(name: &str) -> Ident {
    Ident::new(name, Span::call_site())
}

/// Blocks use `SCREAMING_SNAKE_CASE` constants (`vanilla_blocks::STONE`)
fn to_block_const(name: &str) -> Ident {
    Ident::new(&name.to_shouty_snake_case(), Span::call_site())
}

fn generate_block_item_registrations<'a>(
    items: impl Iterator<Item = &'a (Ident, Ident)>,
) -> TokenStream {
    let registrations = items.map(|(item_field, block_const)| {
        quote! {
            registry.set_behavior(
                &vanilla_items::ITEMS.#item_field,
                Box::new(BlockItemBehavior::new(vanilla_blocks::#block_const)),
            );
        }
    });
    quote! { #(#registrations)* }
}

fn generate_simple_registrations<'a>(
    items: impl Iterator<Item = &'a Ident>,
    behavior_type: &Ident,
) -> TokenStream {
    let registrations = items.map(|item_field| {
        quote! {
            registry.set_behavior(
                &vanilla_items::ITEMS.#item_field,
                Box::new(#behavior_type),
            );
        }
    });
    quote! { #(#registrations)* }
}

pub fn build(items: &[ItemClass]) -> String {
    let mut block_items: Vec<(Ident, Ident)> = Vec::new();
    let mut ender_eye_items: Vec<Ident> = Vec::new();

    for item in items {
        let item_field = to_item_field(&item.name);

        match item.class.as_str() {
            "BlockItem" | "DoubleHighBlockItem" => {
                if let Some(block) = &item.block {
                    block_items.push((item_field, to_block_const(block)));
                }
            }
            "EnderEyeItem" => ender_eye_items.push(item_field),
            _ => {}
        }
    }

    let block_item_registrations = generate_block_item_registrations(block_items.iter());

    let ender_eye_type = Ident::new("EnderEyeBehavior", Span::call_site());
    let ender_eye_registrations =
        generate_simple_registrations(ender_eye_items.iter(), &ender_eye_type);

    let output = quote! {
        //! Generated item behavior assignments.

        use steel_registry::{vanilla_blocks, vanilla_items};
        use crate::behavior::ItemBehaviorRegistry;
        use crate::behavior::items::{BlockItemBehavior, EnderEyeBehavior};

        pub fn register_item_behaviors(registry: &mut ItemBehaviorRegistry) {
            #block_item_registrations
            #ender_eye_registrations
        }
    };

    output.to_string()
}
