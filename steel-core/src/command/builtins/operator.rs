//! Vanilla operator commands backed by Steel's built-in `op` permission group.

use std::{convert::Infallible, sync::Arc};

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
use crate::{
    permission::{OP_GROUP, PermissionSubjectState},
    server::Server,
};

pub(super) fn op_registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("op"), |_| op_command())
}

pub(super) fn deop_registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("deop"), |_| deop_command())
}

fn op_command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("op").then(
        argument("targets", SteelArgumentType::non_operator_profile())
            .executes_suspended(|context| start_operation(context, OperatorAction::Grant)),
    )
}

fn deop_command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("deop").then(
        argument("targets", SteelArgumentType::operator_profile())
            .executes_suspended(|context| start_operation(context, OperatorAction::Revoke)),
    )
}

#[derive(Clone, Copy)]
enum OperatorAction {
    Grant,
    Revoke,
}

impl OperatorAction {
    const fn command_name(self) -> &'static str {
        match self {
            Self::Grant => "op",
            Self::Revoke => "deop",
        }
    }

    fn failed(self) -> TextComponent {
        match self {
            Self::Grant => TextComponent::from(&translations::COMMANDS_OP_FAILED),
            Self::Revoke => TextComponent::from(&translations::COMMANDS_DEOP_FAILED),
        }
    }

    fn success(self, name: String) -> TextComponent {
        match self {
            Self::Grant => translations::COMMANDS_OP_SUCCESS
                .message([TextComponent::plain(name)])
                .component(),
            Self::Revoke => translations::COMMANDS_DEOP_SUCCESS
                .message([TextComponent::plain(name)])
                .component(),
        }
    }
}

fn start_operation(
    context: &SteelCommandContext<CommandSource>,
    action: OperatorAction,
) -> Result<OperatorCommandSuspension, CommandSyntaxError> {
    let argument = context
        .game_profile_argument("targets")
        .cloned()
        .ok_or_else(|| CommandSyntaxError::dynamic("Missing game profile argument 'targets'"))?;
    let source = context.source().clone();
    let task_source = source.clone();
    let (sender, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let result = run_operation(&task_source, argument, action).await;
        let _ = sender.send(result);
    });
    Ok(OperatorCommandSuspension {
        source,
        action,
        receiver,
        task: Some(task),
    })
}

struct OperatorCommandResult {
    changed_names: Vec<String>,
}

async fn run_operation(
    source: &CommandSource,
    argument: GameProfileArgument,
    action: OperatorAction,
) -> Result<OperatorCommandResult, CommandSyntaxError> {
    let targets = argument.resolve(source).await?;
    let mut changed_names = Vec::new();
    for target in targets {
        if update_operator_group(source.server(), target.uuid, action).await? {
            changed_names.push(target.name);
        }
    }
    if changed_names.is_empty() {
        return Err(CommandSyntaxError::dynamic(action.failed()));
    }
    Ok(OperatorCommandResult { changed_names })
}

async fn update_operator_group(
    server: &Arc<Server>,
    uuid: uuid::Uuid,
    action: OperatorAction,
) -> Result<bool, CommandSyntaxError> {
    let result = server
        .try_update_player_permissions(uuid, move |state| {
            let (mut groups, overrides, metadata) = state.into_parts();
            let changed = update_groups(&mut groups, action);
            Ok::<_, Infallible>((
                PermissionSubjectState::new_with_metadata(groups, overrides, metadata),
                changed,
            ))
        })
        .await
        .map_err(|error| CommandSyntaxError::dynamic(error.to_string()))?;
    Ok(result.1)
}

fn update_groups(groups: &mut Vec<String>, action: OperatorAction) -> bool {
    match action {
        OperatorAction::Grant => {
            if groups.iter().any(|group| group == OP_GROUP) {
                false
            } else {
                groups.push(OP_GROUP.to_owned());
                true
            }
        }
        OperatorAction::Revoke => {
            let old_len = groups.len();
            groups.retain(|group| group != OP_GROUP);
            groups.len() != old_len
        }
    }
}

struct OperatorCommandSuspension {
    source: CommandSource,
    action: OperatorAction,
    receiver: oneshot::Receiver<Result<OperatorCommandResult, CommandSyntaxError>>,
    task: Option<JoinHandle<()>>,
}

impl CommandResultSuspension for OperatorCommandSuspension {
    fn order(&self) -> CommandSuspensionOrder {
        CommandSuspensionOrder::Global
    }

    fn poll(&mut self) -> CommandResultSuspensionPoll {
        match self.receiver.try_recv() {
            Ok(result) => {
                self.task = None;
                CommandResultSuspensionPoll::Ready(result.map(|result| {
                    let changed = result.changed_names.len().min(i32::MAX as usize) as i32;
                    for name in result.changed_names {
                        self.source.send_success(&self.action.success(name), true);
                    }
                    changed
                }))
            }
            Err(oneshot::error::TryRecvError::Empty) => CommandResultSuspensionPoll::Pending,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.task = None;
                CommandResultSuspensionPoll::Ready(Err(CommandSyntaxError::dynamic(format!(
                    "{} command task ended without a result",
                    self.action.command_name()
                ))))
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
    use steel_protocol::packets::game::{
        ArgumentType as ProtocolArgumentType, SuggestionType as ProtocolSuggestionType,
    };
    use steel_registry::test_support::init_test_registry;

    use super::{OperatorAction, update_groups};
    use crate::command::builtins::create_dispatcher;
    use crate::command::execution::SteelArgumentType;

    #[test]
    fn operator_group_updates_are_idempotent_and_preserve_other_groups() {
        let mut groups = vec!["builder".to_owned()];
        assert!(update_groups(&mut groups, OperatorAction::Grant));
        assert_eq!(groups, ["builder", "op"]);
        assert!(!update_groups(&mut groups, OperatorAction::Grant));
        assert!(update_groups(&mut groups, OperatorAction::Revoke));
        assert_eq!(groups, ["builder"]);
        assert!(!update_groups(&mut groups, OperatorAction::Revoke));
    }

    #[test]
    fn operator_targets_use_vanillas_game_profile_argument() {
        init_test_registry();
        let dispatcher = create_dispatcher();
        let Ok(dispatcher) = dispatcher else {
            panic!("built-in dispatcher should build");
        };
        for command_name in ["op", "deop"] {
            let root = dispatcher.children(dispatcher.root()).and_then(|children| {
                children.iter().copied().find(|child| {
                    dispatcher
                        .node(*child)
                        .is_some_and(|node| node.name() == command_name)
                })
            });
            let Some(root) = root else {
                panic!("{command_name} root should exist");
            };
            let target = dispatcher
                .children(root)
                .and_then(|children| children.first())
                .and_then(|target| dispatcher.node(*target));
            let Some(target) = target else {
                panic!("{command_name} target should exist");
            };
            let protocol = target
                .argument_type()
                .map(SteelArgumentType::protocol_argument);
            assert!(matches!(
                protocol,
                Some((
                    ProtocolArgumentType::GameProfile,
                    Some(ProtocolSuggestionType::AskServer),
                ))
            ));
            assert!(target.is_executable());
        }
    }
}
