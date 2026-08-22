//! Dynamic library loading and lifecycle management for Steel mods.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use libloading::{Library, Symbol};
use thiserror::Error;
use tracing::{info, warn};

use crate::api::{ModContext, SteelModInitFn, SteelModShutdownFn};
use crate::discovery::{DiscoveredMod, ModSource};
use crate::manifest::ModManifest;

/// Errors occurring during dynamic mod loading and initialization.
#[derive(Debug, Error)]
pub enum ModLoaderError {
    /// Library file not found or inaccessible.
    #[error("Failed to find shared library for mod '{mod_id}' at path '{path:?}'")]
    LibraryNotFound {
        /// Mod ID.
        mod_id: String,
        /// Checked path.
        path: PathBuf,
    },

    /// Dynamic library loading failed.
    #[error("Failed to load shared library for mod '{mod_id}' from '{path:?}': {error}")]
    LoadFailed {
        /// Mod ID.
        mod_id: String,
        /// Library path.
        path: PathBuf,
        /// Underlying libloading error message.
        error: String,
    },

    /// Entrypoint symbol not found in library.
    #[error("Symbol '{symbol}' not found in mod library '{mod_id}': {error}")]
    SymbolNotFound {
        /// Mod ID.
        mod_id: String,
        /// Symbol name.
        symbol: String,
        /// Underlying libloading error message.
        error: String,
    },

    /// Mod initialization entrypoint returned failure code.
    #[error("Mod '{mod_id}' entrypoint '{symbol}' returned failure code: {code}")]
    InitFailed {
        /// Mod ID.
        mod_id: String,
        /// Symbol name.
        symbol: String,
        /// Error code returned.
        code: i32,
    },
}

/// Instance of a loaded mod in memory.
pub struct LoadedMod {
    /// Mod metadata manifest.
    pub manifest: ModManifest,
    /// Loaded dynamic library (if native mod).
    pub library: Option<Arc<Library>>,
    /// Registered shutdown function symbols (if any).
    pub shutdown_symbols: Vec<String>,
}

/// Mod Loader responsible for loading, initializing, and shutting down native Steel mods.
pub struct ModLoader {
    loaded_mods: Vec<LoadedMod>,
}

impl Default for ModLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl ModLoader {
    /// Creates a new empty `ModLoader`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            loaded_mods: Vec::new(),
        }
    }

    /// Loads and initializes a discovered mod.
    ///
    /// # Safety
    /// Calling into dynamic library entrypoints executes third-party foreign code.
    ///
    /// # Errors
    /// Returns `ModLoaderError` if library loading or entrypoint execution fails.
    pub unsafe fn load_and_init(
        &mut self,
        discovered: DiscoveredMod,
        server_ptr: *mut c_void,
    ) -> Result<(), ModLoaderError> {
        let manifest = discovered.manifest;
        let mod_id = manifest.id.clone();

        let library = match &discovered.source {
            ModSource::SharedLibrary(dylib_path) => {
                // SAFETY: Loading dynamic library from path.
                let lib = unsafe {
                    Library::new(dylib_path).map_err(|e| ModLoaderError::LoadFailed {
                        mod_id: mod_id.clone(),
                        path: dylib_path.clone(),
                        error: e.to_string(),
                    })?
                };
                Some(Arc::new(lib))
            }
            ModSource::Directory(dir_path) => {
                // Look for candidate native library inside directory
                let lib_path = Self::find_dylib_in_dir(dir_path, &mod_id);
                if let Some(path) = lib_path {
                    // SAFETY: Loading dynamic library.
                    let lib = unsafe {
                        Library::new(&path).map_err(|e| ModLoaderError::LoadFailed {
                            mod_id: mod_id.clone(),
                            path,
                            error: e.to_string(),
                        })?
                    };
                    Some(Arc::new(lib))
                } else {
                    None
                }
            }
            ModSource::Archive(_) => None, // Data/resource pack mod or metadata-only
        };

        let ctx = ModContext::new(server_ptr);
        let mut shutdown_symbols = Vec::new();

        if let Some(ref lib_arc) = library {
            // Execute "main" entrypoints
            if let Some(main_entrypoints) = manifest.entrypoints.get("main") {
                for symbol_name in main_entrypoints {
                    let symbol_bytes =
                        std::ffi::CString::new(symbol_name.as_str()).unwrap_or_default();

                    // SAFETY: Transmuting loaded symbol pointer to function pointer.
                    let init_fn: Symbol<SteelModInitFn> = unsafe {
                        lib_arc.get(symbol_bytes.as_bytes_with_nul()).map_err(|e| {
                            ModLoaderError::SymbolNotFound {
                                mod_id: mod_id.clone(),
                                symbol: symbol_name.clone(),
                                error: e.to_string(),
                            }
                        })?
                    };

                    // SAFETY: Invoking mod entrypoint.
                    let res = unsafe { init_fn(&ctx) };
                    if res != 0 {
                        return Err(ModLoaderError::InitFailed {
                            mod_id,
                            symbol: symbol_name.clone(),
                            code: res,
                        });
                    }
                }
            }

            // Collect shutdown entrypoints
            if let Some(shutdown_eps) = manifest.entrypoints.get("shutdown") {
                shutdown_symbols.extend(shutdown_eps.clone());
            }
        }

        info!(
            "Successfully loaded mod '{}' (v{})",
            mod_id, manifest.version
        );

        self.loaded_mods.push(LoadedMod {
            manifest,
            library,
            shutdown_symbols,
        });

        Ok(())
    }

    /// Shuts down all loaded mods in reverse topological order.
    ///
    /// # Safety
    /// Calling shutdown function pointers executes foreign code in loaded dynamic libraries.
    pub unsafe fn shutdown(&mut self, server_ptr: *mut c_void) {
        let ctx = ModContext::new(server_ptr);

        while let Some(loaded_mod) = self.loaded_mods.pop() {
            let mod_id = loaded_mod.manifest.id;
            info!("Shutting down mod '{}'", mod_id);

            if let Some(ref lib_arc) = loaded_mod.library {
                for symbol_name in &loaded_mod.shutdown_symbols {
                    let symbol_bytes =
                        std::ffi::CString::new(symbol_name.as_str()).unwrap_or_default();

                    // SAFETY: Getting symbol from dynamic library.
                    if let Ok(shutdown_fn) = unsafe {
                        lib_arc.get::<SteelModShutdownFn>(symbol_bytes.as_bytes_with_nul())
                    } {
                        // SAFETY: Invoking shutdown entrypoint.
                        unsafe { shutdown_fn(&ctx) };
                    } else {
                        warn!("Shutdown symbol '{symbol_name}' not found for mod '{mod_id}'");
                    }
                }
            }
        }
    }

    /// Returns references to all loaded mods.
    #[must_use]
    pub fn loaded_mods(&self) -> &[LoadedMod] {
        &self.loaded_mods
    }

    fn find_dylib_in_dir(dir: &Path, mod_id: &str) -> Option<PathBuf> {
        let candidates = [
            format!("{mod_id}.so"),
            format!("lib{mod_id}.so"),
            format!("{mod_id}.dylib"),
            format!("lib{mod_id}.dylib"),
            format!("{mod_id}.dll"),
        ];

        for c in candidates {
            let p = dir.join(c);
            if p.is_file() {
                return Some(p);
            }
        }
        None
    }
}
