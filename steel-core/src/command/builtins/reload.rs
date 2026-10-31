//! Vanilla `/reload` backed by Steel's plugin resource reloaders.

use steel_utils::{Identifier, translations};
use text_components::TextComponent;
use tokio::{sync::oneshot, task::JoinHandle};

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandResultSuspension, CommandResultSuspensionPoll, CommandSource,
        CommandSuspensionOrder, SteelCommandContext, SteelCommandRuntime, literal,
    },
    registration::CommandRegistration,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("reload"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("reload").executes_suspended(start_reload)
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Command executors use a shared fallible callback signature."
)]
fn start_reload(
    context: &SteelCommandContext<CommandSource>,
) -> Result<ReloadCommandSuspension, CommandSyntaxError> {
    let source = context.source().clone();
    let task_source = source.clone();
    let (sender, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let result = task_source
            .server()
            .reload_resources()
            .await
            .map_err(|error| error.to_string());
        let _ = sender.send(result);
    });

    // Vanilla acknowledges the reload before the new data is published.
    source.send_success(
        &TextComponent::from(&translations::COMMANDS_RELOAD_SUCCESS),
        true,
    );
    Ok(ReloadCommandSuspension {
        source,
        receiver,
        task: Some(task),
    })
}

struct ReloadCommandSuspension {
    source: CommandSource,
    receiver: oneshot::Receiver<Result<(), String>>,
    task: Option<JoinHandle<()>>,
}

impl CommandResultSuspension for ReloadCommandSuspension {
    fn order(&self) -> CommandSuspensionOrder {
        CommandSuspensionOrder::Global
    }

    fn poll(&mut self) -> CommandResultSuspensionPoll {
        match self.receiver.try_recv() {
            Ok(Ok(())) => {
                self.task = None;
                CommandResultSuspensionPoll::Ready(Ok(0))
            }
            Ok(Err(error)) => {
                self.task = None;
                tracing::warn!(%error, "resource reload failed");
                self.source
                    .send_failure(TextComponent::from(&translations::COMMANDS_RELOAD_FAILURE));
                CommandResultSuspensionPoll::Ready(Ok(0))
            }
            Err(oneshot::error::TryRecvError::Empty) => CommandResultSuspensionPoll::Pending,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.task = None;
                tracing::warn!("resource reload task ended without a result");
                self.source
                    .send_failure(TextComponent::from(&translations::COMMANDS_RELOAD_FAILURE));
                CommandResultSuspensionPoll::Ready(Ok(0))
            }
        }
    }

    fn cancel(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
