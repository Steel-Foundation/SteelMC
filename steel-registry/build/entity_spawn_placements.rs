use std::{
    collections::{BTreeMap, btree_map},
    fs,
};

use proc_macro2::TokenStream;
use quote::quote;
use serde::Deserialize;

const PATH: &str = "build_assets/entity_spawn_placements.json";

#[derive(Deserialize)]
struct Asset {
    minecraft_version: String,
    placements: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    entity_type: String,
    placement_type: String,
    heightmap: String,
}

pub(super) fn read() -> BTreeMap<String, TokenStream> {
    println!("cargo:rerun-if-changed={PATH}");
    let json = fs::read_to_string(PATH)
        .unwrap_or_else(|error| panic!("Failed to read {PATH}. Run SteelExtractor: {error}"));
    let asset: Asset = serde_json::from_str(&json)
        .unwrap_or_else(|error| panic!("Failed to parse {PATH}: {error}"));
    let package_version = env!("CARGO_PKG_VERSION");
    let (_, minecraft_version) = package_version
        .split_once("+mc")
        .unwrap_or_else(|| panic!("Package version {package_version} is missing its +mc suffix"));

    assert_eq!(
        asset.minecraft_version, minecraft_version,
        "{PATH} must be extracted from Minecraft {minecraft_version}"
    );
    assert!(
        !asset.placements.is_empty(),
        "No spawn placements in {PATH}"
    );

    let mut placements = BTreeMap::new();
    for entry in asset.placements {
        let kind = match entry.placement_type.as_str() {
            "on_ground" => quote! { SpawnPlacementKind::OnGround },
            "in_water" => quote! { SpawnPlacementKind::InWater },
            "in_lava" => quote! { SpawnPlacementKind::InLava },
            "no_restrictions" => quote! { SpawnPlacementKind::NoRestrictions },
            other => panic!("Unknown spawn placement {other} for {}", entry.entity_type),
        };

        let heightmap = match entry.heightmap.as_str() {
            "motion_blocking" => quote! { SpawnPlacementHeightmap::MotionBlocking },
            "motion_blocking_no_leaves" => {
                quote! { SpawnPlacementHeightmap::MotionBlockingNoLeaves }
            }
            other => panic!("Unknown spawn heightmap {other} for {}", entry.entity_type),
        };

        match placements.entry(entry.entity_type) {
            btree_map::Entry::Vacant(slot) => {
                slot.insert(quote! { SpawnPlacement { kind: #kind, heightmap: #heightmap } });
            }

            btree_map::Entry::Occupied(slot) => {
                panic!(
                    "Duplicate entity spawn placement for {} in {PATH}",
                    slot.key()
                );
            }
        }
    }
    placements
}
