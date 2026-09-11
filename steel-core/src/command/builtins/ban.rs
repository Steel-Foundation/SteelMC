//! Vanilla player-banning command.

use chrono::Utc;
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
use crate::ban::BanEntry;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("ban"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("ban").then(
        argument("targets", SteelArgumentType::game_profile())
            .executes_suspended(ban_without_reason)
            .then(
                argument("reason", SteelArgumentType::message())
                    .executes_suspended(ban_with_reason),
            ),
    )
}

fn ban_without_reason(
    context: &SteelCommandContext<CommandSource>,
) -> Result<BanCommandSuspension, CommandSyntaxError> {
    start_ban(context, None)
}

fn ban_with_reason(
    context: &SteelCommandContext<CommandSource>,
) -> Result<BanCommandSuspension, CommandSyntaxError> {
    let reason = context.message("reason")?;
    let reason = TextComponent::from_snbt(reason)
        .unwrap_or_else(|_| TextComponent::plain(reason.to_owned()));
    start_ban(context, Some(reason))
}

fn start_ban(
    context: &SteelCommandContext<CommandSource>,
    reason: Option<TextComponent>,
) -> Result<BanCommandSuspension, CommandSyntaxError> {
    let argument = context.game_profile_argument("targets").cloned()?;
    let source = context.source().clone();
    let task_source = source.clone();
    let (sender, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let result = run_ban(&task_source, argument, reason).await;
        let _ = sender.send(result);
    });
    Ok(BanCommandSuspension {
        source,
        receiver,
        task: Some(task),
    })
}

struct BanCommandResult {
    banned: Vec<(String, TextComponent)>,
}

async fn run_ban(
    source: &CommandSource,
    argument: GameProfileArgument,
    reason: Option<TextComponent>,
) -> Result<BanCommandResult, CommandSyntaxError> {
    let targets = argument.resolve(source).await?;
    let source_name = source.sender().to_string();
    let mut banned = Vec::new();

    for target in targets {
        if source.server().ban_list.is_banned(target.uuid) {
            continue;
        }

        let entry = BanEntry {
            uuid: target.uuid,
            name: target.name.clone(),
            created: Utc::now(),
            source: source_name.clone(),
            expires: None,
            reason: reason.clone(),
        };
        let reason_message = entry.reason_message();
        source
            .server()
            .ban_list
            .add(entry)
            .await
            .map_err(|error| CommandSyntaxError::dynamic(error.to_string()))?;

        if let Some(online) = source.server().online_player(target.uuid) {
            online.disconnect(TextComponent::from(
                &translations::MULTIPLAYER_DISCONNECT_BANNED,
            ));
        }

        banned.push((target.name, reason_message));
    }

    if banned.is_empty() {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_BAN_FAILED,
        )));
    }
    Ok(BanCommandResult { banned })
}

struct BanCommandSuspension {
    source: CommandSource,
    receiver: oneshot::Receiver<Result<BanCommandResult, CommandSyntaxError>>,
    task: Option<JoinHandle<()>>,
}

impl CommandResultSuspension for BanCommandSuspension {
    fn order(&self) -> CommandSuspensionOrder {
        CommandSuspensionOrder::Global
    }

    fn poll(&mut self) -> CommandResultSuspensionPoll {
        match self.receiver.try_recv() {
            Ok(result) => {
                self.task = None;
                CommandResultSuspensionPoll::Ready(result.map(|result| {
                    let count = result.banned.len().min(i32::MAX as usize) as i32;
                    for (name, reason) in result.banned {
                        let message = translations::COMMANDS_BAN_SUCCESS
                            .message([TextComponent::plain(name), reason])
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
                    "ban command task ended without a result",
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
    fn ban_targets_use_vanillas_game_profile_argument_and_optional_reason() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let Some(ban) = dispatcher.children(dispatcher.root()).and_then(|children| {
            children.iter().copied().find(|child| {
                dispatcher
                    .node(*child)
                    .is_some_and(|node| node.name() == "ban")
            })
        }) else {
            panic!("ban root should exist");
        };
        let Some(targets) = dispatcher
            .children(ban)
            .and_then(|children| children.first())
        else {
            panic!("ban targets should exist");
        };
        assert!(matches!(
            dispatcher.node(*targets),
            Some(node)
                if node.is_executable()
                    && node.argument_type() == Some(&SteelArgumentType::game_profile())
        ));

        let Some(reason) = dispatcher
            .children(*targets)
            .and_then(|children| children.first())
        else {
            panic!("ban reason should exist");
        };
        assert!(matches!(
            dispatcher.node(*reason),
            Some(node)
                if node.is_executable()
                    && node.argument_type() == Some(&SteelArgumentType::message())
        ));
    }
}
