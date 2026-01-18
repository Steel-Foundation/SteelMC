#![allow(unused)]
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
    pub destroy_time: f32,
    pub ignited_by_lava: bool,
    pub liquid: bool,
    pub is_air: bool,
    pub requires_correct_tool_for_drops: bool,
    pub instrument: Cow<'static, str>,
    pub replaceable: bool,
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
            destroy_time: 0.0,
            ignited_by_lava: false,
            liquid: false,
            is_air: false,
            requires_correct_tool_for_drops: false,
            instrument: Cow::Borrowed("HARP"),
            replaceable: false,
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
    pub default: Vec<u16>,
    pub overwrites: Vec<ShapeOverwrite>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Block {
    #[allow(dead_code)]
    pub id: u16,
    pub name: String,
    pub properties: Vec<String>,
    // example bool_true, int_5, enum_Direction_Down
    pub default_properties: Vec<String>,
    pub behavior_properties: BlockConfig,
    pub collision_shapes: ShapeData,
    pub outline_shapes: ShapeData,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Shape {
    pub min: [f32; 3],
    pub max: [f32; 3],
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
        builder_calls.push(quote! { .is_randomly_ticking(#val) });
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
        builder_calls.push(quote! { .is_air(#val) });
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
/// Maps AABB index combinations to a ShapeId.
struct VoxelShapePool {
    // Maps sorted AABB indices to ShapeId
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
        outline_fn_id: u16,
    }
    let mut block_shape_infos: Vec<BlockShapeInfo> = Vec::new();

    // First pass: collect shape data for all blocks and deduplicate functions
    for block in &block_assets.blocks {
        let (collision_default, collision_arms) =
            generate_shape_match(&block.collision_shapes, &mut voxel_pool);
        let (outline_default, outline_arms) =
            generate_shape_match(&block.outline_shapes, &mut voxel_pool);

        // Create signatures and get/insert into pool
        let collision_sig = ShapeFunctionSignature {
            default_id: collision_default,
            arms: collision_arms,
        };
        let outline_sig = ShapeFunctionSignature {
            default_id: outline_default,
            arms: outline_arms,
        };

        let collision_fn_id = shape_fn_pool.get_or_insert(collision_sig);
        let outline_fn_id = shape_fn_pool.get_or_insert(outline_sig);

        block_shape_infos.push(BlockShapeInfo {
            name: block.name.clone(),
            collision_fn_id,
            outline_fn_id,
        });
    }

    // Generate AABB constants
    let aabb_consts: Vec<TokenStream> = block_assets
        .shapes
        .iter()
        .enumerate()
        .map(|(i, shape)| {
            let name = Ident::new(&format!("AABB_{}", i), Span::call_site());
            let min_x = shape.min[0];
            let min_y = shape.min[1];
            let min_z = shape.min[2];
            let max_x = shape.max[0];
            let max_y = shape.max[1];
            let max_z = shape.max[2];
            quote! {
                static #name: AABB = AABB::new(#min_x, #min_y, #min_z, #max_x, #max_y, #max_z);
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
                    static #name: &[AABB] = &[];
                }
            } else if aabb_indices.len() == 1 && aabb_indices[0] == u16::MAX {
                quote! {
                    static #name: &[AABB] = &[AABB::FULL_BLOCK];
                }
            } else {
                let aabb_refs: Vec<TokenStream> = aabb_indices
                    .iter()
                    .map(|&idx| {
                        let aabb_name = Ident::new(&format!("AABB_{}", idx), Span::call_site());
                        quote! { #aabb_name }
                    })
                    .collect();
                quote! {
                    static #name: &[AABB] = &[#(#aabb_refs),*];
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
                const fn #fn_name(_offset: u16) -> &'static [AABB] {
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
                fn #fn_name(offset: u16) -> &'static [AABB] {
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
        let outline_fn = Ident::new(
            &format!("shape_fn_{}", info.outline_fn_id),
            Span::call_site(),
        );

        stream.extend(quote! {
            pub static #block_name: &Block = &Block::new(
                Identifier::vanilla_static(#block_name_str),
                BlockConfig::new()#(#builder_calls)*,
                &[
                    #(#properties),*
                ],
            ).with_shapes(#collision_fn, #outline_fn)#default_state;
        });
    }

    let mut register_stream = TokenStream::new();

    for block in &block_assets.blocks {
        let block_name = Ident::new(&block.name.to_shouty_snake_case(), Span::call_site());

        register_stream.extend(quote! {
            registry.register(#block_name);
        });
    }

    quote! {
        use crate::{
            blocks::{behaviour::{BlockConfig, PushReaction}, Block, offset, BlockRegistry},
            blocks::properties::{self, BlockStateProperties, NoteBlockInstrument},
            blocks::shapes::AABB,
        };
        use steel_utils::Identifier;

        // AABB primitives
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
