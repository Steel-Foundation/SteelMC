//! Extension lifecycle state; registry callbacks run immediately at their native events.
use crate::hooks::{self, ServerStopAction, ServerTickAction};
use crate::{ApiResult, ExtensionContext, ExtensionInit, Server};
use stabby::string::String as ApiString;
use std::{collections::BTreeMap, path::Path, sync::Arc};
use steel_core::{
    behavior::{BLOCK_BEHAVIORS, FLUID_BEHAVIORS},
    extension::{ExtensionHooks, ServerStopHandler, ServerTickHandler},
    server::Server as CoreServer,
};
use steel_utils::locks::SyncMutex;

#[derive(Default)]
struct RegistrationStatus {
    pending: usize,
    error: Option<String>,
}

pub(crate) type ShutdownCallback = extern "C" fn(Server<'_>, u64);

pub(crate) struct ExtensionState {
    pub name: &'static str,
    pub version: &'static str,
    pub config: String,
    status: SyncMutex<RegistrationStatus>,
    shutdown: SyncMutex<Vec<(ShutdownCallback, u64)>>,
}
impl ExtensionState {
    pub(super) fn new(name: &'static str, version: &'static str, config: &str) -> Self {
        Self {
            name,
            version,
            config: config.to_owned(),
            status: SyncMutex::new(RegistrationStatus::default()),
            shutdown: SyncMutex::new(Vec::new()),
        }
    }
    pub(crate) fn begin_registration(&self) {
        self.status.lock().pending += 1;
    }
    pub(crate) fn cancel_registration(&self) {
        self.status.lock().pending -= 1;
    }
    pub(crate) fn run_registration(&self, phase: &str, callback: impl FnOnce() -> ApiResult<()>) {
        let run = self.status.lock().error.is_none();
        let result: Result<(), ApiString> = if run { callback().into() } else { Ok(()) };
        let mut status = self.status.lock();
        status.pending -= 1;
        if let Err(error) = result {
            status.error = Some(format!(
                "extension {} {phase} registration failed: {error}",
                self.name
            ));
        }
    }
    pub(crate) fn add_shutdown(&self, callback: ShutdownCallback, token: u64) {
        self.shutdown.lock().push((callback, token));
    }
    fn check_registration(&self) -> Result<(), String> {
        let status = self.status.lock();
        if let Some(error) = &status.error {
            return Err(error.to_owned());
        }
        if status.pending != 0 {
            return Err(format!(
                "extension {} still has pending registration events",
                self.name
            ));
        }
        Ok(())
    }
}

/// Host data behind [`ExtensionContext`], valid only during init.
pub(crate) struct ExtensionContextData<'a> {
    pub state: &'a ExtensionState,
    pub data_directory: String,
}

/// Loaded and pre-initialized extension, awaiting registry events and final initialization.
pub struct LoadedExtension {
    /// Diagnostic name of this pinned module.
    pub name: &'static str,
    pub(super) state: Arc<ExtensionState>,
    pub(super) initialize: ExtensionInit,
}

/// Initialized extensions in dependency order, retained to run shutdown callbacks.
pub struct InitializedExtensions {
    states: Vec<Arc<ExtensionState>>,
}

impl core::fmt::Debug for InitializedExtensions {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_list().entries(self.names()).finish()
    }
}

impl InitializedExtensions {
    /// Extension names in initialization order.
    #[must_use]
    pub fn names(&self) -> Vec<&'static str> {
        self.states.iter().map(|state| state.name).collect()
    }

    /// Runs every shutdown callback once, in reverse initialization order.
    /// Call on the server's thread before worlds are saved.
    pub fn shutdown(&self, server: &Arc<CoreServer>) {
        for state in self.states.iter().rev() {
            let callbacks = std::mem::take(&mut *state.shutdown.lock());
            for (callback, token) in callbacks {
                callback(Server::from_host(server), token);
            }
        }
    }
}

