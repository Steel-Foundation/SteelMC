//! Code generation for block behaviors.

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct BlockClass {
    pub name: String,
    pub class: String,
    /// Fluid identifier for `LiquidBlock` (e.g., "water", "lava").
    pub fluid: Option<String>,
    /// Tick delay before the button unpresses (e.g., 20 for stone, 30 for wood).
    pub ticks_to_stay_pressed: Option<i32>,
    /// Sound event constant for button click on (e.g., `BLOCK_STONE_BUTTON_CLICK_ON`).
    pub button_click_on: Option<String>,
    /// Sound event constant for button click off (e.g., `BLOCK_STONE_BUTTON_CLICK_OFF`).
    pub button_click_off: Option<String>,
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

// Tjos is okay cause it's a long function. and because it is needed for like all of those blocks there.
#[allow(clippy::too_many_lines)]
pub fn build(blocks: &[BlockClass]) -> String {
    let mut barrel_blocks = Vec::new();
    let mut button_blocks: Vec<(Ident, i32, Ident, Ident)> = Vec::new();
    let mut candle_blocks = Vec::new();
    let mut crafting_table_blocks = Vec::new();
    let mut crop_blocks = Vec::new();
    let mut end_portal_frame_blocks = Vec::new();
    let mut farm_blocks = Vec::new();
    let mut fence_blocks = Vec::new();
    let mut liquid_blocks = Vec::new();
    let mut rotated_pillar_blocks = Vec::new();
    let mut standing_sign_blocks = Vec::new();
    let mut wall_sign_blocks = Vec::new();
    let mut ceiling_hanging_sign_blocks = Vec::new();
    let mut wall_hanging_sign_blocks = Vec::new();
    let mut torch_blocks = Vec::new();
    let mut wall_torch_blocks = Vec::new();
    let mut redstone_torch_blocks = Vec::new();
    let mut redstone_wall_torch_blocks = Vec::new();

    for block in blocks {
        let const_ident = to_const_ident(&block.name);
        match block.class.as_str() {
            "BarrelBlock" => barrel_blocks.push(const_ident),
            "ButtonBlock" => {
                let ticks = block
                    .ticks_to_stay_pressed
                    .expect("ButtonBlock must have ticks_to_stay_pressed");
                let click_on = Ident::new(
                    block
                        .button_click_on
                        .as_ref()
                        .expect("ButtonBlock must have button_click_on"),
                    Span::call_site(),
                );
                let click_off = Ident::new(
                    block
                        .button_click_off
                        .as_ref()
                        .expect("ButtonBlock must have button_click_off"),
                    Span::call_site(),
                );
                button_blocks.push((const_ident, ticks, click_on, click_off));
            }
            "CandleBlock" => candle_blocks.push(const_ident),
            "CraftingTableBlock" => crafting_table_blocks.push(const_ident),
            "CropBlock" => crop_blocks.push(const_ident),
            "EndPortalFrameBlock" => end_portal_frame_blocks.push(const_ident),
            "FarmBlock" => farm_blocks.push(const_ident),
            "FenceBlock" => fence_blocks.push(const_ident),
            "LiquidBlock" => {
                let fluid_ident =
                    to_const_ident(block.fluid.as_ref().expect("LiquidBlock must have a fluid"));
                liquid_blocks.push((const_ident, fluid_ident));
            }
            "RotatedPillarBlock" => rotated_pillar_blocks.push(const_ident),
            "StandingSignBlock" => standing_sign_blocks.push(const_ident),
            "WallSignBlock" => wall_sign_blocks.push(const_ident),
            "CeilingHangingSignBlock" => ceiling_hanging_sign_blocks.push(const_ident),
            "WallHangingSignBlock" => wall_hanging_sign_blocks.push(const_ident),
            "TorchBlock" => torch_blocks.push(const_ident),
            "WallTorchBlock" => wall_torch_blocks.push(const_ident),
            "RedstoneTorchBlock" => redstone_torch_blocks.push(const_ident),
            "RedstoneWallTorchBlock" => redstone_wall_torch_blocks.push(const_ident),
            _ => {}
        }
    }

    let barrel_type = Ident::new("BarrelBlock", Span::call_site());
    let candle_type = Ident::new("CandleBlock", Span::call_site());
    let crafting_table_type = Ident::new("CraftingTableBlock", Span::call_site());
    let crop_type = Ident::new("CropBlock", Span::call_site());
    let end_portal_frame_type = Ident::new("EndPortalFrameBlock", Span::call_site());
    let farmland_type = Ident::new("FarmlandBlock", Span::call_site());
    let fence_type = Ident::new("FenceBlock", Span::call_site());
    let pillar_type = Ident::new("RotatedPillarBlock", Span::call_site());
    let standing_sign_type = Ident::new("StandingSignBlock", Span::call_site());
    let wall_sign_type = Ident::new("WallSignBlock", Span::call_site());
    let ceiling_hanging_sign_type = Ident::new("CeilingHangingSignBlock", Span::call_site());
    let wall_hanging_sign_type = Ident::new("WallHangingSignBlock", Span::call_site());
    let torch_type = Ident::new("TorchBlock", Span::call_site());
    let wall_torch_type = Ident::new("WallTorchBlock", Span::call_site());
    let redstone_torch_type = Ident::new("RedstoneTorchBlock", Span::call_site());
    let redstone_wall_torch_type = Ident::new("RedstoneWallTorchBlock", Span::call_site());

    let barrel_registrations = generate_registrations(barrel_blocks.iter(), &barrel_type);
    let button_registrations = {
        let registrations =
            button_blocks
                .iter()
                .map(|(block_ident, ticks, click_on, click_off)| {
                    quote! {
                        registry.set_behavior(
                            vanilla_blocks::#block_ident,
                            Box::new(ButtonBlock::new(
                                vanilla_blocks::#block_ident,
                                #ticks,
                                sound_events::#click_on,
                                sound_events::#click_off,
                            )),
                        );
                    }
                });
        quote! { #(#registrations)* }
    };
    let candle_registrations = generate_registrations(candle_blocks.iter(), &candle_type);
    let crafting_table_registrations =
        generate_registrations(crafting_table_blocks.iter(), &crafting_table_type);
    let crop_registrations = generate_registrations(crop_blocks.iter(), &crop_type);
    let end_portal_frame_registrations =
        generate_registrations(end_portal_frame_blocks.iter(), &end_portal_frame_type);
    let farm_registrations = generate_registrations(farm_blocks.iter(), &farmland_type);
    let fence_registrations = generate_registrations(fence_blocks.iter(), &fence_type);
    let liquid_registrations = {
        let registrations = liquid_blocks.iter().map(|(block_ident, fluid_ident)| {
            quote! {
                registry.set_behavior(
                    vanilla_blocks::#block_ident,
                    Box::new(LiquidBlock::new(vanilla_blocks::#block_ident, &vanilla_fluids::#fluid_ident)),
                );
            }
        });
        quote! { #(#registrations)* }
    };
    let pillar_registrations = generate_registrations(rotated_pillar_blocks.iter(), &pillar_type);
    let standing_sign_registrations =
        generate_registrations(standing_sign_blocks.iter(), &standing_sign_type);
    let wall_sign_registrations = generate_registrations(wall_sign_blocks.iter(), &wall_sign_type);
    let ceiling_hanging_sign_registrations = generate_registrations(
        ceiling_hanging_sign_blocks.iter(),
        &ceiling_hanging_sign_type,
    );
    let wall_hanging_sign_registrations =
        generate_registrations(wall_hanging_sign_blocks.iter(), &wall_hanging_sign_type);
    let torch_registrations = generate_registrations(torch_blocks.iter(), &torch_type);
    let wall_torch_registrations =
        generate_registrations(wall_torch_blocks.iter(), &wall_torch_type);
    let redstone_torch_registrations =
        generate_registrations(redstone_torch_blocks.iter(), &redstone_torch_type);
    let redstone_wall_torch_registrations =
        generate_registrations(redstone_wall_torch_blocks.iter(), &redstone_wall_torch_type);

    let output = quote! {
        //! Generated block behavior assignments.

        use steel_registry::{sound_events, vanilla_blocks, vanilla_fluids};
        use crate::behavior::BlockBehaviorRegistry;
        use crate::behavior::blocks::{
            BarrelBlock, ButtonBlock, CandleBlock, CraftingTableBlock, CropBlock, EndPortalFrameBlock,
            FarmlandBlock, FenceBlock, LiquidBlock, RotatedPillarBlock, StandingSignBlock, WallSignBlock,
            CeilingHangingSignBlock, WallHangingSignBlock, TorchBlock, WallTorchBlock,
            RedstoneTorchBlock, RedstoneWallTorchBlock,
        };

        pub fn register_block_behaviors(registry: &mut BlockBehaviorRegistry) {
            #barrel_registrations
            #button_registrations
            #candle_registrations
            #crafting_table_registrations
            #crop_registrations
            #end_portal_frame_registrations
            #farm_registrations
            #fence_registrations
            #liquid_registrations
            #pillar_registrations
            #standing_sign_registrations
            #wall_sign_registrations
            #ceiling_hanging_sign_registrations
            #wall_hanging_sign_registrations
            #torch_registrations
            #wall_torch_registrations
            #redstone_torch_registrations
            #redstone_wall_torch_registrations
        }
    };

    output.to_string()
}
