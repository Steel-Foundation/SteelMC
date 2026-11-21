//! Vanilla whitelist command.

use std::sync::Arc;

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
use crate::{entity::Entity as _, server::Server, whitelist::WhitelistEntry};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("whitelist"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("whitelist")
        .then(literal("on").executes_suspended(whitelist_on))
        .then(literal("off").executes_suspended(whitelist_off))
        .then(literal("list").executes(whitelist_list))
        .then(
            literal("add").then(
                argument("targets", SteelArgumentType::game_profile())
                    .executes_suspended(whitelist_add),
            ),
        )
        .then(
            literal("remove").then(
                argument("targets", SteelArgumentType::game_profile())
                    .executes_suspended(whitelist_remove),
            ),
        )
        .then(literal("reload").executes_suspended(whitelist_reload))
}

/// Disconnects online players who are no longer whitelisted, matching
/// vanilla's `PlayerList.kickUnlistedPlayers`. A no-op while the whitelist
/// isn't enabled.
fn kick_unlisted_players(server: &Server) {
    if !server.whitelist.is_enabled() {
        return;
    }
    for player in server.online_players_snapshot() {
        if !server.whitelist.is_whitelisted(player.uuid()) {
            player.disconnect(TextComponent::from(
                &translations::MULTIPLAYER_DISCONNECT_NOT_WHITELISTED,
            ));
        }
    }
}

struct WhitelistActionSuspension {
    source: CommandSource,
    receiver: oneshot::Receiver<Result<TextComponent, CommandSyntaxError>>,
    task: Option<JoinHandle<()>>,
}

impl CommandResultSuspension for WhitelistActionSuspension {
    fn order(&self) -> CommandSuspensionOrder {
        CommandSuspensionOrder::Global
    }

    fn poll(&mut self) -> CommandResultSuspensionPoll {
        match self.receiver.try_recv() {
            Ok(result) => {
                self.task = None;
                CommandResultSuspensionPoll::Ready(result.map(|message| {
                    self.source.send_success(&message, true);
                    1
                }))
            }
            Err(oneshot::error::TryRecvError::Empty) => CommandResultSuspensionPoll::Pending,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.task = None;
                CommandResultSuspensionPoll::Ready(Err(CommandSyntaxError::dynamic(
                    "whitelist command task ended without a result",
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

fn whitelist_on(
    context: &SteelCommandContext<CommandSource>,
) -> Result<WhitelistActionSuspension, CommandSyntaxError> {
    let server = Arc::clone(context.source().server());
    if server.whitelist.is_enabled() {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_WHITELIST_ALREADY_ON,
        )));
    }
    let source = context.source().clone();
    let (sender, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let result = server
            .whitelist
            .set_enabled(true)
            .await
            .map(|()| {
                kick_unlisted_players(&server);
                TextComponent::from(&translations::COMMANDS_WHITELIST_ENABLED)
            })
            .map_err(|error| CommandSyntaxError::dynamic(error.to_string()));
        let _ = sender.send(result);
    });
    Ok(WhitelistActionSuspension {
        source,
        receiver,
        task: Some(task),
    })
}

fn whitelist_off(
    context: &SteelCommandContext<CommandSource>,
) -> Result<WhitelistActionSuspension, CommandSyntaxError> {
    let server = Arc::clone(context.source().server());
    if !server.whitelist.is_enabled() {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_WHITELIST_ALREADY_OFF,
        )));
    }
    let source = context.source().clone();
    let (sender, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let result = server
            .whitelist
            .set_enabled(false)
            .await
            .map(|()| TextComponent::from(&translations::COMMANDS_WHITELIST_DISABLED))
            .map_err(|error| CommandSyntaxError::dynamic(error.to_string()));
        let _ = sender.send(result);
    });
    Ok(WhitelistActionSuspension {
        source,
        receiver,
        task: Some(task),
    })
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Command executors use a shared fallible callback signature."
)]
fn whitelist_reload(
    context: &SteelCommandContext<CommandSource>,
) -> Result<WhitelistActionSuspension, CommandSyntaxError> {
    let server = Arc::clone(context.source().server());
    let source = context.source().clone();
    let (sender, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let result = server
            .whitelist
            .reload()
            .await
            .map(|()| {
                kick_unlisted_players(&server);
                TextComponent::from(&translations::COMMANDS_WHITELIST_RELOADED)
            })
            .map_err(|error| CommandSyntaxError::dynamic(error.to_string()));
        let _ = sender.send(result);
    });
    Ok(WhitelistActionSuspension {
        source,
        receiver,
        task: Some(task),
    })
}

fn whitelist_list(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let entries = context.source().server().whitelist.entries();
    let Ok(count) = i32::try_from(entries.len()) else {
        return Err(CommandSyntaxError::dynamic(
            "Whitelist entry count exceeds the command result range",
        ));
    };

    let message = if entries.is_empty() {
        TextComponent::from(&translations::COMMANDS_WHITELIST_NONE)
    } else {
        let names = entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        translations::COMMANDS_WHITELIST_LIST
            .message([
                TextComponent::plain(count.to_string()),
                TextComponent::plain(names),
            ])
            .component()
    };
    context.source().send_success(&message, false);
    Ok(count)
}

