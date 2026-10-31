//! Reloadable server resources.
//!
//! A resource is prepared off the tick thread and then published from a safe point through the
//! server job queue. Registrations made outside reload events stay registered for the current
//! pass; every resource is reset to its built-in data once the new values are published, so the
//! next pass starts from a clean baseline. Steel has no event system or datapack loader yet, so
//! only the built-in data is rebuilt.

mod advancements;

use std::sync::Arc;

use thiserror::Error;
use tokio::{
    sync::oneshot,
    task::{JoinError, spawn_blocking},
};

use self::advancements::AdvancementsResource;
use super::{
    Server,
    jobs::{JobPoll, ServerJob, ServerJobContext},
};

/// Every resource that `/reload` refreshes.
static RELOADABLE_RESOURCES: &[&dyn ReloadableResource] = &[&AdvancementsResource];

/// A server resource that can be rebuilt from its registration sources.
pub trait ReloadableResource: Send + Sync {
    /// Stable identifier used in diagnostics.
    fn name(&self) -> &'static str;

    /// Discards plugin and datapack data from the previous reload pass.
    ///
    /// Runs after the freshly prepared data is published. Registrations made outside reload
    /// events therefore stay valid for the pass that is currently loading, and are discarded
    /// only once it has been applied.
    fn reset(&self) {}

    /// Builds the replacement value. This runs on the blocking pool.
    fn prepare(&self) -> Result<Box<dyn PreparedResource>, String>;
}

/// A replacement value produced by [`ReloadableResource::prepare`].
pub trait PreparedResource: Send {
    /// Publishes the replacement. This runs on the server tick thread.
    fn apply(self: Box<Self>, server: &Server) -> Result<(), String>;
}

/// A prepared resource waiting to be published with its resource name.
struct PreparedReload {
    name: &'static str,
    resource: Box<dyn PreparedResource>,
}

/// Errors that prevent `/reload` from publishing a full set of resources.
#[derive(Debug, Error)]
pub enum ResourceReloadError {
    /// A resource could not be built.
    #[error("failed to prepare resource '{resource}': {message}")]
    Prepare {
        /// Resource identifier.
        resource: &'static str,
        /// Failure reported by the resource.
        message: String,
    },
    /// A prepared resource could not be published.
    #[error("failed to apply resource '{resource}': {message}")]
    Apply {
        /// Resource identifier.
        resource: &'static str,
        /// Failure reported by the resource.
        message: String,
    },
    /// The blocking preparation task failed.
    #[error("resource preparation task failed: {0}")]
    Task(#[from] JoinError),
    /// The server stopped before the prepared resources could be published.
    #[error("the server stopped before the reload could be applied")]
    Cancelled,
}

impl Server {
    /// Reloads every reloadable resource.
    ///
    /// Preparation runs on the blocking pool. The prepared values are published by a server job,
    /// so live world and player state is never mutated off the tick thread.
    pub async fn reload_resources(self: &Arc<Self>) -> Result<(), ResourceReloadError> {
        // TODO: Fire the reload event here so plugins and datapacks can register their resources
        // for the new pass. Registrations made outside the event are already present and remain
        // valid until this pass has been published.
        let prepared = prepare_reloadable_resources().await?;
        let (sender, receiver) = oneshot::channel();
        self.jobs.spawn(ApplyResourceReloadJob {
            prepared: Some(prepared),
            sender: Some(sender),
        });
        receiver
            .await
            .map_err(|_| ResourceReloadError::Cancelled)??;

        // Reset after publishing so registrations made outside reload events were part of this
        // pass. The next pass starts again from the built-in data. A failed reload keeps them for
        // the next attempt instead of dropping registrations that never reached a published tree.
        reset_reloadable_resources();
        Ok(())
    }
}

fn reset_reloadable_resources() {
    for resource in RELOADABLE_RESOURCES {
        resource.reset();
    }
}

async fn prepare_reloadable_resources() -> Result<Vec<PreparedReload>, ResourceReloadError> {
    let mut tasks = Vec::with_capacity(RELOADABLE_RESOURCES.len());
    for resource in RELOADABLE_RESOURCES {
        let name = resource.name();
        tasks.push((name, spawn_blocking(move || resource.prepare())));
    }

    let mut prepared = Vec::with_capacity(tasks.len());
    for (name, task) in tasks {
        let resource = task
            .await
            .map_err(ResourceReloadError::Task)?
            .map_err(|message| ResourceReloadError::Prepare {
                resource: name,
                message,
            })?;
        prepared.push(PreparedReload { name, resource });
    }
    Ok(prepared)
}

fn apply_reloads(
    prepared: Vec<PreparedReload>,
    server: &Server,
) -> Result<(), ResourceReloadError> {
    for PreparedReload { name, resource } in prepared {
        resource
            .apply(server)
            .map_err(|message| ResourceReloadError::Apply {
                resource: name,
                message,
            })?;
    }
    Ok(())
}

struct ApplyResourceReloadJob {
    prepared: Option<Vec<PreparedReload>>,
    sender: Option<oneshot::Sender<Result<(), ResourceReloadError>>>,
}

impl ServerJob for ApplyResourceReloadJob {
    fn poll(&mut self, context: &mut ServerJobContext) -> JobPoll {
        let Some(prepared) = self.prepared.take() else {
            return JobPoll::Finished;
        };

        let result = match context.server() {
            Some(server) => apply_reloads(prepared, &server),
            None => Err(ResourceReloadError::Cancelled),
        };
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(result);
        }
        JobPoll::Finished
    }
}