/// Runs init only after every module's registration has completed successfully.
/// Each extension receives `data_root/<name>`, created before its init runs. Freezes the
/// action/filter registry and installs the server tick and stop action dispatcher first.
/// Consuming the loaded modules prevents accidentally invoking init twice.
pub fn initialize_all(
    extensions: Vec<LoadedExtension>,
    data_root: &Path,
    hooks: &mut ExtensionHooks,
) -> Result<InitializedExtensions, String> {
    if BLOCK_BEHAVIORS.get().is_none() || FLUID_BEHAVIORS.0.get().is_none() {
        return Err("extension init requires frozen behavior registries".into());
    }
    for extension in &extensions {
        extension.state.check_registration()?;
    }
    hooks::host::freeze()?;
    hooks.register_server_tick(Box::new(EventDispatcher));
    hooks.register_server_stop(Box::new(EventDispatcher));
    let mut states = Vec::with_capacity(extensions.len());
    for extension in extensions {
        let directory = data_root.join(extension.name);
        std::fs::create_dir_all(&directory).map_err(|error| {
            format!(
                "cannot create data directory {} for extension {}: {error}",
                directory.display(),
                extension.name
            )
        })?;
        let directory = std::path::absolute(&directory).unwrap_or(directory);
        let context = ExtensionContextData {
            state: &extension.state,
            data_directory: directory.to_string_lossy().into_owned(),
        };
        let result: Result<(), ApiString> =
            (extension.initialize)(ExtensionContext::from_host(&context)).into();
        result.map_err(|error| format!("extension {} init failed: {error}", extension.name))?;
        states.push(extension.state);
    }
    Ok(InitializedExtensions { states })
}

/// Emits the built-in server tick and stop actions.
struct EventDispatcher;
impl ServerTickHandler for EventDispatcher {
    fn tick(&self, _server: &Arc<CoreServer>, tick_count: u64) {
        let result: Result<(), ApiString> =
            hooks::do_action(&ServerTickAction { tick_count }).into();
        if let Err(error) = result {
            log::error!("extension server tick action failed: {error}");
        }
    }
}
impl ServerStopHandler for EventDispatcher {
    fn stop(&self, _server: &CoreServer) {
        let result: Result<(), ApiString> = hooks::do_action(&ServerStopAction).into();
        if let Err(error) = result {
            log::error!("extension server stop action failed: {error}");
        }
    }
}

/// Dependency declaration of one extension, as read from its descriptor.
pub(crate) struct DependencyInfo<'a> {
    pub name: &'a str,
    pub depends: Vec<&'a str>,
    pub soft_depends: Vec<&'a str>,
}

