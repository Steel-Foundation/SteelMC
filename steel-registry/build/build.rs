use std::{fs, path::Path, process::Command};

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
mod entity_data_serializers;
mod frog_variants;
mod game_rules;
mod instruments;
mod item_tags;
mod items;
mod jukebox_songs;
mod loot_tables;
mod menu_types;
mod packets;
mod painting_variants;
mod pig_variants;
mod recipes;
mod timeline_tags;
mod timelines;
mod trim_materials;
mod trim_patterns;
mod wolf_sound_variants;
mod wolf_variants;
mod zombie_nautilus_variants;

const FMT: bool = cfg!(feature = "fmt");

const OUT_DIR: &str = "src/generated";

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
const ENTITY_DATA_SERIALIZERS: &str = "entity_data_serializers";
const LOOT_TABLES: &str = "loot_tables";
const BLOCK_ENTITY_TYPES: &str = "block_entity_types";
const GAME_RULES: &str = "game_rules";

pub fn main() {
    // Rerun build script when any file in the build/ directory changes
    println!("cargo:rerun-if-changed=build/");

    if !Path::new(OUT_DIR).exists() {
        fs::create_dir(OUT_DIR).unwrap();
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
        (entity_data_serializers::build(), ENTITY_DATA_SERIALIZERS),
        (loot_tables::build(), LOOT_TABLES),
        (block_entity_types::build(), BLOCK_ENTITY_TYPES),
        (game_rules::build(), GAME_RULES),
    ];

    for (content, file_name) in vanilla_builds {
        fs::write(
            format!("{OUT_DIR}/vanilla_{file_name}.rs"),
            content.to_string(),
        )
        .unwrap();
    }

    if FMT && let Ok(entries) = fs::read_dir(OUT_DIR) {
        for entry in entries.flatten() {
            let _ = Command::new("rustfmt").arg(entry.path()).output();
        }
    }
}
