//! Build script for steel-utils that generates translation constants.

use reqwest::blocking::{self, Response};
use serde::Deserialize;
use sha1::{Digest, Sha1};
use std::{
    env,
    fmt::Write as _,
    fs,
    fs::OpenOptions,
    io::{Cursor, ErrorKind, Read, copy},
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use text_components::build::build_translations;

mod entity_events;
mod translations;

const FMT: bool = cfg!(feature = "fmt");

const OUT_DIR: &str = "src/generated";
const IDS: &str = "vanilla_translations/ids";
const REGISTRY: &str = "vanilla_translations/registry";
const ENTITY_EVENTS: &str = "entity_events";
const ASSET_LOCK_TIMEOUT: Duration = Duration::from_secs(300);

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

struct AssetLock {
    path: PathBuf,
}

impl Drop for AssetLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn try_acquire_asset_lock(path: &Path) -> Result<Option<AssetLock>, String> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(_) => Ok(Some(AssetLock {
            path: path.to_path_buf(),
        })),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => Ok(None),
        Err(err) => Err(format!(
            "Failed to create asset extraction lock {}: {err}",
            path.display()
        )),
    }
}

fn assets_are_valid(version_file: &Path, en_us_dest: &Path, target_ver: &str) -> bool {
    version_file.exists()
        && en_us_dest.exists()
        && fs::read_to_string(version_file).is_ok_and(|v| v.trim() == target_ver)
}

fn acquire_asset_lock(
    lock_file: &Path,
    version_file: &Path,
    en_us_dest: &Path,
    target_ver: &str,
) -> Option<AssetLock> {
    let start = Instant::now();
    loop {
        match try_acquire_asset_lock(lock_file) {
            Ok(Some(lock)) => return Some(lock),
            Ok(None) => {
                if assets_are_valid(version_file, en_us_dest, target_ver) {
                    return None;
                }
                assert!(
                    start.elapsed() <= ASSET_LOCK_TIMEOUT,
                    "Timed out waiting for asset extraction lock {}",
                    lock_file.display()
                );
                thread::sleep(Duration::from_millis(250));
            }
            Err(err) => panic!("{err}"),
        }
    }
}

fn sha1_hex(data: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(data);
    let mut out = String::with_capacity(40);
    for byte in hasher.finalize() {
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn validate_downloaded_server_jar(
    data: &[u8],
    download: &DownloadEntry,
    target_ver: &str,
) -> Result<(), String> {
    let actual_size =
        u64::try_from(data.len()).expect("Downloaded server jar is too large to validate");
    if actual_size != download.size {
        return Err(format!(
            "Downloaded server jar for {target_ver} has size {actual_size}, expected {}",
            download.size
        ));
    }

    let actual_sha1 = sha1_hex(data);
    if !actual_sha1.eq_ignore_ascii_case(&download.sha1) {
        return Err(format!(
            "Downloaded server jar for {target_ver} has SHA-1 {actual_sha1}, expected {}",
            download.sha1
        ));
    }

    Ok(())
}

fn download_server_jar(download: &DownloadEntry, target_ver: &str) -> Vec<u8> {
    println!("cargo:warning=Downloading server jar for {target_ver}...");
    let mut jar_resp = blocking::get(&download.url)
        .and_then(Response::error_for_status)
        .unwrap_or_else(|e| panic!("Failed to download server jar from {}: {e}", download.url));

    let mut jar_data = Vec::new();
    jar_resp
        .read_to_end(&mut jar_data)
        .expect("Failed to read server jar response");
    if let Err(err) = validate_downloaded_server_jar(&jar_data, download, target_ver) {
        panic!("{err}");
    }
    jar_data
}

fn download_server_jar_for_version(target_ver: &str) -> Vec<u8> {
    let manifest = fetch_version_manifest();
    let version_entry = manifest
        .versions
        .iter()
        .find(|v| v.id == target_ver)
        .unwrap_or_else(|| panic!("Minecraft version {target_ver} not found in version manifest"));

    let details = fetch_version_details(&version_entry.url, target_ver);
    download_server_jar(&details.downloads.server, target_ver)
}

fn get_server_archive(jar_data: Vec<u8>) -> zip::ZipArchive<Cursor<Vec<u8>>> {
    let outer_cursor = Cursor::new(jar_data);
    let mut outer_archive = zip::ZipArchive::new(outer_cursor)
        .unwrap_or_else(|e| panic!("Failed to read server jar ZIP: {e}"));

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
            .unwrap_or_else(|e| panic!("Failed to locate nested jar {entry_name}: {e}"));
        let mut nested_data = Vec::new();
        nested_file
            .read_to_end(&mut nested_data)
            .unwrap_or_else(|e| panic!("Failed to read nested jar {entry_name}: {e}"));
        let cursor = Cursor::new(nested_data);
        zip::ZipArchive::new(cursor)
            .unwrap_or_else(|e| panic!("Failed to read nested server jar ZIP {entry_name}: {e}"))
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
    let lock_file = build_assets.join(".asset-extract.lock");

    if assets_are_valid(&version_file, &en_us_dest, &target_ver) {
        return;
    }

    let Some(_lock) = acquire_asset_lock(&lock_file, &version_file, &en_us_dest, &target_ver)
    else {
        return;
    };
    if assets_are_valid(&version_file, &en_us_dest, &target_ver) {
        return;
    }

    println!(
        "cargo:warning=Assets not found or version mismatch for Minecraft {target_ver}. Fetching..."
    );

    let jar_data = download_server_jar_for_version(&target_ver);

    if datapack_dir.exists() {
        fs::remove_dir_all(&datapack_dir).expect("Failed to clear old datapack directory");
    }
    fs::create_dir_all(&datapack_dir).expect("Failed to create datapack directory");

    if let Some(parent) = en_us_dest.parent() {
        fs::create_dir_all(parent).expect("Failed to create parent directory for en_us.json");
    }

    let mut archive = get_server_archive(jar_data);
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
