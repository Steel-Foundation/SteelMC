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
    #[serde(default)]
    #[serde(rename = "standingAndWallBlockItem")]
    pub standing_and_wall_block_item: Option<String>,
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

fn generate_sign_item_registrations<'a>(
    items: impl Iterator<Item = &'a (Ident, Ident, Ident)>,
) -> TokenStream {
    let registrations = items.map(|(item_field, standing_const, wall_const)| {
        quote! {
            registry.set_behavior(
                &vanilla_items::ITEMS.#item_field,
                Box::new(SignItemBehavior::new(vanilla_blocks::#standing_const, vanilla_blocks::#wall_const)),
            );
        }
    });
    quote! { #(#registrations)* }
}

fn generate_hanging_sign_item_registrations<'a>(
    items: impl Iterator<Item = &'a (Ident, Ident, Ident)>,
) -> TokenStream {
    let registrations = items.map(|(item_field, ceiling_const, wall_const)| {
        quote! {
            registry.set_behavior(
                &vanilla_items::ITEMS.#item_field,
                Box::new(HangingSignItemBehavior::new(vanilla_blocks::#ceiling_const, vanilla_blocks::#wall_const)),
            );
        }
    });
    quote! { #(#registrations)* }
}

fn generate_standing_and_wall_item_registrations<'a>(
    items: impl Iterator<Item = &'a (Ident, Ident, Ident)>,
) -> TokenStream {
    let registrations = items.map(|(item_field, standing_const, wall_const)| {
        // All vanilla StandingAndWallBlockItem instances use Direction::Down
        // (torches, coral fans, skulls/heads, redstone torch)
        quote! {
            registry.set_behavior(
                &vanilla_items::ITEMS.#item_field,
                Box::new(StandingAndWallBlockItem::new(
                    vanilla_blocks::#standing_const,
                    vanilla_blocks::#wall_const,
                    steel_registry::blocks::properties::Direction::Down,
                )),
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
    let mut sign_items: Vec<(Ident, Ident, Ident)> = Vec::new();
    let mut hanging_sign_items: Vec<(Ident, Ident, Ident)> = Vec::new();
    let mut standing_and_wall_items: Vec<(Ident, Ident, Ident)> = Vec::new();
    let mut ender_eye_items: Vec<Ident> = Vec::new();

    for item in items {
        let item_field = to_item_field(&item.name);

        match item.class.as_str() {
            "BlockItem" | "DoubleHighBlockItem" => {
                if let Some(block) = &item.block {
                    block_items.push((item_field, to_block_const(block)));
                }
            }
            "SignItem" => {
                let block = item.block.as_ref().expect("SignItem missing `block`");
                let wall_block = item
                    .standing_and_wall_block_item
                    .as_ref()
                    .expect("SignItem missing `standingAndWallBlockItem`");
                let standing_const = to_block_const(block);
                let wall_const = to_block_const(wall_block);
                sign_items.push((item_field, standing_const, wall_const));
            }
            "HangingSignItem" => {
                let block = item
                    .block
                    .as_ref()
                    .expect("HangingSignItem missing `block`");
                let wall_block = item
                    .standing_and_wall_block_item
                    .as_ref()
                    .expect("HangingSignItem missing `standingAndWallBlockItem`");
                let ceiling_const = to_block_const(block);
                let wall_const = to_block_const(wall_block);
                hanging_sign_items.push((item_field, ceiling_const, wall_const));
            }
            "StandingAndWallBlockItem" => {
                let block = item
                    .block
                    .as_ref()
                    .expect("StandingAndWallBlockItem missing `block`");
                let wall_block = item
                    .standing_and_wall_block_item
                    .as_ref()
                    .expect("StandingAndWallBlockItem missing `standingAndWallBlockItem`");
                let standing_const = to_block_const(block);
                let wall_const = to_block_const(wall_block);
                standing_and_wall_items.push((item_field, standing_const, wall_const));
            }
            "EnderEyeItem" => ender_eye_items.push(item_field),
            _ => {}
        }
    }

    let block_item_registrations = generate_block_item_registrations(block_items.iter());
    let sign_item_registrations = generate_sign_item_registrations(sign_items.iter());
    let hanging_sign_item_registrations =
        generate_hanging_sign_item_registrations(hanging_sign_items.iter());
    let standing_and_wall_item_registrations =
        generate_standing_and_wall_item_registrations(standing_and_wall_items.iter());

    let ender_eye_type = Ident::new("EnderEyeBehavior", Span::call_site());
    let ender_eye_registrations =
        generate_simple_registrations(ender_eye_items.iter(), &ender_eye_type);

    let output = quote! {
        //! Generated item behavior assignments.

        use steel_registry::{vanilla_blocks, vanilla_items};
        use crate::behavior::ItemBehaviorRegistry;
        use crate::behavior::items::{BlockItemBehavior, EnderEyeBehavior, HangingSignItemBehavior, SignItemBehavior, StandingAndWallBlockItem};

        pub fn register_item_behaviors(registry: &mut ItemBehaviorRegistry) {
            #block_item_registrations
            #sign_item_registrations
            #hanging_sign_item_registrations
            #standing_and_wall_item_registrations
            #ender_eye_registrations
        }
    };

    output.to_string()
}