/// Splits a descriptor's comma-separated dependency list.
pub(crate) fn split_dependencies(list: &str) -> Vec<&str> {
    list.split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
/// Depth-first search state of one extension.
enum Mark {
    New,
    Visiting,
    Done,
}
/// Appends `node` after its dependencies; `Visiting` marks detect cycles.
fn visit(
    node: usize,
    edges: &[Vec<usize>],
    marks: &mut [Mark],
    order: &mut Vec<usize>,
    extensions: &[DependencyInfo<'_>],
) -> Result<(), String> {
    match marks[node] {
        Mark::Done => return Ok(()),
        Mark::Visiting => {
            return Err(format!(
                "extension dependency cycle involving {}",
                extensions[node].name
            ));
        }
        Mark::New => {}
    }
    marks[node] = Mark::Visiting;
    for &dependency in &edges[node] {
        visit(dependency, edges, marks, order, extensions)?;
    }
    marks[node] = Mark::Done;
    order.push(node);
    Ok(())
}

/// Dependency resolution result: load order and the extensions left out.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Resolution {
    /// Indices into the input, dependencies first.
    pub order: Vec<usize>,
    /// Skipped extensions with the reason, in input order.
    pub skipped: Vec<(usize, String)>,
}

/// Drops duplicate names and extensions whose hard dependencies are missing (transitively), then
/// orders the rest so every present dependency comes first, keeping input order otherwise.
/// A dependency cycle is an error because no valid order exists.
pub(crate) fn resolve(extensions: &[DependencyInfo<'_>]) -> Result<Resolution, String> {
    let mut skipped = Vec::new();
    let mut alive = vec![true; extensions.len()];
    let mut counts = BTreeMap::new();
    for extension in extensions {
        *counts.entry(extension.name).or_insert(0_usize) += 1;
    }
    for (position, extension) in extensions.iter().enumerate() {
        if counts[extension.name] > 1 {
            alive[position] = false;
            skipped.push((
                position,
                format!("another file also declares extension {}", extension.name),
            ));
        }
    }
    loop {
        let present: BTreeMap<&str, usize> = extensions
            .iter()
            .enumerate()
            .filter(|(position, _)| alive[*position])
            .map(|(position, extension)| (extension.name, position))
            .collect();
        let mut changed = false;
        for (position, extension) in extensions.iter().enumerate() {
            if !alive[position] {
                continue;
            }
            if let Some(missing) = extension
                .depends
                .iter()
                .find(|name| !present.contains_key(*name))
            {
                alive[position] = false;
                changed = true;
                skipped.push((position, format!("requires missing extension {missing}")));
            }
        }
        if !changed {
            break;
        }
    }
    skipped.sort_by_key(|(position, _)| *position);
    let index: BTreeMap<&str, usize> = extensions
        .iter()
        .enumerate()
        .filter(|(position, _)| alive[*position])
        .map(|(position, extension)| (extension.name, position))
        .collect();
    let mut edges = vec![Vec::new(); extensions.len()];
    for (position, extension) in extensions.iter().enumerate().filter(|(p, _)| alive[*p]) {
        for dependency in extension.depends.iter().chain(&extension.soft_depends) {
            if let Some(&target) = index.get(dependency) {
                edges[position].push(target);
            }
        }
    }
    let mut marks = vec![Mark::New; extensions.len()];
    let mut order = Vec::with_capacity(index.len());
    for node in (0..extensions.len()).filter(|node| alive[*node]) {
        visit(node, &edges, &mut marks, &mut order, extensions)?;
    }
    Ok(Resolution { order, skipped })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info<'a>(name: &'a str, depends: &[&'a str], soft: &[&'a str]) -> DependencyInfo<'a> {
        DependencyInfo {
            name,
            depends: depends.to_vec(),
            soft_depends: soft.to_vec(),
        }
    }

    #[test]
    fn dependencies_come_first_and_independent_extensions_keep_their_order() {
        let extensions = [
            info("worldguard", &["worldedit"], &[]),
            info("essentials", &[], &["vault"]),
            info("worldedit", &[], &[]),
            info("vault", &[], &[]),
        ];
        let resolution = resolve(&extensions).map(|r| r.order);
        assert_eq!(resolution, Ok(vec![2, 0, 3, 1]));
    }

    #[test]
    fn missing_soft_dependency_is_ignored() {
        let extensions = [info("essentials", &[], &["vault"])];
        assert_eq!(resolve(&extensions).map(|r| r.order), Ok(vec![0]));
    }

    #[test]
    fn missing_hard_dependencies_skip_dependents_transitively() {
        let extensions = [
            info("prison-enchants", &["prison-mines"], &[]),
            info("prison-mines", &["worldedit"], &[]),
            info("vault", &[], &[]),
        ];
        let Ok(resolution) = resolve(&extensions) else {
            panic!("missing dependencies are skips, not errors");
        };
        assert_eq!(resolution.order, vec![2]);
        let skipped: Vec<usize> = resolution.skipped.iter().map(|(p, _)| *p).collect();
        assert_eq!(skipped, vec![0, 1]);
        assert!(resolution.skipped[1].1.contains("worldedit"));
    }

    #[test]
    fn duplicate_names_skip_every_copy() {
        let extensions = [
            info("a", &[], &[]),
            info("b", &[], &[]),
            info("a", &[], &[]),
        ];
        let Ok(resolution) = resolve(&extensions) else {
            panic!("duplicates are skips");
        };
        assert_eq!(resolution.order, vec![1]);
        assert_eq!(resolution.skipped.len(), 2);
    }

    #[test]
    fn cycles_are_errors() {
        let cycle = [info("a", &["b"], &[]), info("b", &[], &["a"])];
        assert!(resolve(&cycle).is_err_and(|e| e.contains("cycle")));
    }

    #[test]
    fn dependency_lists_are_trimmed() {
        assert_eq!(split_dependencies(" a, b ,,"), ["a", "b"]);
        assert_eq!(split_dependencies(""), Vec::<&str>::new());
    }
}
