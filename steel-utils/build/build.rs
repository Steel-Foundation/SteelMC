//! Build script for steel-utils that generates translation constants.

use reqwest::blocking;
use serde::Deserialize;
use std::{
    env, fs,
    io::{Cursor, Read, copy},
    path::{Path, PathBuf},
    process::Command,
};

use text_components::build::build_translations;

mod entity_events;
mod translations;

const FMT: bool = cfg!(feature = "fmt");

const OUT_DIR: &str = "src/generated";
const IDS: &str = "vanilla_translations/ids";
const REGISTRY: &str = "vanilla_translations/registry";
const ENTITY_EVENTS: &str = "entity_events";

#[derive(Deserialize)]
struct VersionManifest {
    versions: Vec<VersionEntry>,
}

#[derive(Deserialize)]
struct VersionEntry {
    id: String,
    url: String,
}

#[derive(Deserialize)]
struct VersionDetails {
    downloads: Downloads,
}

#[derive(Deserialize)]
struct Downloads {
    server: DownloadEntry,
}

#[derive(Deserialize)]
struct DownloadEntry {
    url: String,
}

fn get_target_mc_version() -> String {
    let pkg_version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.9.0+mc26.1".to_string());
    if let Some(pos) = pkg_version.find("+mc") {
        pkg_version[pos + 3..].to_string()
    } else {
        panic!("CARGO_PKG_VERSION does not contain +mc suffix: {pkg_version}");
    }
}

fn fetch_version_manifest() -> VersionManifest {
    let manifest_url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
    blocking::get(manifest_url)
        .unwrap_or_else(|e| panic!("Failed to fetch version manifest from {manifest_url}: {e}"))
        .json::<VersionManifest>()
        .expect("Failed to parse version manifest JSON")
}

fn fetch_version_details(version_url: &str, target_ver: &str) -> VersionDetails {
    blocking::get(version_url)
        .unwrap_or_else(|e| panic!("Failed to fetch version details for {target_ver}: {e}"))
        .json::<VersionDetails>()
        .expect("Failed to parse version details JSON")
}

fn download_server_jar(server_jar_url: &str, cached_jar: &Path, target_ver: &str) {
    if cached_jar.exists() {
        return;
    }
    println!("cargo:warning=Downloading server jar for {target_ver}...");
    let mut jar_resp = blocking::get(server_jar_url)
        .unwrap_or_else(|e| panic!("Failed to download server jar from {server_jar_url}: {e}"));
    let mut jar_file = fs::File::create(cached_jar).unwrap_or_else(|e| {
        panic!(
            "Failed to create cached jar file at {}: {e}",
            cached_jar.display()
        )
    });
    copy(&mut jar_resp, &mut jar_file).expect("Failed to write server jar contents to file");
}

fn download_server_jar_for_version(manifest_dir: &str, target_ver: &str) -> PathBuf {
    let build_assets = Path::new(manifest_dir).join("build_assets");
    let cache_dir = build_assets.join(".cache");
    if !cache_dir.exists() {
        fs::create_dir_all(&cache_dir).expect("Failed to create cache directory");
    }
    let cached_jar = cache_dir.join(format!("server-{target_ver}.jar"));
    if cached_jar.exists() {
        return cached_jar;
    }

    let manifest = fetch_version_manifest();
    let version_entry = manifest
        .versions
        .iter()
        .find(|v| v.id == target_ver)
        .unwrap_or_else(|| panic!("Minecraft version {target_ver} not found in version manifest"));

    let details = fetch_version_details(&version_entry.url, target_ver);
    download_server_jar(&details.downloads.server.url, &cached_jar, target_ver);
    cached_jar
}

fn get_server_archive(cached_jar: &Path) -> zip::ZipArchive<Cursor<Vec<u8>>> {
    let mut jar_data = Vec::new();
    {
        let mut jar_file = fs::File::open(cached_jar).expect("Failed to open server jar");
        jar_file
            .read_to_end(&mut jar_data)
            .expect("Failed to read server jar file");
    }
    let outer_cursor = Cursor::new(jar_data);
    let mut outer_archive =
        zip::ZipArchive::new(outer_cursor).expect("Failed to read server jar ZIP");

    let mut nested_entry_name = None;
    for i in 0..outer_archive.len() {
        if let Ok(file) = outer_archive.by_index(i) {
            let name = file.name();
            if name.starts_with("META-INF/versions/")
                && Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"))
            {
                nested_entry_name = Some(name.to_string());
                break;
            }
        }
    }

    if let Some(entry_name) = nested_entry_name {
        println!("cargo:warning=Detected bootstrap jar. Extracting nested jar {entry_name}...");
        let mut nested_file = outer_archive
            .by_name(&entry_name)
            .expect("Failed to locate nested jar");
        let mut nested_data = Vec::new();
        nested_file
            .read_to_end(&mut nested_data)
            .expect("Failed to read nested jar");
        let cursor = Cursor::new(nested_data);
        zip::ZipArchive::new(cursor).expect("Failed to read nested server jar ZIP")
    } else {
        outer_archive
    }
}

