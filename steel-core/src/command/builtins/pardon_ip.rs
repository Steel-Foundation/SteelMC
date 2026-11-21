//! Vanilla IP-unbanning command.

use std::net::IpAddr;

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

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("pardon-ip"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("pardon-ip")
        .then(argument("target", SteelArgumentType::word()).executes_suspended(start_pardon_ip))
}

fn start_pardon_ip(
    context: &SteelCommandContext<CommandSource>,
) -> Result<PardonIpCommandSuspension, CommandSyntaxError> {
    let target = context.word("target")?.to_owned();
    let source = context.source().clone();
    let task_source = source.clone();
    let (sender, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let result = run_pardon_ip(&task_source, target).await;
        let _ = sender.send(result);
    });
    Ok(PardonIpCommandSuspension {
        source,
        receiver,
        task: Some(task),
    })
}

async fn run_pardon_ip(
    source: &CommandSource,
    target: String,
) -> Result<String, CommandSyntaxError> {
    if target.parse::<IpAddr>().is_err() {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_PARDONIP_INVALID,
        )));
    }

    let removed = source
        .server()
        .ip_ban_list
        .remove(&target)
        .await
        .map_err(|error| CommandSyntaxError::dynamic(error.to_string()))?;
    if !removed {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_PARDONIP_FAILED,
        )));
    }
    Ok(target)
}

struct PardonIpCommandSuspension {
    source: CommandSource,
    receiver: oneshot::Receiver<Result<String, CommandSyntaxError>>,
    task: Option<JoinHandle<()>>,
}

impl CommandResultSuspension for PardonIpCommandSuspension {
    fn order(&self) -> CommandSuspensionOrder {
        CommandSuspensionOrder::Global
    }

    fn poll(&mut self) -> CommandResultSuspensionPoll {
        match self.receiver.try_recv() {
            Ok(result) => {
                self.task = None;
                CommandResultSuspensionPoll::Ready(result.map(|ip| {
                    let message = translations::COMMANDS_PARDONIP_SUCCESS
                        .message([TextComponent::plain(ip)])
                        .component();
                    self.source.send_success(&message, true);
                    1
                }))
            }
            Err(oneshot::error::TryRecvError::Empty) => CommandResultSuspensionPoll::Pending,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.task = None;
                CommandResultSuspensionPoll::Ready(Err(CommandSyntaxError::dynamic(
                    "pardon-ip command task ended without a result",
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
    fn pardon_ip_target_is_a_word() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let Some(pardon_ip) = dispatcher.children(dispatcher.root()).and_then(|children| {
            children.iter().copied().find(|child| {
                dispatcher
                    .node(*child)
                    .is_some_and(|node| node.name() == "pardon-ip")
            })
        }) else {
            panic!("pardon-ip root should exist");
        };
        let Some(target) = dispatcher
            .children(pardon_ip)
            .and_then(|children| children.first())
        else {
            panic!("pardon-ip target should exist");
        };
        assert!(matches!(
            dispatcher.node(*target),
            Some(node)
                if node.is_executable()
                    && node.argument_type() == Some(&SteelArgumentType::word())
        ));
    }
}
