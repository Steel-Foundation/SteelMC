//! Host adapters. This module and its Steel dependencies are absent in extension builds.

pub(crate) mod commands;
pub(crate) mod content;
pub(crate) mod effects;
pub(crate) mod events;
pub(crate) mod fill;
mod generator;
pub(crate) mod hud;
pub(crate) mod items;
mod lifecycle;
pub(crate) mod menus;
mod package;
pub(crate) mod players;
pub(crate) mod runtime;
pub(crate) mod services;
pub(crate) mod worlds;
use crate::content::api as content_api;
use crate::hooks::api as hooks_api;
use crate::runtime::api as runtime_api;
use lifecycle::{DependencyInfo, resolve, split_dependencies};
pub(crate) use lifecycle::{ExtensionContextData, ExtensionState};
pub use lifecycle::{InitializedExtensions, LoadedExtension, initialize_all};
pub use package::discover_archives;

use crate::{ExtensionDescriptor, bindings::api};
use libloading::Library;
use stabby::{
    host::{RegistrationHook, RegistrationStatus},
    string::String as ApiString,
    vec::Vec as ApiVec,
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};
use steel_utils::locks::SyncMutex;

/// Descriptor names loaded in this process. Native libraries keep their static state for the
/// process lifetime, so a name can never be loaded twice, even across loader calls.
static EXTENSION_NAMES: SyncMutex<BTreeSet<&'static str>> = SyncMutex::new(BTreeSet::new());

use steel_core::{
    behavior::BlockBehaviorRegistry,
    extension::{ExtensionHooks, ExtensionInfo, WorldTickHandler},
    world::World as SteelWorld,
    worldgen::{
        WorldGeneratorRegistry,
        generator::{GenerationChunk, TerrainPhase},
    },
};
use steel_registry::{REGISTRY, RegistryExt, TaggedRegistryExt, blocks::BlockRef};
use steel_utils::{
    BlockPos as CoreBlockPos, BlockStateId, ChunkPos, Identifier, types::UpdateFlags,
};

use crate::GeneratorRegistry;
use crate::block_behavior::{
    DynBlockBehavior,
    adapter::Adapter,
    block_entity::{api as entity_api, factory as entity_factory},
    capability_api, original, values_api,
};
use crate::{BlockPos, BlockState, Hooks, Registry, World, WorldTickCallback};

pub(crate) use generator::{random_at, register_generator, write_terrain};

pub(crate) struct Registration<'a> {
    pub behaviors: &'a mut BlockBehaviorRegistry,
    pub config: &'a str,
}

pub(crate) struct HookRegistration<'a> {
    pub extension: &'a Arc<ExtensionState>,
    pub hooks: &'a mut ExtensionHooks,
}

pub(crate) struct TerrainAccess<'a> {
    pub chunk: GenerationChunk<'a, TerrainPhase>,
    pub seed: i64,
}

pub(crate) fn subscribe_blocks(
    hooks: &mut HookRegistration<'_>,
    callback: extern "C" fn(Registry<'_>) -> crate::ApiResult<()>,
) -> Result<(), String> {
    hooks.extension.begin_registration();
    let extension = Arc::clone(hooks.extension);
    let result = BlockBehaviorRegistry::on_registration(move |registry| {
        extension.run_registration("block", || {
            let mut access = Registration {
                behaviors: registry,
                config: &extension.config,
            };
            callback(Registry::from_host(&mut access))
        });
    });
    if result.is_err() {
        hooks.extension.cancel_registration();
    }
    result
}

pub(crate) fn subscribe_generators(
    hooks: &mut HookRegistration<'_>,
    callback: extern "C" fn(GeneratorRegistry<'_>) -> crate::ApiResult<()>,
) -> Result<(), String> {
    hooks.extension.begin_registration();
    let extension = Arc::clone(hooks.extension);
    let result = WorldGeneratorRegistry::on_registration(move |registry| {
        extension.run_registration("world generator", || {
            callback(GeneratorRegistry::from_host(registry))
        });
    });
    if result.is_err() {
        hooks.extension.cancel_registration();
    }
    result
}

/// The host's only API bootstrap. Extensions receive this function pointer directly.
#[must_use]
#[stabby::export(callback)]
pub fn bootstrap(mut hook: RegistrationHook<'_>) -> RegistrationStatus {
    for register in [
        api::register_exports,
        crate::events::api::register_exports,
        crate::events::more::api::register_exports,
        crate::players::api::register_exports,
        crate::command::api::register_exports,
        crate::services::api::register_exports,
        crate::items::api::register_exports,
        crate::effects::api::register_exports,
        crate::hud::api::register_exports,
        crate::worlds::api::register_exports,
        crate::menus::api::register_exports,
        crate::fill::api::register_exports,
        original::api::register_exports,
        values_api::api::register_exports,
        capability_api::api::register_exports,
        entity_api::api::register_exports,
        entity_factory::api::register_exports,
        runtime_api::register_exports,
        hooks_api::register_exports,
        content_api::register_exports,
    ] {
        let status = register(&mut hook);
        if status != RegistrationStatus::Accepted {
            return status;
        }
    }
    RegistrationStatus::Accepted
}

/// One extension to load: a ZIP package (`extension.toml` plus per-target libraries) or a raw
/// native library, and optionally its configuration.
pub struct ExtensionSource {
    /// ZIP package or native library path.
    pub path: PathBuf,
    /// Extension-owned TOML configuration. `None` reads `<data root>/<name>/config.toml`,
    /// creating an empty file on first start.
    pub config: Option<String>,
}

/// Extensions ready for registration and init, plus the files that were left out.
pub struct LoadReport {
    /// Pre-initialized extensions in dependency order.
    pub extensions: Vec<LoadedExtension>,
    /// One diagnostic per skipped file or extension.
    pub skipped: Vec<String>,
}

struct OpenedExtension<'a> {
    source: &'a ExtensionSource,
    descriptor: &'static ExtensionDescriptor,
}

/// ZIP packages and native libraries directly inside `directory`, sorted by file name.
/// A missing directory is empty.
///
/// # Errors
///
/// Returns an error when the directory exists but cannot be read.
pub fn discover(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = discover_archives(directory)?;
    if let Ok(entries) = std::fs::read_dir(directory) {
        files.extend(entries.flatten().map(|entry| entry.path()).filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(std::env::consts::DLL_SUFFIX))
        }));
    }
    files.sort();
    Ok(files)
}