fn download_and_extract_assets(manifest_dir: &str) {
    let target_ver = get_target_mc_version();
    let build_assets = Path::new(manifest_dir).join("build_assets");
    let datapack_base = build_assets.join("builtin_datapacks");
    let datapack_dir = datapack_base.join("minecraft");
    let version_file = datapack_dir.join(".version");
    let en_us_dest = build_assets.join("en_us.json");

    let is_valid = version_file.exists()
        && en_us_dest.exists()
        && fs::read_to_string(&version_file).is_ok_and(|v| v.trim() == target_ver);

    if is_valid {
        return;
    }

    println!(
        "cargo:warning=Assets not found or version mismatch for Minecraft {target_ver}. Fetching..."
    );

    let cached_jar = download_server_jar_for_version(manifest_dir, &target_ver);

    if datapack_dir.exists() {
        fs::remove_dir_all(&datapack_dir).expect("Failed to clear old datapack directory");
    }
    fs::create_dir_all(&datapack_dir).expect("Failed to create datapack directory");

    if let Some(parent) = en_us_dest.parent() {
        fs::create_dir_all(parent).expect("Failed to create parent directory for en_us.json");
    }

    let mut archive = get_server_archive(&cached_jar);
    let mut extracted_en_us = false;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .expect("Failed to read file in server jar");
        let name = file.name();

        if name.starts_with("data/minecraft/") {
            let rel_path = name
                .strip_prefix("data/")
                .expect("Name must start with data/");
            let dest_path = datapack_base.join(rel_path);

            if file.is_dir() {
                fs::create_dir_all(&dest_path).expect("Failed to create directory");
            } else {
                if let Some(parent) = dest_path.parent() {
                    fs::create_dir_all(parent).expect("Failed to create parent directory");
                }
                let mut out_file = fs::File::create(&dest_path).unwrap_or_else(|e| {
                    panic!("Failed to create file {}: {e}", dest_path.display())
                });
                copy(&mut file, &mut out_file).expect("Failed to extract file");
            }
        } else if name == "assets/minecraft/lang/en_us.json" {
            let mut out_file = fs::File::create(&en_us_dest)
                .unwrap_or_else(|e| panic!("Failed to create file {}: {e}", en_us_dest.display()));
            copy(&mut file, &mut out_file).expect("Failed to extract en_us.json");
            extracted_en_us = true;
        }
    }

    assert!(
        extracted_en_us,
        "Failed to find assets/minecraft/lang/en_us.json in server jar"
    );

    fs::write(&version_file, &target_ver).expect("Failed to write version file");
    println!(
        "cargo:warning=Successfully extracted datapack and translation files for Minecraft {target_ver}."
    );
}

/// Main build script entry point that generates translation constants.
pub fn main() {
    println!("cargo:rerun-if-changed=build/");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");

    // Download and extract datapack and translation assets under steel-utils
    download_and_extract_assets(&manifest_dir);

    if !Path::new(&format!("{OUT_DIR}/vanilla_translations")).exists() {
        fs::create_dir_all(format!("{OUT_DIR}/vanilla_translations"))
            .expect("Failed to create output directory");
    }

    let content = build_translations("build_assets/en_us.json");
    write_if_changed(format!("{OUT_DIR}/{IDS}.rs"), content.to_string());

    let content = translations::build();
    write_if_changed(format!("{OUT_DIR}/{REGISTRY}.rs"), content.to_string());

    let content = entity_events::build();
    write_if_changed(format!("{OUT_DIR}/{ENTITY_EVENTS}.rs"), content.to_string());

    if FMT && let Ok(entries) = fs::read_dir(OUT_DIR) {
        for entry in entries.flatten() {
            let _ = Command::new("rustfmt").arg(entry.path()).output();
        }
    }
}

fn write_if_changed(path: impl AsRef<Path>, content: String) {
    let path = path.as_ref();
    if let Ok(existing) = fs::read_to_string(path)
        && existing == content
    {
        return;
    }

    if let Err(error) = fs::write(path, content) {
        panic!("Failed to write {}: {error}", path.display());
    }
}
