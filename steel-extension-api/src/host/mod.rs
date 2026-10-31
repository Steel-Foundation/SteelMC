//! Host adapters. This module and its Steel dependencies are absent in extension builds.

mod generator;
mod lifecycle;
mod package;
pub use package::discover_archives;
pub(crate) mod runtime;
use crate::hooks::api as hooks_api;
use crate::runtime::api as runtime_api;
pub(crate) use lifecycle::ExtensionState;
pub use lifecycle::{LoadedExtension, initialize_all};

use crate::{ExtensionDescriptor, bindings::api};
use libloading::Library;
use stabby::{
    host::{RegistrationHook, RegistrationStatus},
    string::String as ApiString,
    vec::Vec as ApiVec,
};
use std::{collections::BTreeSet, path::Path, sync::Arc};
use steel_utils::locks::SyncMutex;

static EXTENSION_NAMES: SyncMutex<BTreeSet<&'static str>> = SyncMutex::new(BTreeSet::new());

use steel_core::{
    behavior::BlockBehaviorRegistry,
    extension::{ExtensionHooks, WorldTickHandler},
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
        original::api::register_exports,
        values_api::api::register_exports,
        capability_api::api::register_exports,
        entity_api::api::register_exports,
        entity_factory::api::register_exports,
        runtime_api::register_exports,
        hooks_api::register_exports,
    ] {
        let status = register(&mut hook);
        if status != RegistrationStatus::Accepted {
            return status;
        }
    }
    RegistrationStatus::Accepted
}

/// Loads a configured module and invokes pre-init with hooks only.
/// Call before any registry initialization; libraries remain pinned until exit.
///
/// # Safety
/// The configured library must be trusted native code and honor the fixed
/// `steel_extension_v2` descriptor contract. ABI reports cannot validate machine code.
pub unsafe fn load(
    path: &Path,
    config: &str,
    hooks: &mut ExtensionHooks,
) -> Result<LoadedExtension, String> {
    if path.extension().is_some_and(|ext| ext == "zip") {
        let package = package::prepare(path)?;
        // Extracted libraries have the same process lifetime as pinned native code,
        // including failure paths after library constructors have run.
        let _directory = package.directory.keep();
        // SAFETY: the caller trusts this archive; extraction preserves its library.
        return unsafe { load_native(&package.library, config, hooks, Some(&package.name)) };
    }
    // SAFETY: the caller trusts this explicitly selected native library.
    unsafe { load_native(path, config, hooks, None) }
}

unsafe fn load_native(
    path: &Path,
    config: &str,
    hooks: &mut ExtensionHooks,
    expected_name: Option<&str>,
) -> Result<LoadedExtension, String> {
    // SAFETY: the caller explicitly trusts this native library and its constructors.
    let library = unsafe { Library::new(path) }
        .map_err(|error| format!("failed to load extension {}: {error}", path.display()))?;
    // Native callbacks can be retained by global host registries or destructors;
    // process-lifetime pinning applies even when initialization subsequently fails.
    let library = Box::leak(Box::new(library));
    // SAFETY: version two fixes this getter's signature and descriptor layout;
    // the caller trusts the library to honor it. The pinned library cannot unload.
    let getter = unsafe {
        library.get::<extern "C" fn() -> &'static ExtensionDescriptor>(b"steel_extension_v2")
    }
    .map_err(|error| {
        format!(
            "extension {} has no version-two entry: {error}",
            path.display()
        )
    })?;
    let descriptor = getter();
    let pre_init = descriptor.pre_initializer()?;
    let initialize = descriptor.initializer()?;
    let name = descriptor.name.as_str();
    if name.is_empty() || expected_name.is_some_and(|expected| expected != name) {
        return Err(
            "extension descriptor name is empty or differs from its package manifest".into(),
        );
    }
    if !EXTENSION_NAMES.lock().insert(name) {
        return Err(format!("extension {name} was loaded more than once"));
    }
    let state = Arc::new(ExtensionState::new(name, config));
    let result: Result<(), ApiString> = {
        let mut hook_registration = HookRegistration {
            hooks,
            extension: &state,
        };
        pre_init(bootstrap, Hooks::from_host(&mut hook_registration)).into()
    };
    result.map_err(|error| format!("extension {name} pre-init failed: {error}"))?;
    Ok(LoadedExtension {
        name,
        state,
        initialize,
    })
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