/// Opens a package or library and validates its descriptor without running extension code.
///
/// # Safety
/// The library must be trusted native code honoring the `steel_extension_v3` contract.
unsafe fn open(source: &ExtensionSource) -> Result<OpenedExtension<'_>, String> {
    let path = &source.path;
    if path.extension().is_some_and(|ext| ext == "zip") {
        let package =
            package::prepare(path).map_err(|error| format!("{}: {error}", path.display()))?;
        // Extracted libraries have the same process lifetime as pinned native code,
        // including failure paths after library constructors have run.
        let _directory = package.directory.keep();
        // SAFETY: the caller trusts this archive; extraction preserves its library.
        let descriptor = unsafe { open_library(&package.library, path) }?;
        if descriptor.name.as_str() != package.name {
            return Err(format!(
                "{}: descriptor name {} differs from its package manifest name {}",
                path.display(),
                descriptor.name,
                package.name
            ));
        }
        return Ok(OpenedExtension { source, descriptor });
    }
    // SAFETY: the caller trusts this explicitly selected native library.
    let descriptor = unsafe { open_library(path, path) }?;
    Ok(OpenedExtension { source, descriptor })
}

/// Pins a native library and returns its checked descriptor; `origin` names it in errors.
///
/// # Safety
/// Same contract as [`open`].
unsafe fn open_library(
    library: &Path,
    origin: &Path,
) -> Result<&'static ExtensionDescriptor, String> {
    // SAFETY: the caller explicitly trusts this native library and its constructors.
    let library = unsafe { Library::new(library) }
        .map_err(|error| format!("{}: cannot load library: {error}", origin.display()))?;
    // Native callbacks can be retained by global host registries or destructors;
    // process-lifetime pinning applies even when initialization subsequently fails.
    let library = Box::leak(Box::new(library));
    // SAFETY: version three fixes this getter's signature and descriptor layout;
    // the caller trusts the library to honor it. The pinned library cannot unload.
    let getter = unsafe {
        library.get::<extern "C" fn() -> &'static ExtensionDescriptor>(b"steel_extension_v3")
    };
    let Ok(getter) = getter else {
        let older = [
            b"steel_extension_v2".as_slice(),
            b"steel_plugin_v4".as_slice(),
            b"steel_plugin_v3".as_slice(),
            b"steel_plugin_v2".as_slice(),
        ]
        .into_iter()
        // SAFETY: only symbol presence is checked; the symbol is never used.
        .any(|symbol| unsafe { library.get::<*const ()>(symbol) }.is_ok());
        return Err(if older {
            format!(
                "{}: built for an older Steel extension API (rebuild it against API {})",
                origin.display(),
                crate::API_VERSION
            )
        } else {
            format!("{}: not a Steel extension", origin.display())
        });
    };
    let descriptor = getter();
    if descriptor.name.as_str().is_empty() {
        return Err(format!(
            "{}: extension descriptor name is empty",
            origin.display()
        ));
    }
    descriptor
        .pre_initializer()
        .and_then(|_| descriptor.initializer())
        .map_err(|error| format!("{}: {error}", origin.display()))?;
    Ok(descriptor)
}

