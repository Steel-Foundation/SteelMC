//! Vanilla IP-banning command.
//!
//! Unlike vanilla's `BanIpCommands`, the target must be a literal IP address:
//! Steel doesn't currently track a connected player's remote address past
//! login, so resolving a player name to their current IP (and kicking every
//! other player sharing a freshly banned IP) isn't possible yet.

use std::net::IpAddr;

use chrono::Utc;
use steel_utils::{Identifier, translations};
use text_components::TextComponent;
use tokio::{sync::oneshot, task::JoinHandle};

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandResultSuspension, CommandResultSuspensionPoll, CommandSource,
        CommandSuspensionOrder, SteelArgumentType, SteelCommandContext, SteelCommandRuntime,
        argument, literal,
    },
    registration::CommandRegistration,
};
use crate::ban::IpBanEntry;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("ban-ip"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("ban-ip").then(
        argument("target", SteelArgumentType::word())
            .executes_suspended(ban_ip_without_reason)
            .then(
                argument("reason", SteelArgumentType::message())
                    .executes_suspended(ban_ip_with_reason),
            ),
    )
}

fn ban_ip_without_reason(
    context: &SteelCommandContext<CommandSource>,
) -> Result<BanIpCommandSuspension, CommandSyntaxError> {
    start_ban_ip(context, None)
}

fn ban_ip_with_reason(
    context: &SteelCommandContext<CommandSource>,
) -> Result<BanIpCommandSuspension, CommandSyntaxError> {
    let reason = context.message("reason")?.to_owned();
    start_ban_ip(context, Some(reason))
}

fn start_ban_ip(
    context: &SteelCommandContext<CommandSource>,
    reason: Option<String>,
) -> Result<BanIpCommandSuspension, CommandSyntaxError> {
    let target = context.word("target")?.to_owned();
    let source = context.source().clone();
    let task_source = source.clone();
    let (sender, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let result = run_ban_ip(&task_source, target, reason).await;
        let _ = sender.send(result);
    });
    Ok(BanIpCommandSuspension {
        source,
        receiver,
        task: Some(task),
    })
}

struct BanIpCommandResult {
    ip: String,
    reason: TextComponent,
}

async fn run_ban_ip(
    source: &CommandSource,
    target: String,
    reason: Option<String>,
) -> Result<BanIpCommandResult, CommandSyntaxError> {
    if target.parse::<IpAddr>().is_err() {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_BANIP_INVALID,
        )));
    }
    if source.server().ip_ban_list.is_banned(&target) {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_BANIP_FAILED,
        )));
    }

    let entry = IpBanEntry {
        ip: target.clone(),
        created: Utc::now(),
        source: source.sender().to_string(),
        expires: None,
        reason,
    };
    let reason_message = entry.reason_message();
    source
        .server()
        .ip_ban_list
        .add(entry)
        .await
        .map_err(|error| CommandSyntaxError::dynamic(error.to_string()))?;

    Ok(BanIpCommandResult {
        ip: target,
        reason: reason_message,
    })
}

struct BanIpCommandSuspension {
    source: CommandSource,
    receiver: oneshot::Receiver<Result<BanIpCommandResult, CommandSyntaxError>>,
    task: Option<JoinHandle<()>>,
}

impl CommandResultSuspension for BanIpCommandSuspension {
    fn order(&self) -> CommandSuspensionOrder {
        CommandSuspensionOrder::Global
    }

    fn poll(&mut self) -> CommandResultSuspensionPoll {
        match self.receiver.try_recv() {
            Ok(result) => {
                self.task = None;
                CommandResultSuspensionPoll::Ready(result.map(|result| {
                    let message = translations::COMMANDS_BANIP_SUCCESS
                        .message([TextComponent::plain(result.ip), result.reason])
                        .component();
                    self.source.send_success(&message, true);
                    1
                }))
            }
            Err(oneshot::error::TryRecvError::Empty) => CommandResultSuspensionPoll::Pending,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.task = None;
                CommandResultSuspensionPoll::Ready(Err(CommandSyntaxError::dynamic(
                    "ban-ip command task ended without a result",
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
    fn ban_ip_target_is_a_word_with_an_optional_reason() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let Some(ban_ip) = dispatcher.children(dispatcher.root()).and_then(|children| {
            children.iter().copied().find(|child| {
                dispatcher
                    .node(*child)
                    .is_some_and(|node| node.name() == "ban-ip")
            })
        }) else {
            panic!("ban-ip root should exist");
        };
        let Some(target) = dispatcher
            .children(ban_ip)
            .and_then(|children| children.first())
        else {
            panic!("ban-ip target should exist");
        };
        assert!(matches!(
            dispatcher.node(*target),
            Some(node)
                if node.is_executable()
                    && node.argument_type() == Some(&SteelArgumentType::word())
        ));

        let Some(reason) = dispatcher
            .children(*target)
            .and_then(|children| children.first())
        else {
            panic!("ban-ip reason should exist");
        };
        assert!(matches!(
            dispatcher.node(*reason),
            Some(node)
                if node.is_executable()
                    && node.argument_type() == Some(&SteelArgumentType::message())
        ));
    }
}
