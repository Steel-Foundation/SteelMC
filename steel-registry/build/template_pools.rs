use std::fs;
use std::io::Read;

use proc_macro2::TokenStream;
use quote::quote;
use serde::Deserialize;
use serde_json::Value;
// ── JSON structures ──

#[derive(Deserialize, Debug)]
struct PoolJson {
    fallback: String,
    elements: Vec<WeightedElementJson>,
}

#[derive(Deserialize, Debug)]
struct WeightedElementJson {
    element: ElementJson,
    weight: i32,
}

#[derive(Deserialize, Debug)]
struct ElementJson {
    element_type: String,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    processors: Option<ProcessorsJson>,
    #[serde(default)]
    projection: Option<String>,
    #[serde(default)]
    feature: Option<String>,
    #[serde(default)]
    elements: Option<Vec<ElementJson>>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum ProcessorsJson {
    Registry(String),
    Direct { processors: Vec<Value> },
}

// ── NBT jigsaw extraction ──

/// Extracted jigsaw block data from an NBT structure template.
struct ExtractedTemplate {
    size: [i32; 3],
    jigsaws: Vec<ExtractedJigsaw>,
}

struct ExtractedJigsaw {
    pos: [i32; 3],
    orientation: String,
    name: String,
    target: String,
    pool: String,
    joint: String,
    final_state: String,
    selection_priority: i32,
    placement_priority: i32,
}

fn extract_template(path: &str) -> Result<ExtractedTemplate, String> {
    let compressed =
        fs::read(path).map_err(|e| format!("Failed to read NBT template {path}: {e}"))?;
    let mut decoder = flate2::read::GzDecoder::new(&compressed[..]);
    let mut data = Vec::new();
    decoder
        .read_to_end(&mut data)
        .map_err(|e| format!("Failed to decompress NBT template {path}: {e}"))?;

    let nbt = simdnbt::borrow::read(&mut std::io::Cursor::new(&data))
        .map_err(|e| format!("Failed to parse NBT template {path}: {e}"))?;
    let root = match nbt {
        simdnbt::borrow::Nbt::Some(base) => base,
        simdnbt::borrow::Nbt::None => return Err(format!("NBT template {path} is empty")),
    };

    let compound = root.as_compound();

    // Extract size
    let size_list = compound
        .list("size")
        .ok_or_else(|| format!("NBT template {path} is missing size"))?;
    let size_ints = size_list
        .ints()
        .ok_or_else(|| format!("NBT template {path} has non-int size list"))?;
    if size_ints.len() < 3 {
        return Err(format!(
            "NBT template {path} size list has fewer than 3 entries"
        ));
    }
    let size = [size_ints[0], size_ints[1], size_ints[2]];

    // Build palette to find jigsaw block indices.
    // Most templates use "palette" (singular), but some (shipwrecks) use "palettes"
    // (list of palettes for random block variants). Use the first palette in that case.
    let palette = if let Some(p) = compound.list("palette").and_then(|l| l.compounds()) {
        p
    } else if let Some(palettes) = compound.list("palettes").and_then(|l| l.lists()) {
        // palettes is a list of lists; each inner list is a palette (list of compounds)
        let mut iter = palettes.into_iter();
        match iter.next() {
            Some(first_palette) => match first_palette.compounds() {
                Some(c) => c,
                None => {
                    return Err(format!(
                        "NBT template {path} palettes[0] is not a compound list"
                    ));
                }
            },
            None => {
                return Err(format!("NBT template {path} has an empty palettes list"));
            }
        }
    } else {
        return Err(format!(
            "NBT template {path} is missing palette or palettes"
        ));
    };
    let palette_len = palette.len();
    let mut jigsaw_indices: Vec<(usize, String)> = Vec::new();
    for (i, entry) in palette.into_iter().enumerate() {
        let Some(name) = entry.string("Name") else {
            continue;
        };
        if name.to_str() == "minecraft:jigsaw" {
            let Some(orientation) = entry
                .compound("Properties")
                .and_then(|p| p.string("orientation"))
                .map(|s| s.to_str().to_string())
            else {
                return Err(format!(
                    "Jigsaw block state in template {path} is missing orientation"
                ));
            };
            jigsaw_indices.push((i, orientation));
        }
    }

    // Extract blocks, then collect jigsaw block entities when the palette has jigsaws.
    let blocks = compound
        .list("blocks")
        .ok_or_else(|| format!("NBT template {path} is missing blocks"))?
        .compounds()
        .ok_or_else(|| format!("NBT template {path} has non-compound blocks list"))?;

    if jigsaw_indices.is_empty() {
        return Ok(ExtractedTemplate {
            size,
            jigsaws: Vec::new(),
        });
    }

    let mut jigsaws = Vec::new();

    for block in blocks {
        let state = block
            .int("state")
            .ok_or_else(|| format!("Block in template {path} is missing state"))?;
        if state < 0 {
            return Err(format!(
                "Block in template {path} has negative state {state}"
            ));
        }
        let state = usize::try_from(state)
            .map_err(|_| format!("Block state {state} in template {path} does not fit usize"))?;
        if state >= palette_len {
            return Err(format!(
                "Block state {state} in template {path} is outside palette length {palette_len}"
            ));
        }
        let matching = jigsaw_indices.iter().find(|(idx, _)| *idx == state);
        let Some((_, orientation)) = matching else {
            continue;
        };

        let pos_list = block
            .list("pos")
            .ok_or_else(|| format!("Jigsaw block in template {path} is missing pos"))?
            .ints()
            .ok_or_else(|| format!("Jigsaw block in template {path} has non-int pos list"))?;
        if pos_list.len() < 3 {
            return Err(format!(
                "Jigsaw block in template {path} has fewer than 3 pos entries"
            ));
        }

        let nbt_data = block
            .compound("nbt")
            .ok_or_else(|| format!("Jigsaw block in template {path} is missing nbt"))?;

        let get_str = |key: &str| -> Result<String, String> {
            nbt_data
                .string(key)
                .map(|s| s.to_str().to_string())
                .ok_or_else(|| format!("Jigsaw block in template {path} is missing {key}"))
        };

        jigsaws.push(ExtractedJigsaw {
            pos: [pos_list[0], pos_list[1], pos_list[2]],
            orientation: orientation.clone(),
            name: get_str("name")?,
            target: get_str("target")?,
            pool: get_str("pool")?,
            joint: get_str("joint")?,
            final_state: get_str("final_state")?,
            selection_priority: nbt_data.int("selection_priority").unwrap_or(0),
            placement_priority: nbt_data.int("placement_priority").unwrap_or(0),
        });
    }

    // Sort jigsaws by (Y, X, Z) to match vanilla's `buildInfoList` order.
    // Vanilla re-sorts template blocks on load: full blocks, then non-full,
    // then block entities (jigsaws) — each group sorted by Y→X→Z.
    jigsaws.sort_by(|a, b| {
        a.pos[1]
            .cmp(&b.pos[1])
            .then(a.pos[0].cmp(&b.pos[0]))
            .then(a.pos[2].cmp(&b.pos[2]))
    });

    Ok(ExtractedTemplate { size, jigsaws })
}

// ── Code generation helpers ──

fn gen_identifier(id: &str) -> TokenStream {
    if id.is_empty() {
        panic!("Cannot generate an empty identifier");
    }
    if let Some((namespace, path)) = id.split_once(':') {
        quote! { Identifier::new(#namespace, #path) }
    } else {
        quote! { Identifier::vanilla(#id.to_string()) }
    }
}

fn required<T>(value: Option<T>, context: &str, field: &str) -> T {
    value.unwrap_or_else(|| panic!("Missing required field {field} in {context}"))
}

fn gen_projection(proj: &Option<String>, context: &str) -> TokenStream {
    match proj.as_deref() {
        Some("rigid") => quote! { Projection::Rigid },
        Some("terrain_matching") => quote! { Projection::TerrainMatching },
        Some(other) => panic!("Unknown projection {other} in {context}"),
        None => panic!("Missing required field projection in {context}"),
    }
}

fn gen_processors(processors: Option<&ProcessorsJson>, context: &str) -> TokenStream {
    match processors {
        Some(ProcessorsJson::Registry(id)) => {
            let id = gen_identifier(id);
            quote! { ProcessorList::Registry(#id) }
        }
        Some(ProcessorsJson::Direct { processors }) => {
            if !processors.is_empty() {
                panic!("Direct non-empty processor lists are not generated yet in {context}");
            }
            quote! { ProcessorList::Empty }
        }
        None => panic!("Missing required field processors in {context}"),
    }
}

fn gen_element(elem: &ElementJson, context: &str) -> TokenStream {
    match elem.element_type.as_str() {
        "minecraft:single_pool_element" => {
            let location = gen_identifier(required(elem.location.as_deref(), context, "location"));
            let processors = gen_processors(elem.processors.as_ref(), context);
            let projection = gen_projection(&elem.projection, context);
            quote! { PoolElement::Single { location: #location, processors: #processors, projection: #projection } }
        }
        "minecraft:legacy_single_pool_element" => {
            let location = gen_identifier(required(elem.location.as_deref(), context, "location"));
            let processors = gen_processors(elem.processors.as_ref(), context);
            let projection = gen_projection(&elem.projection, context);
            quote! { PoolElement::LegacySingle { location: #location, processors: #processors, projection: #projection } }
        }
        "minecraft:empty_pool_element" => {
            quote! { PoolElement::Empty }
        }
        "minecraft:feature_pool_element" => {
            let feature = gen_identifier(required(elem.feature.as_deref(), context, "feature"));
            let projection = gen_projection(&elem.projection, context);
            quote! { PoolElement::Feature { feature: #feature, projection: #projection } }
        }
        "minecraft:list_pool_element" => {
            let elems = required(elem.elements.as_ref(), context, "elements");
            if elems.is_empty() {
                panic!("Field elements must be non-empty in {context}");
            }
            let sub_elements: Vec<TokenStream> = elems
                .iter()
                .enumerate()
                .map(|(index, elem)| gen_element(elem, &format!("{context}.elements[{index}]")))
                .collect();
            let projection = gen_projection(&elem.projection, context);
            quote! { PoolElement::List { elements: vec![#(#sub_elements),*], projection: #projection } }
        }
        other => panic!("Unknown pool element type: {other}"),
    }
}

fn gen_orientation(s: &str) -> TokenStream {
    match s {
        "down_east" => quote! { JigsawOrientation::DownEast },
        "down_north" => quote! { JigsawOrientation::DownNorth },
        "down_south" => quote! { JigsawOrientation::DownSouth },
        "down_west" => quote! { JigsawOrientation::DownWest },
        "up_east" => quote! { JigsawOrientation::UpEast },
        "up_north" => quote! { JigsawOrientation::UpNorth },
        "up_south" => quote! { JigsawOrientation::UpSouth },
        "up_west" => quote! { JigsawOrientation::UpWest },
        "west_up" => quote! { JigsawOrientation::WestUp },
        "east_up" => quote! { JigsawOrientation::EastUp },
        "north_up" => quote! { JigsawOrientation::NorthUp },
        "south_up" => quote! { JigsawOrientation::SouthUp },
        other => panic!("Unknown jigsaw orientation: {other}"),
    }
}

fn gen_joint(s: &str) -> TokenStream {
    match s {
        "aligned" => quote! { JointType::Aligned },
        "rollable" => quote! { JointType::Rollable },
        other => panic!("Unknown jigsaw joint type: {other}"),
    }
}

// ── Main build function ──

pub(crate) fn build() -> TokenStream {
    let pool_dir = "../steel-utils/build_assets/builtin_datapacks/minecraft/worldgen/template_pool";
    let structure_dir = "../steel-utils/build_assets/builtin_datapacks/minecraft/structure";
    println!("cargo:rerun-if-changed={pool_dir}");
    println!("cargo:rerun-if-changed={structure_dir}");

    // ── Parse template pools ──

    let mut pools: Vec<(String, PoolJson)> = Vec::new();
    collect_pool_files(pool_dir, "", &mut pools);
    pools.sort_by(|a, b| a.0.cmp(&b.0));

    let mut pool_tokens = TokenStream::new();
    for (name, pool) in &pools {
        let key = gen_identifier(&format!("minecraft:{name}"));
        let fallback = gen_identifier(&pool.fallback);

        let elements: Vec<TokenStream> = pool
            .elements
            .iter()
            .enumerate()
            .map(|(index, we)| {
                if we.weight <= 0 {
                    panic!("Template pool {name} element {index} has non-positive weight");
                }
                let elem = gen_element(&we.element, &format!("{name}.elements[{index}]"));
                let weight = we.weight;
                quote! { (#elem, #weight) }
            })
            .collect();

        pool_tokens.extend(quote! {
            TemplatePoolData {
                key: #key,
                fallback: #fallback,
                elements: vec![#(#elements),*],
            },
        });
    }

    // ── Parse structure NBT files ──

    let mut templates: Vec<(String, ExtractedTemplate)> = Vec::new();
    collect_nbt_files(structure_dir, "", &mut templates);
    templates.sort_by(|a, b| a.0.cmp(&b.0));

    let mut template_tokens = TokenStream::new();
    let mut template_nbt_match_arms = TokenStream::new();
    for (name, tmpl) in &templates {
        let key = gen_identifier(&format!("minecraft:{name}"));
        let sx = tmpl.size[0];
        let sy = tmpl.size[1];
        let sz = tmpl.size[2];
        let include_path = format!(
            "../../../steel-utils/build_assets/builtin_datapacks/minecraft/structure/{name}.nbt"
        );

        let jigsaw_tokens: Vec<TokenStream> = tmpl
            .jigsaws
            .iter()
            .map(|j| {
                let px = j.pos[0];
                let py = j.pos[1];
                let pz = j.pos[2];
                let orientation = gen_orientation(&j.orientation);
                let jname = gen_identifier(&j.name);
                let target = gen_identifier(&j.target);
                let pool = gen_identifier(&j.pool);
                let joint = gen_joint(&j.joint);
                let final_state = gen_identifier(&j.final_state);
                let sel_pri = j.selection_priority;
                let plc_pri = j.placement_priority;

                quote! {
                    JigsawBlock {
                        pos: [#px, #py, #pz],
                        orientation: #orientation,
                        name: #jname,
                        target: #target,
                        pool: #pool,
                        joint: #joint,
                        final_state: #final_state,
                        selection_priority: #sel_pri,
                        placement_priority: #plc_pri,
                    }
                }
            })
            .collect();

        template_tokens.extend(quote! {
            (#key, TemplateData {
                size: [#sx, #sy, #sz],
                jigsaws: vec![#(#jigsaw_tokens),*],
            }),
        });
        template_nbt_match_arms.extend(quote! {
            #name => Some(include_bytes!(#include_path)),
        });
    }

    let pool_count = pools.len();
    let template_count = templates.len();

    quote! {
        use crate::template_pool::{
            TemplatePoolData, PoolElement, ProcessorList, Projection, TemplateData,
            JigsawBlock, JigsawOrientation, JointType,
        };
        use steel_utils::Identifier;

        /// Returns all vanilla template pools parsed from the datapack.
        pub fn vanilla_template_pools() -> Vec<TemplatePoolData> {
            vec![#pool_tokens]
        }

        /// Returns all vanilla structure templates with their jigsaw data.
        ///
        /// Each entry is (template_key, template_data).
        pub fn vanilla_templates() -> Vec<(Identifier, TemplateData)> {
            vec![#template_tokens]
        }

        /// Returns the compressed NBT bytes for a vanilla structure template.
        pub fn vanilla_template_nbt_bytes(key: &Identifier) -> Option<&'static [u8]> {
            if key.namespace != Identifier::VANILLA_NAMESPACE {
                return None;
            }

            match key.path.as_ref() {
                #template_nbt_match_arms
                _ => None,
            }
        }

        /// Number of template pools.
        pub const POOL_COUNT: usize = #pool_count;

        /// Number of structure templates.
        pub const TEMPLATE_COUNT: usize = #template_count;
    }
}

fn collect_pool_files(dir: &str, prefix: &str, out: &mut Vec<(String, PoolJson)>) {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("Failed to read template pool directory {dir}: {e}"));
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            let dir_name = path.file_name().unwrap().to_str().unwrap();
            let new_prefix = if prefix.is_empty() {
                dir_name.to_string()
            } else {
                format!("{prefix}/{dir_name}")
            };
            collect_pool_files(path.to_str().unwrap(), &new_prefix, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let file_name = path.file_stem().unwrap().to_str().unwrap();
            let full_name = if prefix.is_empty() {
                file_name.to_string()
            } else {
                format!("{prefix}/{file_name}")
            };
            let content = fs::read_to_string(&path).unwrap();
            let pool: PoolJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse template pool {full_name}: {e}"));
            out.push((full_name, pool));
        }
    }
}

fn collect_nbt_files(dir: &str, prefix: &str, out: &mut Vec<(String, ExtractedTemplate)>) {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("Failed to read structure template directory {dir}: {e}"));
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            let dir_name = path.file_name().unwrap().to_str().unwrap();
            let new_prefix = if prefix.is_empty() {
                dir_name.to_string()
            } else {
                format!("{prefix}/{dir_name}")
            };
            collect_nbt_files(path.to_str().unwrap(), &new_prefix, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("nbt") {
            let file_name = path.file_stem().unwrap().to_str().unwrap();
            let full_name = if prefix.is_empty() {
                file_name.to_string()
            } else {
                format!("{prefix}/{file_name}")
            };
            let template = extract_template(path.to_str().unwrap())
                .unwrap_or_else(|e| panic!("Failed to parse NBT template {full_name}: {e}"));
            out.push((full_name, template));
        }
    }
}
