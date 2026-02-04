use std::{env, fs, path::Path, process::Command};

mod banner_patterns;
mod biomes;
mod block_entity_types;
mod block_tags;
mod blocks;
mod cat_variants;
mod chat_types;
mod chicken_variants;
mod cow_variants;
mod damage_types;
mod dialog_tags;
mod dialogs;
mod dimension_types;
mod entities;
mod entity_data;
mod fluid_tags;
mod fluids;

mod frog_variants;
mod game_rules;
mod instruments;
mod item_tags;
mod items;
mod jukebox_songs;
mod level_events;
mod loot_tables;
mod menu_types;
mod packets;
mod painting_variants;
mod pig_variants;
mod recipes;
mod sound_events;
mod sound_types;
mod timeline_tags;
mod timelines;
mod trim_materials;
mod trim_patterns;
mod wolf_sound_variants;
mod wolf_variants;
mod zombie_nautilus_variants;

const FMT: bool = cfg!(feature = "fmt");

const BLOCKS: &str = "blocks";
const BLOCK_TAGS: &str = "block_tags";
const ITEMS: &str = "items";
const ITEM_TAGS: &str = "item_tags";
const PACKETS: &str = "packets";
const BANNER_PATTERNS: &str = "banner_patterns";
const BIOMES: &str = "biomes";
const CHAT_TYPES: &str = "chat_types";
const TRIM_PATTERNS: &str = "trim_patterns";
const TRIM_MATERIALS: &str = "trim_materials";
const WOLF_VARIANTS: &str = "wolf_variants";
const WOLF_SOUNDS: &str = "wolf_sound_variants";
const PIG_VARIANTS: &str = "pig_variants";
const FROG_VARIANTS: &str = "frog_variants";
const CAT_VARIANTS: &str = "cat_variants";
const COW_VARIANTS: &str = "cow_variants";
const CHICKEN_VARIANTS: &str = "chicken_variants";
const PAINTING_VARIANTS: &str = "painting_variants";
const DIMENSIONS: &str = "dimension_types";
const DAMAGE_TYPES: &str = "damage_types";
const JUKEBOX_SONGS: &str = "jukebox_songs";
const INSTRUMENTS: &str = "instruments";
const DIALOGS: &str = "dialogs";
const DIALOG_TAGS: &str = "dialog_tags";
const MENU_TYPES: &str = "menu_types";
const TIMELINES: &str = "timelines";
const TIMELINE_TAGS: &str = "timeline_tags";
const ZOMBIE_NAUTILUS_VARIANTS: &str = "zombie_nautilus_variants";
const RECIPES: &str = "recipes";
const VANILLA_ENTITIES: &str = "entities";
const ENTITY_DATA: &str = "entity_data";
const FLUIDS: &str = "fluids";
const FLUID_TAGS: &str = "fluid_tags";

const LOOT_TABLES: &str = "loot_tables";
const BLOCK_ENTITY_TYPES: &str = "block_entity_types";
const GAME_RULES: &str = "game_rules";
const LEVEL_EVENTS: &str = "level_events";
const SOUND_EVENTS: &str = "sound_events";
const SOUND_TYPES: &str = "sound_types";

pub fn main() {
    // Rerun build script when any file in the build/ directory changes
    println!("cargo:rerun-if-changed=build/");

    // Use CARGO_MANIFEST_DIR to get the absolute path to the crate directory
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let out_dir = Path::new(&manifest_dir).join("src/generated");

    // Create the generated directory if it doesn't exist
    if !out_dir.exists() {
        fs::create_dir(&out_dir).unwrap();
    }

    let vanilla_builds = [
        (blocks::build(), BLOCKS),
        (block_tags::build(), BLOCK_TAGS),
        (items::build(), ITEMS),
        (item_tags::build(), ITEM_TAGS),
        (packets::build(), PACKETS),
        (banner_patterns::build(), BANNER_PATTERNS),
        (biomes::build(), BIOMES),
        (chat_types::build(), CHAT_TYPES),
        (trim_patterns::build(), TRIM_PATTERNS),
        (trim_materials::build(), TRIM_MATERIALS),
        (wolf_variants::build(), WOLF_VARIANTS),
        (wolf_sound_variants::build(), WOLF_SOUNDS),
        (pig_variants::build(), PIG_VARIANTS),
        (frog_variants::build(), FROG_VARIANTS),
        (cat_variants::build(), CAT_VARIANTS),
        (cow_variants::build(), COW_VARIANTS),
        (chicken_variants::build(), CHICKEN_VARIANTS),
        (painting_variants::build(), PAINTING_VARIANTS),
        (dimension_types::build(), DIMENSIONS),
        (damage_types::build(), DAMAGE_TYPES),
        (jukebox_songs::build(), JUKEBOX_SONGS),
        (instruments::build(), INSTRUMENTS),
        (dialogs::build(), DIALOGS),
        (dialog_tags::build(), DIALOG_TAGS),
        (menu_types::build(), MENU_TYPES),
        (timelines::build(), TIMELINES),
        (timeline_tags::build(), TIMELINE_TAGS),
        (zombie_nautilus_variants::build(), ZOMBIE_NAUTILUS_VARIANTS),
        (recipes::build(), RECIPES),
        (entities::build(), VANILLA_ENTITIES),
        (entity_data::build(), ENTITY_DATA),
        (fluids::build(), FLUIDS),
        (fluid_tags::build(), FLUID_TAGS),
        (loot_tables::build(), LOOT_TABLES),
        (block_entity_types::build(), BLOCK_ENTITY_TYPES),
        (game_rules::build(), GAME_RULES),
        (level_events::build(), LEVEL_EVENTS),
        (sound_events::build(), SOUND_EVENTS),
        (sound_types::build(), SOUND_TYPES),
    ];

    // Track which files we're generating this run
    let mut generated_files: Vec<std::path::PathBuf> = Vec::new();

    for (content, file_name) in vanilla_builds {
        let path = out_dir.join(format!("vanilla_{file_name}.rs"));
        let content = content.to_string();
        generated_files.push(path.clone());

        // Only write if content changed
        if let Ok(existing) = fs::read_to_string(&path)
            && existing == content
        {
            continue;
        }
        fs::write(&path, content).unwrap();
    }

    // Remove any stale files not generated this run
    if let Ok(entries) = fs::read_dir(&out_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !generated_files.contains(&path) {
                let _ = fs::remove_file(&path);
            }
        }
    }

    if FMT && let Ok(entries) = fs::read_dir(&out_dir) {
        for entry in entries.flatten() {
            let _ = Command::new("rustfmt").arg(entry.path()).output();
        }
    }
}
