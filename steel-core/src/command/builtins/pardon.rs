//! Vanilla player-unbanning command.

use steel_utils::{Identifier, translations};
use text_components::TextComponent;
use tokio::{sync::oneshot, task::JoinHandle};

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandResultSuspension, CommandResultSuspensionPoll, CommandSource,
        CommandSuspensionOrder, GameProfileArgument, SteelArgumentType, SteelCommandContext,
        SteelCommandRuntime, argument, literal,
    },
    registration::CommandRegistration,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("pardon"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("pardon").then(
        argument("targets", SteelArgumentType::game_profile()).executes_suspended(start_pardon),
    )
}

fn start_pardon(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PardonCommandSuspension, CommandSyntaxError> {
    let argument = context.game_profile_argument("targets").cloned()?;
    let source = context.source().clone();
    let task_source = source.clone();
    let (sender, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let result = run_pardon(&task_source, argument).await;
        let _ = sender.send(result);
    });
    Ok(PardonCommandSuspension {
        source,
        receiver,
        task: Some(task),
    })
}

struct PardonCommandResult {
    pardoned_names: Vec<String>,
}

async fn run_pardon(
    source: &CommandSource,
    argument: GameProfileArgument,
) -> Result<PardonCommandResult, CommandSyntaxError> {
    let targets = argument.resolve(source).await?;
    let mut pardoned_names = Vec::new();

    for target in targets {
        let removed = source
            .server()
            .ban_list
            .remove(target.uuid)
            .await
            .map_err(|error| CommandSyntaxError::dynamic(error.to_string()))?;
        if removed {
            pardoned_names.push(target.name);
        }
    }

    if pardoned_names.is_empty() {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_PARDON_FAILED,
        )));
    }
    Ok(PardonCommandResult { pardoned_names })
}

struct PardonCommandSuspension {
    source: CommandSource,
    receiver: oneshot::Receiver<Result<PardonCommandResult, CommandSyntaxError>>,
    task: Option<JoinHandle<()>>,
}

impl CommandResultSuspension for PardonCommandSuspension {
    fn order(&self) -> CommandSuspensionOrder {
        CommandSuspensionOrder::Global
    }

    fn poll(&mut self) -> CommandResultSuspensionPoll {
        match self.receiver.try_recv() {
            Ok(result) => {
                self.task = None;
                CommandResultSuspensionPoll::Ready(result.map(|result| {
                    let count = result.pardoned_names.len().min(i32::MAX as usize) as i32;
                    for name in result.pardoned_names {
                        let message = translations::COMMANDS_PARDON_SUCCESS
                            .message([TextComponent::plain(name)])
                            .component();
                        self.source.send_success(&message, true);
                    }
                    count
                }))
            }
            Err(oneshot::error::TryRecvError::Empty) => CommandResultSuspensionPoll::Pending,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.task = None;
                CommandResultSuspensionPoll::Ready(Err(CommandSyntaxError::dynamic(
                    "pardon command task ended without a result",
                )))
            }
        }
    }

    fn cancel(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use super::super::create_dispatcher;
    use crate::command::execution::SteelArgumentType;

    #[test]
    fn pardon_targets_use_vanillas_game_profile_argument() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let Some(pardon) = dispatcher.children(dispatcher.root()).and_then(|children| {
            children.iter().copied().find(|child| {
                dispatcher
                    .node(*child)
                    .is_some_and(|node| node.name() == "pardon")
            })
        }) else {
            panic!("pardon root should exist");
        };
        let Some(targets) = dispatcher
            .children(pardon)
            .and_then(|children| children.first())
        else {
            panic!("pardon targets should exist");
        };
        assert!(matches!(
            dispatcher.node(*targets),
            Some(node)
                if node.is_executable()
                    && node.argument_type() == Some(&SteelArgumentType::game_profile())
        ));
    }
}