/// Reads `<data root>/<name>/config.toml`, creating an empty one when missing.
fn read_config(data_root: &Path, name: &str) -> Result<String, String> {
    let directory = data_root.join(name);
    let path = directory.join("config.toml");
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(&directory)
                .and_then(|()| std::fs::write(&path, ""))
                .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
            Ok(String::new())
        }
        Err(error) => Err(format!("cannot read {}: {error}", path.display())),
    }
}

fn pre_initialize(
    opened: &OpenedExtension<'_>,
    config: &str,
    hooks: &mut ExtensionHooks,
) -> Result<LoadedExtension, String> {
    let descriptor = opened.descriptor;
    let pre_init = descriptor.pre_initializer()?;
    let initialize = descriptor.initializer()?;
    let name = descriptor.name.as_str();
    if !EXTENSION_NAMES.lock().insert(name) {
        return Err(format!("extension {name} was loaded more than once"));
    }
    let state = Arc::new(ExtensionState::new(
        name,
        descriptor.version.as_str(),
        config,
    ));
    let result: Result<(), ApiString> = {
        let mut hook_registration = HookRegistration {
            hooks,
            extension: &state,
        };
        pre_init(bootstrap, Hooks::from_host(&mut hook_registration)).into()
    };
    result.map_err(|error| format!("extension {name} pre-init failed: {error}"))?;
    hooks.infos.push(ExtensionInfo {
        name: name.to_owned(),
        version: descriptor.version.as_str().to_owned(),
        api_version: descriptor.api_version.as_str().to_owned(),
        depends: split_dependencies(descriptor.depends.as_str())
            .into_iter()
            .map(str::to_owned)
            .collect(),
        soft_depends: split_dependencies(descriptor.soft_depends.as_str())
            .into_iter()
            .map(str::to_owned)
            .collect(),
        source: opened.source.path.display().to_string(),
    });
    Ok(LoadedExtension {
        name,
        state,
        initialize,
    })
}

/// Opens every source, skips unusable files and extensions with missing dependencies, orders
/// the rest by dependency, and pre-initializes them. Call before any registry initialization;
/// libraries remain pinned until exit. A pre-init failure, a name loaded twice or a dependency
/// cycle is an error, because native registrations cannot be rolled back.
///
/// Differences from the original `steel_extension_v2` loader: sources are opened first and
/// only usable ones run code, so a broken or outdated file in `extensions/` is skipped with a
/// diagnostic instead of stopping startup; duplicate names within one call are skipped too.
///
/// # Safety
/// Every library must be trusted native code honoring the fixed `steel_extension_v3`
/// descriptor contract. ABI reports cannot validate machine code.
pub unsafe fn load_all(
    sources: &[ExtensionSource],
    data_root: &Path,
    hooks: &mut ExtensionHooks,
) -> Result<LoadReport, String> {
    menus::install(hooks);
    let mut skipped = Vec::new();
    let mut opened = Vec::with_capacity(sources.len());
    for source in sources {
        // SAFETY: forwarded from this function's contract.
        match unsafe { open(source) } {
            Ok(extension) => opened.push(extension),
            Err(error) => skipped.push(error),
        }
    }
    let dependencies: Vec<_> = opened
        .iter()
        .map(|extension| DependencyInfo {
            name: extension.descriptor.name.as_str(),
            depends: split_dependencies(extension.descriptor.depends.as_str()),
            soft_depends: split_dependencies(extension.descriptor.soft_depends.as_str()),
        })
        .collect();
    let resolution = resolve(&dependencies)?;
    for (index, reason) in resolution.skipped {
        skipped.push(format!(
            "{} (extension {}): {reason}",
            opened[index].source.path.display(),
            dependencies[index].name
        ));
    }
    let mut extensions = Vec::with_capacity(resolution.order.len());
    for index in resolution.order {
        let extension = &opened[index];
        let config = match &extension.source.config {
            Some(config) => config.clone(),
            None => read_config(data_root, extension.descriptor.name.as_str())?,
        };
        extensions.push(pre_initialize(extension, &config, hooks)?);
    }
    Ok(LoadReport {
        extensions,
        skipped,
    })
}

/// Loads one package or library with no other extensions present and invokes pre-init.
///
/// # Safety
/// Same contract as [`load_all`].
pub unsafe fn load(
    path: &Path,
    config: &str,
    hooks: &mut ExtensionHooks,
) -> Result<LoadedExtension, String> {
    let sources = [ExtensionSource {
        path: path.to_owned(),
        config: Some(config.to_owned()),
    }];
    // SAFETY: forwarded from this function's contract.
    let report = unsafe { load_all(&sources, Path::new("."), hooks) }?;
    if let Some(reason) = report.skipped.into_iter().next() {
        return Err(reason);
    }
    report
        .extensions
        .into_iter()
        .next()
        .ok_or_else(|| "extension produced no lifecycle entry".to_owned())
}

