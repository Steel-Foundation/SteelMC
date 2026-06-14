//! Build script for steel-utils that generates translation constants.

use reqwest::blocking::{self, Response};
use serde::Deserialize;
use sha1::{Digest, Sha1};
use std::{
    env,
    fmt::Write as _,
    fs,
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
    sha1: String,
    size: u64,
    url: String,
}

fn get_target_mc_version() -> String {
    let pkg_version = env::var("CARGO_PKG_VERSION")
        .expect("Something is wrong with your env, can't find the var CARGO_PKG_VERSION");
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

fn sha1_hex(path: &Path) -> Result<String, String> {
    let mut file =
        fs::File::open(path).map_err(|e| format!("Failed to open {}: {e}", path.display()))?;
    let mut hasher = Sha1::new();
    let mut buffer = [0; 8192];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let mut out = String::with_capacity(40);
    for byte in hasher.finalize() {
        let _ = write!(&mut out, "{byte:02x}");
    }
    Ok(out)
}

fn validate_downloaded_server_jar(
    path: &Path,
    download: &DownloadEntry,
    target_ver: &str,
) -> Result<(), String> {
    let actual_size = fs::metadata(path)
        .map_err(|e| format!("Failed to read metadata for {}: {e}", path.display()))?
        .len();
    if actual_size != download.size {
        return Err(format!(
            "Downloaded server jar for {target_ver} has size {actual_size}, expected {}",
            download.size
        ));
    }

    let actual_sha1 = sha1_hex(path)?;
    if !actual_sha1.eq_ignore_ascii_case(&download.sha1) {
        return Err(format!(
            "Downloaded server jar for {target_ver} has SHA-1 {actual_sha1}, expected {}",
            download.sha1
        ));
    }

    try_get_server_archive(path)?;
    Ok(())
}

fn download_server_jar(download: &DownloadEntry, cached_jar: &Path, target_ver: &str) {
    println!("cargo:warning=Downloading server jar for {target_ver}...");
    let mut jar_resp = blocking::get(&download.url)
        .and_then(Response::error_for_status)
        .unwrap_or_else(|e| panic!("Failed to download server jar from {}: {e}", download.url));

    let tmp_jar = cached_jar.with_extension("jar.tmp");
    if tmp_jar.exists() {
        fs::remove_file(&tmp_jar).unwrap_or_else(|e| {
            panic!(
                "Failed to remove temporary jar file at {}: {e}",
                tmp_jar.display()
            )
        });
    }

    let mut jar_file = fs::File::create(&tmp_jar).unwrap_or_else(|e| {
        panic!(
            "Failed to create temporary jar file at {}: {e}",
            tmp_jar.display()
        )
    });
    copy(&mut jar_resp, &mut jar_file).expect("Failed to write server jar contents to file");
    jar_file
        .sync_all()
        .expect("Failed to flush server jar contents to disk");

    if let Err(err) = validate_downloaded_server_jar(&tmp_jar, download, target_ver) {
        let _ = fs::remove_file(&tmp_jar);
        panic!("{err}");
    }

    fs::rename(&tmp_jar, cached_jar).unwrap_or_else(|e| {
        panic!(
            "Failed to move downloaded server jar from {} to {}: {e}",
            tmp_jar.display(),
            cached_jar.display()
        )
    });
}

fn download_server_jar_for_version(manifest_dir: &str, target_ver: &str) -> PathBuf {
    let build_assets = Path::new(manifest_dir).join("build_assets");
    let cache_dir = build_assets.join(".cache");
    if !cache_dir.exists() {
        fs::create_dir_all(&cache_dir).expect("Failed to create cache directory");
    }
    let cached_jar = cache_dir.join(format!("server-{target_ver}.jar"));
    if cached_jar.exists() {
        match try_get_server_archive(&cached_jar) {
            Ok(_) => return cached_jar,
            Err(err) => {
                println!(
                    "cargo:warning=Cached server jar for {target_ver} is invalid ({err}). Re-downloading..."
                );
                fs::remove_file(&cached_jar).unwrap_or_else(|e| {
                    panic!(
                        "Failed to remove invalid cached jar file at {}: {e}",
                        cached_jar.display()
                    )
                });
            }
        }
    }

    let manifest = fetch_version_manifest();
    let version_entry = manifest
        .versions
        .iter()
        .find(|v| v.id == target_ver)
        .unwrap_or_else(|| panic!("Minecraft version {target_ver} not found in version manifest"));

    let details = fetch_version_details(&version_entry.url, target_ver);
    download_server_jar(&details.downloads.server, &cached_jar, target_ver);
    cached_jar
}

fn try_get_server_archive(cached_jar: &Path) -> Result<zip::ZipArchive<Cursor<Vec<u8>>>, String> {
    let mut jar_data = Vec::new();
    {
        let mut jar_file = fs::File::open(cached_jar)
            .map_err(|e| format!("Failed to open server jar {}: {e}", cached_jar.display()))?;
        jar_file.read_to_end(&mut jar_data).map_err(|e| {
            format!(
                "Failed to read server jar file {}: {e}",
                cached_jar.display()
            )
        })?;
    }
    let outer_cursor = Cursor::new(jar_data);
    let mut outer_archive = zip::ZipArchive::new(outer_cursor)
        .map_err(|e| format!("Failed to read server jar ZIP: {e}"))?;

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
            .map_err(|e| format!("Failed to locate nested jar {entry_name}: {e}"))?;
        let mut nested_data = Vec::new();
        nested_file
            .read_to_end(&mut nested_data)
            .map_err(|e| format!("Failed to read nested jar {entry_name}: {e}"))?;
        let cursor = Cursor::new(nested_data);
        zip::ZipArchive::new(cursor)
            .map_err(|e| format!("Failed to read nested server jar ZIP {entry_name}: {e}"))
    } else {
        Ok(outer_archive)
    }
}

fn get_server_archive(cached_jar: &Path) -> zip::ZipArchive<Cursor<Vec<u8>>> {
    try_get_server_archive(cached_jar).unwrap_or_else(|e| panic!("{e}"))
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
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=../Cargo.toml");

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
