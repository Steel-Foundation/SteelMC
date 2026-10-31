//! Extension lifecycle state; registry callbacks run immediately at their native events.
use crate::hooks::{self, ServerStopAction, ServerTickAction};
use crate::{ApiResult, ExtensionInit};
use stabby::{str::Str, string::String as ApiString};
use std::sync::Arc;
use steel_core::behavior::{BLOCK_BEHAVIORS, FLUID_BEHAVIORS};
use steel_core::extension::{ExtensionHooks, ServerStopHandler, ServerTickHandler};
use steel_core::server::Server;
use steel_utils::locks::SyncMutex;

#[derive(Default)]
struct RegistrationStatus {
    pending: usize,
    error: Option<String>,
}

pub(crate) struct ExtensionState {
    pub name: &'static str,
    pub config: String,
    status: SyncMutex<RegistrationStatus>,
}
impl ExtensionState {
    pub(super) fn new(name: &'static str, config: &str) -> Self {
        Self {
            name,
            config: config.to_owned(),
            status: SyncMutex::new(RegistrationStatus::default()),
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

/// Loaded and pre-initialized extension, awaiting registry events and final initialization.
pub struct LoadedExtension {
    /// Diagnostic name of this pinned module.
    pub name: &'static str,
    pub(super) state: Arc<ExtensionState>,
    pub(super) initialize: ExtensionInit,
}

/// Runs init only after every module's registration has completed successfully.
/// Consuming the loaded modules prevents accidentally invoking init twice.
pub fn initialize_all(
    extensions: Vec<LoadedExtension>,
    hooks: &mut ExtensionHooks,
) -> Result<Vec<&'static str>, String> {
    if BLOCK_BEHAVIORS.get().is_none() || FLUID_BEHAVIORS.0.get().is_none() {
        return Err("extension init requires frozen behavior registries".into());
    }
    for extension in &extensions {
        extension.state.check_registration()?;
    }
    hooks::host::freeze()?;
    hooks.register_server_tick(Box::new(EventDispatcher));
    hooks.register_server_stop(Box::new(EventDispatcher));
    let mut names = Vec::with_capacity(extensions.len());
    for extension in extensions {
        let result: Result<(), ApiString> =
            (extension.initialize)(Str::new(&extension.state.config)).into();
        result.map_err(|error| format!("extension {} init failed: {error}", extension.name))?;
        names.push(extension.name);
    }
    Ok(names)
}

struct EventDispatcher;
impl ServerTickHandler for EventDispatcher {
    fn tick(&self, _server: &Arc<Server>, tick_count: u64) {
        let result: Result<(), ApiString> =
            hooks::do_action(&ServerTickAction { tick_count }).into();
        if let Err(error) = result {
            log::error!("extension server tick action failed: {error}");
        }
    }
}
impl ServerStopHandler for EventDispatcher {
    fn stop(&self, _server: &Server) {
        let result: Result<(), ApiString> = hooks::do_action(&ServerStopAction).into();
        if let Err(error) = result {
            log::error!("extension server stop action failed: {error}");
        }
    }
}