pub(crate) fn block_by_key(key: &str) -> Result<BlockRef, String> {
    ensure_registry_access()?;
    let identifier: Identifier = key.parse().map_err(str::to_owned)?;
    REGISTRY
        .blocks
        .by_key(&identifier)
        .ok_or_else(|| format!("unknown block {identifier}"))
}

pub(crate) fn blocks_in_tag(key: &str) -> Result<ApiVec<ApiString>, String> {
    ensure_registry_access()?;
    let identifier: Identifier = key.parse().map_err(str::to_owned)?;
    let blocks = REGISTRY
        .blocks
        .get_tag(&identifier)
        .ok_or_else(|| format!("unknown block tag {identifier}"))?;
    Ok(blocks
        .into_iter()
        .map(|block| block.key.to_string().into())
        .collect())
}

pub(crate) fn ensure_registry_access() -> Result<(), String> {
    if !REGISTRY.is_initialized() {
        return Err("registries are unavailable during pre-init".into());
    }
    Ok(())
}

pub(crate) fn replace_block_behavior(
    registration: &mut Registration<'_>,
    block: &str,
    behavior: DynBlockBehavior,
) -> Result<(), String> {
    let block = block_by_key(block)?;
    registration
        .behaviors
        .set_behavior(block, Box::new(Adapter(behavior)));
    Ok(())
}

pub(crate) struct TickHandler(pub WorldTickCallback);

impl WorldTickHandler for TickHandler {
    fn tick(&self, world: &Arc<SteelWorld>, tick_count: u64) {
        (self.0)(World::from_host(world), tick_count);
    }
}

const fn position(pos: BlockPos) -> CoreBlockPos {
    CoreBlockPos::new(pos.x, pos.y, pos.z)
}

pub(crate) fn read_block(world: &Arc<SteelWorld>, pos: BlockPos) -> Result<BlockState, String> {
    let pos = position(pos);
    if !world.is_in_valid_bounds(pos) {
        return Err("block position is outside world bounds".to_owned());
    }
    world
        .chunk_map
        .with_full_chunk(ChunkPos::from_block_pos(pos), |chunk| BlockState {
            id: chunk.get_block_state(pos).0,
        })
        .ok_or_else(|| "block's chunk is not loaded".to_owned())
}

pub(crate) fn set_block(
    world: &Arc<SteelWorld>,
    pos: BlockPos,
    state: BlockState,
) -> Result<(), String> {
    let current = read_block(world, pos)?;
    if current == state {
        return Ok(());
    }
    if REGISTRY
        .blocks
        .by_state_id(BlockStateId(state.id))
        .is_none()
    {
        return Err("invalid block state".to_owned());
    }
    if world.set_block(
        position(pos),
        BlockStateId(state.id),
        UpdateFlags::UPDATE_ALL,
    ) {
        Ok(())
    } else {
        Err("block update could not be applied".to_owned())
    }
}

pub(crate) fn set_block_with_flags(
    world: &Arc<SteelWorld>,
    pos: BlockPos,
    state: BlockState,
    flags: u16,
) -> Result<(), String> {
    let current = read_block(world, pos)?;
    if current == state {
        return Ok(());
    }
    if REGISTRY
        .blocks
        .by_state_id(BlockStateId(state.id))
        .is_none()
    {
        return Err("invalid block state".to_owned());
    }
    if world.set_block(
        position(pos),
        BlockStateId(state.id),
        UpdateFlags::from_bits_truncate(flags),
    ) {
        Ok(())
    } else {
        Err("block update could not be applied".to_owned())
    }
}

pub(crate) fn parse_block_state(text: &str) -> Result<BlockState, String> {
    ensure_registry_access()?;
    steel_core::command::parse_block_state(text)
        .map(|state| BlockState { id: state.0 })
        .map_err(|error| error.to_string())
}

fn valid_state(state: BlockState) -> Option<BlockRef> {
    REGISTRY.blocks.by_state_id(BlockStateId(state.id))
}

pub(crate) fn block_key(state: BlockState) -> String {
    valid_state(state).map_or_else(|| "minecraft:air".to_owned(), |block| block.key.to_string())
}

pub(crate) fn block_state_string(state: BlockState) -> String {
    let Some(block) = valid_state(state) else {
        return "minecraft:air".to_owned();
    };
    let mut text = block.key.to_string();
    let properties: Vec<String> = REGISTRY
        .blocks
        .properties(BlockStateId(state.id))
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    if !properties.is_empty() {
        text.push('[');
        text.push_str(&properties.join(","));
        text.push(']');
    }
    text
}

pub(crate) fn is_air(state: BlockState) -> bool {
    use steel_registry::blocks::block_state_ext::BlockStateExt as _;
    valid_state(state).is_none_or(|_| BlockStateId(state.id).is_air())
}
