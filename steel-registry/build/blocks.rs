#![expect(unused)]
// Todo! Remove this^

use core::panic;
use std::{borrow::Cow, fs};

use rustc_hash::FxHashMap;

use heck::{ToShoutySnakeCase, ToUpperCamelCase};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BlockConfig {
    pub has_collision: bool,
    pub can_occlude: bool,
    pub explosion_resistance: f32,
    pub is_randomly_ticking: bool,
    pub force_solid_off: bool,
    pub force_solid_on: bool,
    pub push_reaction: Cow<'static, str>,
    pub friction: f32,
    pub speed_factor: f32,
    pub jump_factor: f32,
    pub dynamic_shape: bool,
    pub offset_type: Cow<'static, str>,
    pub max_horizontal_offset: f32,
    pub max_vertical_offset: f32,
    pub destroy_time: f32,
    pub ignited_by_lava: bool,
    pub liquid: bool,
    pub is_air: bool,
    pub requires_correct_tool_for_drops: bool,
    pub instrument: Cow<'static, str>,
    pub replaceable: bool,
    #[serde(default, rename = "sound_type")]
    pub sound_type: Option<Cow<'static, str>>,
}

impl BlockConfig {
    /// Starts building a new set of block properties.
    pub const fn new() -> Self {
        Self {
            has_collision: true,
            can_occlude: true,
            explosion_resistance: 0.0,
            is_randomly_ticking: false,
            force_solid_off: false,
            force_solid_on: false,
            push_reaction: Cow::Borrowed("NORMAL"),
            friction: 0.6,
            speed_factor: 1.0,
            jump_factor: 1.0,
            dynamic_shape: false,
            offset_type: Cow::Borrowed("NONE"),
            max_horizontal_offset: 0.25,
            max_vertical_offset: 0.2,
            destroy_time: 0.0,
            ignited_by_lava: false,
            liquid: false,
            is_air: false,
            requires_correct_tool_for_drops: false,
            instrument: Cow::Borrowed("HARP"),
            replaceable: false,
            sound_type: None,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct ShapeOverwrite {
    pub offset: u16,
    pub shapes: Vec<u16>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ShapeData {
    #[serde(default, rename = "usesOffset")]
    pub uses_offset: bool,
    pub default: Vec<u16>,
    pub overwrites: Vec<ShapeOverwrite>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Block {
    #[expect(dead_code)]
    pub id: u16,
    pub name: String,
    pub properties: Vec<String>,
    // example bool_true, int_5, enum_Direction_Down
    pub default_properties: Vec<String>,
    pub behavior_properties: BlockConfig,
    pub collision_shapes: ShapeData,
    pub support_shapes: ShapeData,
    pub outline_shapes: ShapeData,
    pub occlusion_shapes: ShapeData,
    pub interaction_shapes: ShapeData,
    pub visual_shapes: ShapeData,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Shape {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

#[derive(Deserialize, Clone, Debug)]
pub struct BlockAssets {
    pub blocks: Vec<Block>,
    pub shapes: Vec<Shape>,
}

/// Converts a push reaction string to a TokenStream representing the enum variant
fn push_reaction_to_tokens(reaction: &str) -> TokenStream {
    match reaction {
        "NORMAL" => quote! { PushReaction::Normal },
        "DESTROY" => quote! { PushReaction::Destroy },
        "BLOCK" => quote! { PushReaction::Block },
        "IGNORE" => quote! { PushReaction::Ignore },
        "PUSH_ONLY" => quote! { PushReaction::PushOnly },
        _ => panic!("Unknown push reaction: {}", reaction),
    }
}

/// Converts an instrument string to a TokenStream representing the enum variant
fn instrument_to_tokens(instrument: &str) -> TokenStream {
    match instrument.to_uppercase().as_str() {
        "HARP" => quote! { NoteBlockInstrument::Harp },
        "BASEDRUM" => quote! { NoteBlockInstrument::Basedrum },
        "SNARE" => quote! { NoteBlockInstrument::Snare },
        "HAT" => quote! { NoteBlockInstrument::Hat },
        "BASS" => quote! { NoteBlockInstrument::Bass },
        "FLUTE" => quote! { NoteBlockInstrument::Flute },
        "BELL" => quote! { NoteBlockInstrument::Bell },
        "GUITAR" => quote! { NoteBlockInstrument::Guitar },
        "CHIME" => quote! { NoteBlockInstrument::Chime },
        "XYLOPHONE" => quote! { NoteBlockInstrument::Xylophone },
        "IRON_XYLOPHONE" => {
            quote! { NoteBlockInstrument::IronXylophone }
        }
        "COW_BELL" => quote! { NoteBlockInstrument::CowBell },
        "DIDGERIDOO" => quote! { NoteBlockInstrument::Didgeridoo },
        "BIT" => quote! { NoteBlockInstrument::Bit },
        "BANJO" => quote! { NoteBlockInstrument::Banjo },
        "PLING" => quote! { NoteBlockInstrument::Pling },
        "TRUMPET" => quote! { NoteBlockInstrument::Trumpet },
        "TRUMPET_EXPOSED" => quote! { NoteBlockInstrument::TrumpetExposed },
        "TRUMPET_OXIDIZED" => quote! { NoteBlockInstrument::TrumpetOxidized },
        "TRUMPET_WEATHERED" => quote! { NoteBlockInstrument::TrumpetWeathered },
        "ZOMBIE" => quote! { NoteBlockInstrument::Zombie },
        "SKELETON" => quote! { NoteBlockInstrument::Skeleton },
        "CREEPER" => quote! { NoteBlockInstrument::Creeper },
        "DRAGON" => quote! { NoteBlockInstrument::Dragon },
        "WITHER_SKELETON" => {
            quote! { NoteBlockInstrument::WitherSkeleton }
        }
        "PIGLIN" => quote! { NoteBlockInstrument::Piglin },
        "CUSTOM_HEAD" => quote! { NoteBlockInstrument::CustomHead },
        _ => panic!("Unknown instrument: {}", instrument),
    }
}

fn offset_type_to_tokens(offset_type: &str) -> TokenStream {
    match offset_type {
        "NONE" => quote! { OffsetType::None },
        "XZ" => quote! { OffsetType::Xz },
        "XYZ" => quote! { OffsetType::Xyz },
        _ => panic!("Unknown offset type: {}", offset_type),
    }
}

/// Generates builder method calls for properties that differ from defaults
fn generate_builder_calls(bp: &BlockConfig, default_props: &BlockConfig) -> Vec<TokenStream> {
    let mut builder_calls = Vec::new();

    if bp.has_collision != default_props.has_collision {
        let val = bp.has_collision;
        builder_calls.push(quote! { .has_collision(#val) });
    }
    if bp.can_occlude != default_props.can_occlude {
        let val = bp.can_occlude;
        builder_calls.push(quote! { .can_occlude(#val) });
    }
    if bp.explosion_resistance != default_props.explosion_resistance {
        let val = bp.explosion_resistance;
        builder_calls.push(quote! { .explosion_resistance(#val) });
    }
    if bp.is_randomly_ticking != default_props.is_randomly_ticking {
        let val = bp.is_randomly_ticking;
        builder_calls.push(quote! { .set_is_randomly_ticking(#val) });
    }
    if bp.force_solid_off != default_props.force_solid_off {
        let val = bp.force_solid_off;
        builder_calls.push(quote! { .force_solid_off(#val) });
    }
    if bp.force_solid_on != default_props.force_solid_on {
        let val = bp.force_solid_on;
        builder_calls.push(quote! { .force_solid_on(#val) });
    }
    if bp.push_reaction != default_props.push_reaction {
        let reaction = push_reaction_to_tokens(bp.push_reaction.as_ref());
        builder_calls.push(quote! { .push_reaction(#reaction) });
    }
    if bp.friction != default_props.friction {
        let val = bp.friction;
        builder_calls.push(quote! { .friction(#val) });
    }
    if bp.speed_factor != default_props.speed_factor {
        let val = bp.speed_factor;
        builder_calls.push(quote! { .speed_factor(#val) });
    }
    if bp.jump_factor != default_props.jump_factor {
        let val = bp.jump_factor;
        builder_calls.push(quote! { .jump_factor(#val) });
    }
    if bp.dynamic_shape != default_props.dynamic_shape {
        let val = bp.dynamic_shape;
        builder_calls.push(quote! { .dynamic_shape(#val) });
    }
    if bp.offset_type != default_props.offset_type {
        let offset_type = offset_type_to_tokens(bp.offset_type.as_ref());
        builder_calls.push(quote! { .offset_type(#offset_type) });
    }
    if bp.max_horizontal_offset != default_props.max_horizontal_offset {
        let val = bp.max_horizontal_offset;
        builder_calls.push(quote! { .max_horizontal_offset(#val) });
    }
    if bp.max_vertical_offset != default_props.max_vertical_offset {
        let val = bp.max_vertical_offset;
        builder_calls.push(quote! { .max_vertical_offset(#val) });
    }
    if bp.destroy_time != default_props.destroy_time {
        let val = bp.destroy_time;
        builder_calls.push(quote! { .destroy_time(#val) });
    }
    if bp.ignited_by_lava != default_props.ignited_by_lava {
        let val = bp.ignited_by_lava;
        builder_calls.push(quote! { .ignited_by_lava(#val) });
    }
    if bp.liquid != default_props.liquid {
        let val = bp.liquid;
        builder_calls.push(quote! { .liquid(#val) });
    }
    if bp.is_air != default_props.is_air {
        let val = bp.is_air;
        builder_calls.push(quote! { .set_is_air(#val) });
    }
    if bp.requires_correct_tool_for_drops != default_props.requires_correct_tool_for_drops {
        let val = bp.requires_correct_tool_for_drops;
        builder_calls.push(quote! { .requires_correct_tool_for_drops(#val) });
    }
    if bp.instrument != default_props.instrument {
        let instrument = instrument_to_tokens(bp.instrument.as_ref());
        builder_calls.push(quote! { .instrument(#instrument) });
    }
    if bp.replaceable != default_props.replaceable {
        let val = bp.replaceable;
        builder_calls.push(quote! { .replaceable(#val) });
    }
    if let Some(sound_type) = &bp.sound_type {
        let sound_type_ident = Ident::new(sound_type, Span::call_site());
        builder_calls.push(quote! { .sound_type(crate::sound_types::#sound_type_ident) });
    }

    builder_calls
}

/// Generates the default state initialization for blocks with properties
fn generate_default_state(block: &Block) -> TokenStream {
    if block.properties.is_empty() || block.default_properties.is_empty() {
        return quote! {};
    }

    let property_values = block
        .properties
        .iter()
        .zip(block.default_properties.iter())
        .map(|(prop_name, default_val)| {
            let property_ident =
                Ident::new(&prop_name.to_shouty_snake_case(), Span::call_site());

            // Parse the default value format
            let value_expr = if default_val.starts_with("bool_") {
                // Boolean: "bool_true" or "bool_false"
                let bool_val = default_val == "bool_true";
                quote! {
                    BlockStateProperties::#property_ident.index_of(#bool_val)
                }
            } else if default_val.starts_with("int_") {
                // Integer: "int_5" - convert to internal index (value - min)
                let int_val = default_val
                    .strip_prefix("int_")
                    .unwrap()
                    .parse::<u8>()
                    .unwrap();
                quote! { BlockStateProperties::#property_ident.get_internal_index_const(&#int_val) }
            } else if default_val.starts_with("enum_") {
                // Enum: "enum_Direction_Down" -> Direction::Down
                let enum_part = default_val.strip_prefix("enum_").unwrap();
                let parts: Vec<&str> = enum_part.split('_').collect();

                if parts.len() >= 2 {
                    // First part is enum type, rest is variant name
                    let enum_type = parts[0];
                    let variant_name = parts[1..].join("_");

                    let enum_type_ident = Ident::new(enum_type, Span::call_site());
                    let variant_ident =
                        Ident::new(&variant_name.to_upper_camel_case(), Span::call_site());

                    quote! { BlockStateProperties::#property_ident.get_internal_index_const(&properties::#enum_type_ident::#variant_ident) }
                } else {
                    // Fallback if format is unexpected
                    quote! { 0 }
                }
            } else {
                // Unknown format, default to 0
                quote! { 0 }
            };

            quote! {
                BlockStateProperties::#property_ident => #value_expr
            }
        })
        .collect::<Vec<_>>();

    quote! {
        .with_default_state(offset!(
            #(#property_values),*
        ))
    }
}

/// VoxelShape pool that deduplicates shape combinations.
/// Maps block-local box index combinations to a ShapeId.
struct VoxelShapePool {
    // Maps sorted block-local box indices to ShapeId.
    shapes: FxHashMap<Vec<u16>, u16>,
    // Ordered list of shapes for generation
    shape_list: Vec<Vec<u16>>,
}

impl VoxelShapePool {
    fn new() -> Self {
        let mut pool = Self {
            shapes: FxHashMap::default(),
            shape_list: Vec::new(),
        };
        // Reserve ID 0 for empty shape, ID 1 for full block
        pool.get_or_insert(vec![]); // EMPTY = 0
        pool.get_or_insert(vec![u16::MAX]); // FULL_BLOCK = 1 (special marker)
        pool
    }

    fn get_or_insert(&mut self, aabb_indices: Vec<u16>) -> u16 {
        if let Some(&id) = self.shapes.get(&aabb_indices) {
            return id;
        }
        let id = self.shape_list.len() as u16;
        self.shapes.insert(aabb_indices.clone(), id);
        self.shape_list.push(aabb_indices);
        id
    }
}

/// Represents a unique shape function signature (default + match arms).
/// Used to deduplicate identical shape functions across blocks.
#[derive(Clone, PartialEq, Eq, Hash)]
struct ShapeFunctionSignature {
    default_id: u16,
    // Sorted arms: Vec of (sorted offsets, shape_id)
    arms: Vec<(Vec<u16>, u16)>,
}

/// Pool for deduplicating shape functions.
struct ShapeFunctionPool {
    // Maps function signature to function ID
    functions: FxHashMap<ShapeFunctionSignature, u16>,
    // Ordered list of function signatures for generation
    function_list: Vec<ShapeFunctionSignature>,
}

impl ShapeFunctionPool {
    fn new() -> Self {
        Self {
            functions: FxHashMap::default(),
            function_list: Vec::new(),
        }
    }

    fn get_or_insert(&mut self, sig: ShapeFunctionSignature) -> u16 {
        if let Some(&id) = self.functions.get(&sig) {
            return id;
        }
        let id = self.function_list.len() as u16;
        self.functions.insert(sig.clone(), id);
        self.function_list.push(sig);
        id
    }
}

/// Generates a match arm for shape overwrites.
/// Groups offsets with the same shape ID together.
fn generate_shape_match(
    shape_data: &ShapeData,
    voxel_pool: &mut VoxelShapePool,
) -> (u16, Vec<(Vec<u16>, u16)>) {
    // Get default shape ID
    let default_id = voxel_pool.get_or_insert(shape_data.default.clone());

    // Group overwrites by their shape (to combine offsets with | patterns)
    let mut shape_to_offsets: FxHashMap<Vec<u16>, Vec<u16>> = FxHashMap::default();
    for overwrite in &shape_data.overwrites {
        shape_to_offsets
            .entry(overwrite.shapes.clone())
            .or_default()
            .push(overwrite.offset);
    }

    // Convert to (offsets, shape_id) pairs
    let mut arms: Vec<(Vec<u16>, u16)> = shape_to_offsets
        .into_iter()
        .map(|(shapes, mut offsets)| {
            offsets.sort();
            let shape_id = voxel_pool.get_or_insert(shapes);
            (offsets, shape_id)
        })
        .collect();

    // Sort by first offset for consistent output
    arms.sort_by_key(|(offsets, _)| offsets.first().copied().unwrap_or(0));

    (default_id, arms)
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/blocks.json");
    let block_assets: BlockAssets =
        serde_json::from_str(&fs::read_to_string("build_assets/blocks.json").unwrap()).unwrap();

    // Create default properties for comparison
    let default_props = BlockConfig::new();

    // VoxelShape pool for deduplication
    let mut voxel_pool = VoxelShapePool::new();

    // Shape function pool for deduplication
    let mut shape_fn_pool = ShapeFunctionPool::new();

    // Collect per-block shape function IDs
    struct BlockShapeInfo {
        name: String,
        collision_fn_id: u16,
        support_fn_id: u16,
        outline_fn_id: u16,
        occlusion_fn_id: u16,
        interaction_fn_id: u16,
        visual_fn_id: u16,
        collision_uses_offset: bool,
        support_uses_offset: bool,
        outline_uses_offset: bool,
        occlusion_uses_offset: bool,
        interaction_uses_offset: bool,
        visual_uses_offset: bool,
    }
    let mut block_shape_infos: Vec<BlockShapeInfo> = Vec::new();

    // First pass: collect shape data for all blocks and deduplicate functions
    for block in &block_assets.blocks {
        let (collision_default, collision_arms) =
            generate_shape_match(&block.collision_shapes, &mut voxel_pool);
        let (support_default, support_arms) =
            generate_shape_match(&block.support_shapes, &mut voxel_pool);
        let (outline_default, outline_arms) =
            generate_shape_match(&block.outline_shapes, &mut voxel_pool);
        let (occlusion_default, occlusion_arms) =
            generate_shape_match(&block.occlusion_shapes, &mut voxel_pool);
        let (interaction_default, interaction_arms) =
            generate_shape_match(&block.interaction_shapes, &mut voxel_pool);
        let (visual_default, visual_arms) =
            generate_shape_match(&block.visual_shapes, &mut voxel_pool);

        // Create signatures and get/insert into pool
        let collision_sig = ShapeFunctionSignature {
            default_id: collision_default,
            arms: collision_arms,
        };
        let support_sig = ShapeFunctionSignature {
            default_id: support_default,
            arms: support_arms,
        };
        let outline_sig = ShapeFunctionSignature {
            default_id: outline_default,
            arms: outline_arms,
        };
        let occlusion_sig = ShapeFunctionSignature {
            default_id: occlusion_default,
            arms: occlusion_arms,
        };
        let interaction_sig = ShapeFunctionSignature {
            default_id: interaction_default,
            arms: interaction_arms,
        };
        let visual_sig = ShapeFunctionSignature {
            default_id: visual_default,
            arms: visual_arms,
        };

        let collision_fn_id = shape_fn_pool.get_or_insert(collision_sig);
        let support_fn_id = shape_fn_pool.get_or_insert(support_sig);
        let outline_fn_id = shape_fn_pool.get_or_insert(outline_sig);
        let occlusion_fn_id = shape_fn_pool.get_or_insert(occlusion_sig);
        let interaction_fn_id = shape_fn_pool.get_or_insert(interaction_sig);
        let visual_fn_id = shape_fn_pool.get_or_insert(visual_sig);

        block_shape_infos.push(BlockShapeInfo {
            name: block.name.clone(),
            collision_fn_id,
            support_fn_id,
            outline_fn_id,
            occlusion_fn_id,
            interaction_fn_id,
            visual_fn_id,
            collision_uses_offset: block.collision_shapes.uses_offset,
            support_uses_offset: block.support_shapes.uses_offset,
            outline_uses_offset: block.outline_shapes.uses_offset,
            occlusion_uses_offset: block.occlusion_shapes.uses_offset,
            interaction_uses_offset: block.interaction_shapes.uses_offset,
            visual_uses_offset: block.visual_shapes.uses_offset,
        });
    }

    // Generate block-local box constants.
    let aabb_consts: Vec<TokenStream> = block_assets
        .shapes
        .iter()
        .enumerate()
        .map(|(i, shape)| {
            let name = Ident::new(&format!("BOX_{}", i), Span::call_site());
            let min_x = shape.min[0];
            let min_y = shape.min[1];
            let min_z = shape.min[2];
            let max_x = shape.max[0];
            let max_y = shape.max[1];
            let max_z = shape.max[2];
            quote! {
                const #name: BlockLocalAabb =
                    BlockLocalAabb::new(#min_x, #min_y, #min_z, #max_x, #max_y, #max_z);
            }
        })
        .collect();

    // Generate VoxelShape constants (deduplicated)
    let voxel_shape_consts: Vec<TokenStream> = voxel_pool
        .shape_list
        .iter()
        .enumerate()
        .map(|(id, aabb_indices)| {
            let name = Ident::new(&format!("VSHAPE_{}", id), Span::call_site());
            if aabb_indices.is_empty() {
                quote! {
                    const #name: VoxelShape = VoxelShape::EMPTY;
                }
            } else if aabb_indices.len() == 1 && aabb_indices[0] == u16::MAX {
                quote! {
                    const #name: VoxelShape = VoxelShape::FULL_BLOCK;
                }
            } else {
                let aabb_refs: Vec<TokenStream> = aabb_indices
                    .iter()
                    .map(|&idx| {
                        let aabb_name = Ident::new(&format!("BOX_{}", idx), Span::call_site());
                        quote! { #aabb_name }
                    })
                    .collect();
                quote! {
                    const #name: VoxelShape = VoxelShape::from_boxes(&[#(#aabb_refs),*]);
                }
            }
        })
        .collect();

    // Generate deduplicated shape functions
    let mut shape_fns = TokenStream::new();

    for (fn_id, sig) in shape_fn_pool.function_list.iter().enumerate() {
        let fn_name = Ident::new(&format!("shape_fn_{}", fn_id), Span::call_site());
        let default_shape = Ident::new(&format!("VSHAPE_{}", sig.default_id), Span::call_site());

        if sig.arms.is_empty() {
            shape_fns.extend(quote! {
                #[inline]
                const fn #fn_name(_offset: u16) -> VoxelShape {
                    #default_shape
                }
            });
        } else {
            let arms: Vec<TokenStream> = sig
                .arms
                .iter()
                .map(|(offsets, shape_id)| {
                    let shape_name = Ident::new(&format!("VSHAPE_{}", shape_id), Span::call_site());
                    let patterns: Vec<TokenStream> = offsets
                        .iter()
                        .map(|&o| {
                            quote! { #o }
                        })
                        .collect();
                    quote! {
                        #(#patterns)|* => #shape_name,
                    }
                })
                .collect();

            shape_fns.extend(quote! {
                #[inline]
                fn #fn_name(offset: u16) -> VoxelShape {
                    match offset {
                        #(#arms)*
                        _ => #default_shape,
                    }
                }
            });
        }
    }

    // Generate block constants with shape functions
    let mut stream = TokenStream::new();

    for (block, info) in block_assets.blocks.iter().zip(block_shape_infos.iter()) {
        let block_name = Ident::new(&block.name.to_shouty_snake_case(), Span::call_site());
        let block_name_str = block.name.clone();
        let properties = block
            .properties
            .iter()
            .map(|p| {
                let property_name = Ident::new(&p.to_shouty_snake_case(), Span::call_site());
                quote! {
                    &BlockStateProperties::#property_name
                }
            })
            .collect::<Vec<_>>();

        // Generate builder method calls for properties that differ from defaults
        let builder_calls = generate_builder_calls(&block.behavior_properties, &default_props);

        // Generate default state if block has properties
        let default_state = generate_default_state(block);

        // Shape function references (now using deduplicated function IDs)
        let collision_fn = Ident::new(
            &format!("shape_fn_{}", info.collision_fn_id),
            Span::call_site(),
        );
        let support_fn = Ident::new(
            &format!("shape_fn_{}", info.support_fn_id),
            Span::call_site(),
        );
        let outline_fn = Ident::new(
            &format!("shape_fn_{}", info.outline_fn_id),
            Span::call_site(),
        );
        let occlusion_fn = Ident::new(
            &format!("shape_fn_{}", info.occlusion_fn_id),
            Span::call_site(),
        );
        let interaction_fn = Ident::new(
            &format!("shape_fn_{}", info.interaction_fn_id),
            Span::call_site(),
        );
        let visual_fn = Ident::new(
            &format!("shape_fn_{}", info.visual_fn_id),
            Span::call_site(),
        );
        let shape_offsets = if info.collision_uses_offset
            || info.support_uses_offset
            || info.outline_uses_offset
            || info.occlusion_uses_offset
            || info.interaction_uses_offset
            || info.visual_uses_offset
        {
            let collision = info.collision_uses_offset;
            let support = info.support_uses_offset;
            let outline = info.outline_uses_offset;
            let occlusion = info.occlusion_uses_offset;
            let interaction = info.interaction_uses_offset;
            let visual = info.visual_uses_offset;
            quote! {
                .with_shape_offsets(ShapeOffsetFlags::new(
                    #collision,
                    #support,
                    #outline,
                    #occlusion,
                    #interaction,
                    #visual,
                ))
            }
        } else {
            quote! {}
        };

        stream.extend(quote! {
            pub static #block_name: Block = Block::new(
                Identifier::vanilla_static(#block_name_str),
                BlockConfig::new()#(#builder_calls)*,
                &[
                    #(#properties),*
                ],
            ).with_shapes(
                #collision_fn,
                #support_fn,
                #outline_fn,
                #occlusion_fn,
                #interaction_fn,
                #visual_fn,
            ) #shape_offsets #default_state;
        });
    }

    let mut register_stream = TokenStream::new();

    for block in &block_assets.blocks {
        let block_name = Ident::new(&block.name.to_shouty_snake_case(), Span::call_site());

        register_stream.extend(quote! {
            registry.register(&#block_name);
        });
    }

    quote! {
        use crate::{
            blocks::{
                behavior::{BlockConfig, OffsetType, PushReaction},
                shapes::ShapeOffsetFlags,
                Block, offset, BlockRegistry,
            },
            blocks::properties::{self, BlockStateProperties, NoteBlockInstrument},
            blocks::shapes::VoxelShape,
        };
        use steel_utils::{BlockLocalAabb, Identifier};

        // Block-local collision primitives.
        #(#aabb_consts)*

        // Deduplicated VoxelShapes
        #(#voxel_shape_consts)*

        // Deduplicated shape functions
        #shape_fns

        // Block constants
        #stream

        pub fn register_blocks(registry: &mut BlockRegistry) {
            #register_stream
        }
    }
}
