//! Code generation for block behaviors.

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote};
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
    pub max_age: Option<i32>,
}

fn to_const_ident(name: &str) -> Ident {
    Ident::new(&name.to_shouty_snake_case(), Span::call_site())
}

/// Derives the `WeatherState` variant from a block name based on its prefix.
/// TODO: Extract this?
fn weather_state_from_name(name: &str) -> Ident {
    let variant = if name.starts_with("oxidized_") {
        "Oxidized"
    } else if name.starts_with("weathered_") {
        "Weathered"
    } else if name.starts_with("exposed_") {
        "Exposed"
    } else {
        "Unaffected"
    };
    Ident::new(variant, Span::call_site())
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

fn generate_crop_registrations<'a>(
    blocks: impl Iterator<Item = &'a (Ident, i32)>,
    behavior_type: &Ident,
) -> TokenStream {
    let registrations = blocks.map(|(ident, max_age)| {
        let max_age_ident = format_ident!("AGE_{max_age}");
        let max_age = *max_age as u8;
        quote! {
            registry.set_behavior(
                vanilla_blocks::#ident,
                Box::new(#behavior_type::with_age(vanilla_blocks::#ident, BlockStateProperties::#max_age_ident, #max_age)),
            );
        }
    });
    quote! { #(#registrations)* }
}

// Tjos is okay cause it's a long function. and because it is needed for like all of those blocks there.
#[allow(clippy::too_many_lines)]
pub fn build(blocks: &[BlockClass]) -> String {
    // WARNING: PLEASE KEEP ALPHABETICALLY ORDERED <3
    let mut bamboo_sapling_blocks = Vec::new();
    let mut bamboo_stalk_blocks = Vec::new();
    let mut barrel_blocks = Vec::new();
    let mut beetroots_blocks = Vec::new();
    let mut button_blocks: Vec<(Ident, i32, Ident, Ident)> = Vec::new();
    let mut cactus_blocks = Vec::new();
    let mut cactus_flower_blocks: Vec<Ident> = Vec::new();
    let mut candle_blocks = Vec::new();
    let mut ceiling_hanging_sign_blocks = Vec::new();
    let mut crafting_table_blocks = Vec::new();
    let mut crop_blocks = Vec::new();
    let mut end_portal_frame_blocks = Vec::new();
    let mut farm_blocks = Vec::new();
    let mut fence_blocks = Vec::new();
    let mut flower_blocks = Vec::new();
    let mut liquid_blocks = Vec::new();
    let mut redstone_torch_blocks = Vec::new();
    let mut redstone_wall_torch_blocks = Vec::new();
    let mut rotated_pillar_blocks = Vec::new();
    let mut seagrass_blocks = Vec::new();
    let mut standing_sign_blocks = Vec::new();
    let mut tall_seagrass_blocks = Vec::new();
    let mut torch_blocks = Vec::new();
    let mut torchflower_blocks = Vec::new();
    let mut wall_hanging_sign_blocks = Vec::new();
    let mut wall_sign_blocks = Vec::new();
    let mut wall_torch_blocks = Vec::new();
    let mut weathering_full_blocks: Vec<(Ident, Ident)> = Vec::new();

    for block in blocks {
        let const_ident = to_const_ident(&block.name);
        match block.class.as_str() {
            "BambooSaplingBlock" => bamboo_sapling_blocks.push(const_ident),
            "BambooStalkBlock" => bamboo_stalk_blocks.push(const_ident),
            "BarrelBlock" => barrel_blocks.push(const_ident),
            "BeetrootBlock" => {
                beetroots_blocks.push((
                    const_ident,
                    block
                        .max_age
                        .expect("Beetroots Blocks should have a max_age attribute!"),
                ));
            }
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
            "CactusBlock" => cactus_blocks.push(const_ident),
            "CactusFlowerBlock" => cactus_flower_blocks.push(const_ident),
            "CandleBlock" => candle_blocks.push(const_ident),
            "CeilingHangingSignBlock" => ceiling_hanging_sign_blocks.push(const_ident),
            "CraftingTableBlock" => crafting_table_blocks.push(const_ident),
            "CropBlock" | "CarrotBlock" | "PotatoBlock" => {
                crop_blocks.push((
                    const_ident,
                    block
                        .max_age
                        .expect("Crop Blocks should have a max_age attribute!"),
                ));
            }
            "EndPortalFrameBlock" => end_portal_frame_blocks.push(const_ident),
            "FarmBlock" => farm_blocks.push(const_ident),
            "FenceBlock" => fence_blocks.push(const_ident),
            "FlowerBlock" => flower_blocks.push(const_ident),
            "LiquidBlock" => {
                let fluid_ident =
                    to_const_ident(block.fluid.as_ref().expect("LiquidBlock must have a fluid"));
                liquid_blocks.push((const_ident, fluid_ident));
            }
            "RedstoneTorchBlock" => redstone_torch_blocks.push(const_ident),
            "RedstoneWallTorchBlock" => redstone_wall_torch_blocks.push(const_ident),
            "RotatedPillarBlock" => rotated_pillar_blocks.push(const_ident),
            "SeagrassBlock" => seagrass_blocks.push(const_ident),
            "StandingSignBlock" => standing_sign_blocks.push(const_ident),
            "TallSeagrassBlock" => tall_seagrass_blocks.push(const_ident),
            "TorchBlock" => torch_blocks.push(const_ident),
            "TorchflowerCropBlock" => {
                torchflower_blocks.push((
                    const_ident,
                    block
                        .max_age
                        .expect("Torchflower Blocks should have a max_age attribute!"),
                ));
            }
            "WallHangingSignBlock" => wall_hanging_sign_blocks.push(const_ident),
            "WallSignBlock" => wall_sign_blocks.push(const_ident),
            "WallTorchBlock" => wall_torch_blocks.push(const_ident),
            "WeatheringCopperFullBlock" => {
                let weather_state = weather_state_from_name(&block.name);
                weathering_full_blocks.push((const_ident, weather_state));
            }
            _ => {}
        }
    }

    let bamboo_sapling_type = Ident::new("BambooSaplingBlock", Span::call_site());
    let bamboo_stalk_type = Ident::new("BambooStalkBlock", Span::call_site());
    let barrel_type = Ident::new("BarrelBlock", Span::call_site());
    let beetroots_type = Ident::new("BeetrootBlock", Span::call_site());
    let cactus_flower_type = Ident::new("CactusFlowerBlock", Span::call_site());
    let cactus_type = Ident::new("CactusBlock", Span::call_site());
    let candle_type = Ident::new("CandleBlock", Span::call_site());
    let ceiling_hanging_sign_type = Ident::new("CeilingHangingSignBlock", Span::call_site());
    let crafting_table_type = Ident::new("CraftingTableBlock", Span::call_site());
    let crop_type = Ident::new("CropBlock", Span::call_site());
    let end_portal_frame_type = Ident::new("EndPortalFrameBlock", Span::call_site());
    let farmland_type = Ident::new("FarmlandBlock", Span::call_site());
    let fence_type = Ident::new("FenceBlock", Span::call_site());
    let flower_type = Ident::new("FlowerBlock", Span::call_site());
    let pillar_type = Ident::new("RotatedPillarBlock", Span::call_site());
    let redstone_torch_type = Ident::new("RedstoneTorchBlock", Span::call_site());
    let redstone_wall_torch_type = Ident::new("RedstoneWallTorchBlock", Span::call_site());
    let seagrass_type = Ident::new("SeagrassBlock", Span::call_site());
    let standing_sign_type = Ident::new("StandingSignBlock", Span::call_site());
    let tall_seagrass_type = Ident::new("TallSeagrassBlock", Span::call_site());
    let torch_type = Ident::new("TorchBlock", Span::call_site());
    let torchflower_type = Ident::new("TorchflowerBlock", Span::call_site());
    let wall_hanging_sign_type = Ident::new("WallHangingSignBlock", Span::call_site());
    let wall_sign_type = Ident::new("WallSignBlock", Span::call_site());
    let wall_torch_type = Ident::new("WallTorchBlock", Span::call_site());

    let bamboo_sapling_registrations =
        generate_registrations(bamboo_sapling_blocks.iter(), &bamboo_sapling_type);
    let bamboo_stalk_registrations =
        generate_registrations(bamboo_stalk_blocks.iter(), &bamboo_stalk_type);
    let barrel_registrations = generate_registrations(barrel_blocks.iter(), &barrel_type);
    let beetroots_registrations =
        generate_crop_registrations(beetroots_blocks.iter(), &beetroots_type);
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
    let cactus_flower_registrations =
        generate_registrations(cactus_flower_blocks.iter(), &cactus_flower_type);
    let cactus_registrations = generate_registrations(cactus_blocks.iter(), &cactus_type);
    let candle_registrations = generate_registrations(candle_blocks.iter(), &candle_type);
    let ceiling_hanging_sign_registrations = generate_registrations(
        ceiling_hanging_sign_blocks.iter(),
        &ceiling_hanging_sign_type,
    );
    let crafting_table_registrations =
        generate_registrations(crafting_table_blocks.iter(), &crafting_table_type);
    let crop_registrations = generate_crop_registrations(crop_blocks.iter(), &crop_type);
    let end_portal_frame_registrations =
        generate_registrations(end_portal_frame_blocks.iter(), &end_portal_frame_type);
    let farm_registrations = generate_registrations(farm_blocks.iter(), &farmland_type);
    let fence_registrations = generate_registrations(fence_blocks.iter(), &fence_type);
    let flower_registrations = generate_registrations(flower_blocks.iter(), &flower_type);
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
    let redstone_torch_registrations =
        generate_registrations(redstone_torch_blocks.iter(), &redstone_torch_type);
    let redstone_wall_torch_registrations =
        generate_registrations(redstone_wall_torch_blocks.iter(), &redstone_wall_torch_type);
    let seagrass_registrations = generate_registrations(seagrass_blocks.iter(), &seagrass_type);
    let standing_sign_registrations =
        generate_registrations(standing_sign_blocks.iter(), &standing_sign_type);
    let tall_seagrass_registrations =
        generate_registrations(tall_seagrass_blocks.iter(), &tall_seagrass_type);
    let torch_registrations = generate_registrations(torch_blocks.iter(), &torch_type);
    let torchflower_registrations =
        generate_crop_registrations(torchflower_blocks.iter(), &torchflower_type);
    let wall_hanging_sign_registrations =
        generate_registrations(wall_hanging_sign_blocks.iter(), &wall_hanging_sign_type);
    let wall_sign_registrations = generate_registrations(wall_sign_blocks.iter(), &wall_sign_type);
    let wall_torch_registrations =
        generate_registrations(wall_torch_blocks.iter(), &wall_torch_type);
    let weathering_full_block_registrations = {
        let registrations = weathering_full_blocks
            .iter()
            .map(|(block_ident, state_ident)| {
                quote! {
                    registry.set_behavior(
                        vanilla_blocks::#block_ident,
                        Box::new(WeatheringCopperFullBlock::new(
                            vanilla_blocks::#block_ident,
                            WeatherState::#state_ident,
                        )),
                    );
                }
            });
        quote! { #(#registrations)* }
    };

    let output = quote! {
        //! Generated block behavior assignments.

        use steel_registry::blocks::properties::BlockStateProperties;
        use steel_registry::{sound_events, vanilla_blocks, vanilla_fluids};

        use crate::behavior::blocks::{
            crops::{BambooSaplingBlock, BambooStalkBlock, BeetrootBlock, CactusBlock, CactusFlowerBlock, CropBlock, FlowerBlock, SeagrassBlock, TallSeagrassBlock, TorchflowerBlock},
            BarrelBlock,
            ButtonBlock,
            CandleBlock,
            CeilingHangingSignBlock,
            CraftingTableBlock,
            EndPortalFrameBlock,
            FarmlandBlock,
            FenceBlock,
            LiquidBlock,
            RedstoneTorchBlock,
            RedstoneWallTorchBlock,
            RotatedPillarBlock,
            StandingSignBlock,
            TorchBlock,
            WallHangingSignBlock,
            WallSignBlock,
            WallTorchBlock,
            WeatherState,
            WeatheringCopperFullBlock,
        };
        use crate::behavior::BlockBehaviorRegistry;

        pub fn register_block_behaviors(registry: &mut BlockBehaviorRegistry) {
            #bamboo_sapling_registrations
            #bamboo_stalk_registrations
            #barrel_registrations
            #beetroots_registrations
            #button_registrations
            #cactus_registrations
            #cactus_flower_registrations
            #candle_registrations
            #ceiling_hanging_sign_registrations
            #crafting_table_registrations
            #crop_registrations
            #end_portal_frame_registrations
            #farm_registrations
            #fence_registrations
            #flower_registrations
            #liquid_registrations
            #pillar_registrations
            #redstone_torch_registrations
            #redstone_wall_torch_registrations
            #seagrass_registrations
            #standing_sign_registrations
            #tall_seagrass_registrations
            #torch_registrations
            #torchflower_registrations
            #wall_hanging_sign_registrations
            #wall_sign_registrations
            #wall_torch_registrations
            #weathering_full_block_registrations
        }
    };

    output.to_string()
}