struct WhitelistTargetsSuspension {
    source: CommandSource,
    receiver: oneshot::Receiver<Result<Vec<String>, CommandSyntaxError>>,
    task: Option<JoinHandle<()>>,
    success: fn(String) -> TextComponent,
}

impl CommandResultSuspension for WhitelistTargetsSuspension {
    fn order(&self) -> CommandSuspensionOrder {
        CommandSuspensionOrder::Global
    }

    fn poll(&mut self) -> CommandResultSuspensionPoll {
        match self.receiver.try_recv() {
            Ok(result) => {
                self.task = None;
                CommandResultSuspensionPoll::Ready(result.map(|names| {
                    let count = names.len().min(i32::MAX as usize) as i32;
                    for name in names {
                        self.source.send_success(&(self.success)(name), true);
                    }
                    count
                }))
            }
            Err(oneshot::error::TryRecvError::Empty) => CommandResultSuspensionPoll::Pending,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.task = None;
                CommandResultSuspensionPoll::Ready(Err(CommandSyntaxError::dynamic(
                    "whitelist command task ended without a result",
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

fn whitelist_add(
    context: &SteelCommandContext<CommandSource>,
) -> Result<WhitelistTargetsSuspension, CommandSyntaxError> {
    let argument = context.game_profile_argument("targets").cloned()?;
    let source = context.source().clone();
    let task_source = source.clone();
    let (sender, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let result = run_whitelist_add(&task_source, argument).await;
        let _ = sender.send(result);
    });
    Ok(WhitelistTargetsSuspension {
        source,
        receiver,
        task: Some(task),
        success: |name| {
            translations::COMMANDS_WHITELIST_ADD_SUCCESS
                .message([TextComponent::plain(name)])
                .component()
        },
    })
}

async fn run_whitelist_add(
    source: &CommandSource,
    argument: GameProfileArgument,
) -> Result<Vec<String>, CommandSyntaxError> {
    let targets = argument.resolve(source).await?;
    let mut added = Vec::new();

    for target in targets {
        let entry = WhitelistEntry {
            uuid: target.uuid,
            name: target.name.clone(),
        };
        let newly_added = source
            .server()
            .whitelist
            .add(entry)
            .await
            .map_err(|error| CommandSyntaxError::dynamic(error.to_string()))?;
        if newly_added {
            added.push(target.name);
        }
    }

    if added.is_empty() {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_WHITELIST_ADD_FAILED,
        )));
    }
    Ok(added)
}

fn whitelist_remove(
    context: &SteelCommandContext<CommandSource>,
) -> Result<WhitelistTargetsSuspension, CommandSyntaxError> {
    let argument = context.game_profile_argument("targets").cloned()?;
    let source = context.source().clone();
    let task_source = source.clone();
    let (sender, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let result = run_whitelist_remove(&task_source, argument).await;
        let _ = sender.send(result);
    });
    Ok(WhitelistTargetsSuspension {
        source,
        receiver,
        task: Some(task),
        success: |name| {
            translations::COMMANDS_WHITELIST_REMOVE_SUCCESS
                .message([TextComponent::plain(name)])
                .component()
        },
    })
}

async fn run_whitelist_remove(
    source: &CommandSource,
    argument: GameProfileArgument,
) -> Result<Vec<String>, CommandSyntaxError> {
    let targets = argument.resolve(source).await?;
    let mut removed = Vec::new();

    for target in targets {
        let was_removed = source
            .server()
            .whitelist
            .remove(target.uuid)
            .await
            .map_err(|error| CommandSyntaxError::dynamic(error.to_string()))?;
        if was_removed {
            removed.push(target.name);
        }
    }

    if removed.is_empty() {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_WHITELIST_REMOVE_FAILED,
        )));
    }
    kick_unlisted_players(source.server());
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use super::super::create_dispatcher;
    use crate::command::execution::SteelArgumentType;

    #[test]
    fn whitelist_graph_supports_every_vanilla_subcommand() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let Some(whitelist) = dispatcher.children(dispatcher.root()).and_then(|children| {
            children.iter().copied().find(|child| {
                dispatcher
                    .node(*child)
                    .is_some_and(|node| node.name() == "whitelist")
            })
        }) else {
            panic!("whitelist root should exist");
        };

        let mut subcommand_names = Vec::new();
        for child in dispatcher.children(whitelist).into_iter().flatten() {
            if let Some(node) = dispatcher.node(*child) {
                subcommand_names.push(node.name());
            }
        }
        for expected in ["on", "off", "list", "add", "remove", "reload"] {
            assert!(
                subcommand_names.contains(&expected),
                "missing whitelist subcommand {expected}"
            );
        }

        let Some(add) = dispatcher
            .children(whitelist)
            .into_iter()
            .flatten()
            .find(|child| {
                dispatcher
                    .node(**child)
                    .is_some_and(|node| node.name() == "add")
            })
        else {
            panic!("whitelist add should exist");
        };
        let Some(add_targets) = dispatcher
            .children(*add)
            .and_then(|children| children.first())
        else {
            panic!("whitelist add targets should exist");
        };
        assert!(matches!(
            dispatcher.node(*add_targets),
            Some(node)
                if node.is_executable()
                    && node.argument_type() == Some(&SteelArgumentType::game_profile())
        ));
    }
}
