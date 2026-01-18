//! Code generation for block behaviors.

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct BlockClass {
    pub name: String,
    pub class: String,
}

fn to_const_ident(name: &str) -> Ident {
    Ident::new(&name.to_shouty_snake_case(), Span::call_site())
}

fn generate_registrations<'a>(
    blocks: impl Iterator<Item = &'a Ident>,
    behavior_type: &Ident,
) -> TokenStream {
    let registrations = blocks.map(|ident| {
        quote! {
            registry.set_behavior(
                vanilla_blocks::#ident,
                Box::new(#behavior_type::new(vanilla_blocks::#ident)),
            );
        }
    });
    quote! { #(#registrations)* }
}

pub fn build(blocks: &[BlockClass]) -> String {
    let mut crafting_table_blocks = Vec::new();
    let mut crop_blocks = Vec::new();
    let mut end_portal_frame_blocks = Vec::new();
    let mut farm_blocks = Vec::new();
    let mut fence_blocks = Vec::new();
    let mut rotated_pillar_blocks = Vec::new();

    for block in blocks {
        let const_ident = to_const_ident(&block.name);
        match block.class.as_str() {
            "CraftingTableBlock" => crafting_table_blocks.push(const_ident),
            "CropBlock" => crop_blocks.push(const_ident),
            "EndPortalFrameBlock" => end_portal_frame_blocks.push(const_ident),
            "FarmBlock" => farm_blocks.push(const_ident),
            "FenceBlock" => fence_blocks.push(const_ident),
            "RotatedPillarBlock" => rotated_pillar_blocks.push(const_ident),
            _ => {}
        }
    }

    let crafting_table_type = Ident::new("CraftingTableBlock", Span::call_site());
    let crop_type = Ident::new("CropBlock", Span::call_site());
    let end_portal_frame_type = Ident::new("EndPortalFrameBlock", Span::call_site());
    let farmland_type = Ident::new("FarmlandBlock", Span::call_site());
    let fence_type = Ident::new("FenceBlock", Span::call_site());
    let pillar_type = Ident::new("RotatedPillarBlock", Span::call_site());

    let crafting_table_registrations =
        generate_registrations(crafting_table_blocks.iter(), &crafting_table_type);
    let crop_registrations = generate_registrations(crop_blocks.iter(), &crop_type);
    let end_portal_frame_registrations =
        generate_registrations(end_portal_frame_blocks.iter(), &end_portal_frame_type);
    let farm_registrations = generate_registrations(farm_blocks.iter(), &farmland_type);
    let fence_registrations = generate_registrations(fence_blocks.iter(), &fence_type);
    let pillar_registrations = generate_registrations(rotated_pillar_blocks.iter(), &pillar_type);

    let output = quote! {
        //! Generated block behavior assignments.

        use steel_registry::vanilla_blocks;
        use crate::behavior::BlockBehaviorRegistry;
        use crate::behavior::blocks::{CraftingTableBlock, CropBlock, EndPortalFrameBlock, FarmlandBlock, FenceBlock, RotatedPillarBlock};

        pub fn register_block_behaviors(registry: &mut BlockBehaviorRegistry) {
            #crafting_table_registrations
            #crop_registrations
            #end_portal_frame_registrations
            #farm_registrations
            #fence_registrations
            #pillar_registrations
        }
    };

    output.to_string()
}
