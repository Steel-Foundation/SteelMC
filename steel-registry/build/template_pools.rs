use std::fs;
use std::io::Read;

use proc_macro2::TokenStream;
use quote::quote;
use serde::Deserialize;

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
    projection: Option<String>,
    #[serde(default)]
    feature: Option<String>,
    #[serde(default)]
    elements: Option<Vec<ElementJson>>,
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

fn extract_template(path: &str) -> Option<ExtractedTemplate> {
    let compressed = fs::read(path).ok()?;
    let mut decoder = flate2::read::GzDecoder::new(&compressed[..]);
    let mut data = Vec::new();
    decoder.read_to_end(&mut data).ok()?;

    let nbt = simdnbt::borrow::read(&mut std::io::Cursor::new(&data)).ok()?;
    let root = match nbt {
        simdnbt::borrow::Nbt::Some(base) => base,
        simdnbt::borrow::Nbt::None => return None,
    };

    let compound = root.as_compound();

    // Extract size
    let size_list = compound.list("size")?;
    let size_ints = size_list.ints()?;
    if size_ints.len() < 3 {
        return None;
    }
    let size = [size_ints[0], size_ints[1], size_ints[2]];

    // Build palette to find jigsaw block indices
    let palette = compound.list("palette")?.compounds()?;
    let mut jigsaw_indices: Vec<(usize, String)> = Vec::new();
    for (i, entry) in palette.into_iter().enumerate() {
        let Some(name) = entry.string("Name") else {
            continue;
        };
        if name.to_str() == "minecraft:jigsaw" {
            let orientation = entry
                .compound("Properties")
                .and_then(|p| p.string("orientation"))
                .map(|s| s.to_str().to_string())
                .unwrap_or_else(|| "north_up".to_string());
            jigsaw_indices.push((i, orientation));
        }
    }

    if jigsaw_indices.is_empty() {
        return Some(ExtractedTemplate {
            size,
            jigsaws: Vec::new(),
        });
    }

    // Extract jigsaw blocks
    let blocks = compound.list("blocks")?.compounds()?;
    let mut jigsaws = Vec::new();

    for block in blocks {
        let state = block.int("state")? as usize;
        let matching = jigsaw_indices.iter().find(|(idx, _)| *idx == state);
        let Some((_, orientation)) = matching else {
            continue;
        };

        let pos_list = block.list("pos")?.ints()?;
        if pos_list.len() < 3 {
            continue;
        }

        let nbt_data = match block.compound("nbt") {
            Some(c) => c,
            None => continue,
        };

        let get_str =
            |key: &str| -> String { nbt_data.string(key).map(|s| s.to_str().to_string()).unwrap_or_default() };

        jigsaws.push(ExtractedJigsaw {
            pos: [pos_list[0], pos_list[1], pos_list[2]],
            orientation: orientation.clone(),
            name: get_str("name"),
            target: get_str("target"),
            pool: get_str("pool"),
            joint: get_str("joint"),
            final_state: get_str("final_state"),
            selection_priority: nbt_data.int("selection_priority").unwrap_or(0),
            placement_priority: nbt_data.int("placement_priority").unwrap_or(0),
        });
    }

    Some(ExtractedTemplate { size, jigsaws })
}

// ── Code generation helpers ──

fn gen_identifier(id: &str) -> TokenStream {
    if let Some((namespace, path)) = id.split_once(':') {
        quote! { Identifier::new(#namespace, #path) }
    } else {
        quote! { Identifier::vanilla(#id.to_string()) }
    }
}

fn gen_projection(proj: &Option<String>) -> TokenStream {
    match proj.as_deref() {
        Some("terrain_matching") => quote! { Projection::TerrainMatching },
        _ => quote! { Projection::Rigid },
    }
}

fn gen_element(elem: &ElementJson) -> TokenStream {
    match elem.element_type.as_str() {
        "minecraft:single_pool_element" => {
            let location = gen_identifier(elem.location.as_deref().unwrap_or(""));
            let projection = gen_projection(&elem.projection);
            quote! { PoolElement::Single { location: #location, projection: #projection } }
        }
        "minecraft:legacy_single_pool_element" => {
            let location = gen_identifier(elem.location.as_deref().unwrap_or(""));
            let projection = gen_projection(&elem.projection);
            quote! { PoolElement::LegacySingle { location: #location, projection: #projection } }
        }
        "minecraft:empty_pool_element" => {
            quote! { PoolElement::Empty }
        }
        "minecraft:feature_pool_element" => {
            let feature = gen_identifier(elem.feature.as_deref().unwrap_or(""));
            let projection = gen_projection(&elem.projection);
            quote! { PoolElement::Feature { feature: #feature, projection: #projection } }
        }
        "minecraft:list_pool_element" => {
            let sub_elements: Vec<TokenStream> = elem
                .elements
                .as_ref()
                .map(|elems| elems.iter().map(gen_element).collect())
                .unwrap_or_default();
            let projection = gen_projection(&elem.projection);
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
        _ => quote! { JointType::Rollable },
    }
}

// ── Main build function ──

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/worldgen/template_pool/");
    println!("cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/structure/");

    let pool_dir =
        "build_assets/builtin_datapacks/minecraft/data/minecraft/worldgen/template_pool";
    let structure_dir =
        "build_assets/builtin_datapacks/minecraft/data/minecraft/structure";

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
            .map(|we| {
                let elem = gen_element(&we.element);
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
    for (name, tmpl) in &templates {
        let key = gen_identifier(&format!("minecraft:{name}"));
        let sx = tmpl.size[0];
        let sy = tmpl.size[1];
        let sz = tmpl.size[2];

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
    }

    let pool_count = pools.len();
    let template_count = templates.len();

    quote! {
        use crate::template_pool::{
            TemplatePoolData, PoolElement, Projection, TemplateData,
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

        /// Number of template pools.
        pub const POOL_COUNT: usize = #pool_count;

        /// Number of structure templates.
        pub const TEMPLATE_COUNT: usize = #template_count;
    }
}

fn collect_pool_files(dir: &str, prefix: &str, out: &mut Vec<(String, PoolJson)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
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
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
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
            if let Some(template) = extract_template(path.to_str().unwrap()) {
                out.push((full_name, template));
            } else {
                eprintln!("cargo:warning=Failed to parse NBT template: {full_name}");
            }
        }
    }
}
