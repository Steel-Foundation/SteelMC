//! Mod discovery in directories, archives (.jar/.zip), or direct dynamic libraries.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use tracing::{debug, warn};
use zip::ZipArchive;

use crate::manifest::ModManifest;

/// Source location type of a discovered mod.
#[derive(Debug, Clone)]
pub enum ModSource {
    /// Mod resides in an unpacked directory.
    Directory(PathBuf),
    /// Mod resides inside a ZIP / JAR archive.
    Archive(PathBuf),
    /// Mod resides in a compiled shared library file (.so, .dylib, .dll).
    SharedLibrary(PathBuf),
}

/// A mod discovered on disk before loading.
#[derive(Debug, Clone)]
pub struct DiscoveredMod {
    /// Parsed manifest.
    pub manifest: ModManifest,
    /// Origin source location.
    pub source: ModSource,
}

/// Mod discovery utility.
pub struct ModDiscovery;

impl ModDiscovery {
    /// Scans a directory for mods (.jar, .zip, .so, .dylib, .dll or subdirectories).
    #[must_use]
    pub fn discover_all(mods_dir: &Path) -> Vec<DiscoveredMod> {
        let mut discovered = Vec::new();

        if !mods_dir.exists() {
            if let Err(err) = fs::create_dir_all(mods_dir) {
                warn!("Failed to create mods directory at {:?}: {err}", mods_dir);
            }
            return discovered;
        }

        let read_dir = match fs::read_dir(mods_dir) {
            Ok(rd) => rd,
            Err(err) => {
                warn!("Failed to read mods directory at {:?}: {err}", mods_dir);
                return discovered;
            }
        };

        for entry in read_dir.flatten() {
            let path = entry.path();

            if path.is_dir() {
                if let Some(m) = Self::discover_from_dir(&path) {
                    discovered.push(m);
                }
            } else if path.is_file() {
                let ext = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default();
                match ext {
                    "jar" | "zip" => {
                        if let Some(m) = Self::discover_from_archive(&path) {
                            discovered.push(m);
                        }
                    }
                    "so" | "dylib" | "dll" => {
                        if let Some(m) = Self::discover_from_dylib(&path) {
                            discovered.push(m);
                        }
                    }
                    _ => {}
                }
            }
        }

        discovered
    }

    /// Discovers a mod from an unpacked directory containing `steel.mod.json`.
    #[must_use]
    pub fn discover_from_dir(dir: &Path) -> Option<DiscoveredMod> {
        let manifest_candidates = ["steel.mod.json"];
        for candidate in manifest_candidates {
            let manifest_path = dir.join(candidate);
            if manifest_path.is_file() {
                if let Ok(content) = fs::read_to_string(&manifest_path) {
                    if let Ok(manifest) = ModManifest::parse_json(&content) {
                        debug!("Discovered mod '{}' from directory {:?}", manifest.id, dir);
                        return Some(DiscoveredMod {
                            manifest,
                            source: ModSource::Directory(dir.to_path_buf()),
                        });
                    }
                }
            }
        }
        None
    }

    /// Discovers a mod inside a ZIP/JAR archive containing `steel.mod.json`.
    #[must_use]
    pub fn discover_from_archive(archive_path: &Path) -> Option<DiscoveredMod> {
        let file = File::open(archive_path).ok()?;
        let mut archive = ZipArchive::new(file).ok()?;

        let manifest_candidates = ["steel.mod.json"];
        for candidate in manifest_candidates {
            if let Ok(mut zip_file) = archive.by_name(candidate) {
                let mut content = String::new();
                if zip_file.read_to_string(&mut content).is_ok() {
                    if let Ok(manifest) = ModManifest::parse_json(&content) {
                        debug!(
                            "Discovered mod '{}' from archive {:?}",
                            manifest.id, archive_path
                        );
                        return Some(DiscoveredMod {
                            manifest,
                            source: ModSource::Archive(archive_path.to_path_buf()),
                        });
                    }
                }
            }
        }
        None
    }

    /// Discovers a mod from a bare dynamic library (.so, .dylib, .dll).
    /// Generates a synthetic minimal manifest if no explicit json is provided.
    #[must_use]
    pub fn discover_from_dylib(dylib_path: &Path) -> Option<DiscoveredMod> {
        let stem = dylib_path.file_stem()?.to_str()?;
        let mod_id = stem.strip_prefix("lib").unwrap_or(stem).to_lowercase();

        let json = format!(
            r#"{{
                "schemaVersion": 1,
                "id": "{mod_id}",
                "version": "1.0.0",
                "entrypoints": {{
                    "main": ["{stem}_init"]
                }}
            }}"#
        );

        let manifest = ModManifest::parse_json(&json).ok()?;
        Some(DiscoveredMod {
            manifest,
            source: ModSource::SharedLibrary(dylib_path.to_path_buf()),
        })
    }
}
