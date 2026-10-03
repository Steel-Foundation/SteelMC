//! Vanilla ban-list-listing command.

use steel_utils::{Identifier, translations};
use text_components::TextComponent;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{CommandSource, SteelCommandContext, SteelCommandRuntime, literal},
    registration::CommandRegistration,
};
use crate::ban::{BanEntry, IpBanEntry};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("banlist"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("banlist")
        .executes(banlist_all)
        .then(literal("ips").executes(banlist_ips))
        .then(literal("players").executes(banlist_players))
}

fn banlist_all(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let server = context.source().server();
    let mut rows = player_rows(&server.ban_list.entries());
    rows.extend(ip_rows(&server.ip_ban_list.entries()));
    show_list(context, rows)
}

fn banlist_ips(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let rows = ip_rows(&context.source().server().ip_ban_list.entries());
    show_list(context, rows)
}

fn banlist_players(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let rows = player_rows(&context.source().server().ban_list.entries());
    show_list(context, rows)
}

fn player_rows(entries: &[BanEntry]) -> Vec<(String, String, TextComponent)> {
    entries
        .iter()
        .map(|entry| {
            (
                entry.name.clone(),
                entry.source.clone(),
                entry.reason_message(),
            )
        })
        .collect()
}

fn ip_rows(entries: &[IpBanEntry]) -> Vec<(String, String, TextComponent)> {
    entries
        .iter()
        .map(|entry| {
            (
                entry.ip.clone(),
                entry.source.clone(),
                entry.reason_message(),
            )
        })
        .collect()
}

fn show_list(
    context: &SteelCommandContext<CommandSource>,
    rows: Vec<(String, String, TextComponent)>,
) -> Result<i32, CommandSyntaxError> {
    let Ok(count) = i32::try_from(rows.len()) else {
        return Err(CommandSyntaxError::dynamic(
            "Ban list entry count exceeds the command result range",
        ));
    };

    if rows.is_empty() {
        context.source().send_success(
            &TextComponent::from(&translations::COMMANDS_BANLIST_NONE),
            false,
        );
        return Ok(0);
    }

    let header = translations::COMMANDS_BANLIST_LIST
        .message([TextComponent::plain(count.to_string())])
        .component();
    context.source().send_success(&header, false);

    for (display_name, source, reason) in rows {
        let line = translations::COMMANDS_BANLIST_ENTRY
            .message([
                TextComponent::plain(display_name),
                TextComponent::plain(source),
                reason,
            ])
            .component();
        context.source().send_success(&line, false);
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use super::super::create_dispatcher;

    #[test]
    fn banlist_graph_supports_the_bare_ips_and_players_forms() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let Some(banlist) = dispatcher.children(dispatcher.root()).and_then(|children| {
            children.iter().copied().find(|child| {
                dispatcher
                    .node(*child)
                    .is_some_and(|node| node.name() == "banlist")
            })
        }) else {
            panic!("banlist root should exist");
        };
        let Some(banlist_node) = dispatcher.node(banlist) else {
            panic!("banlist root node should exist");
        };
        assert!(banlist_node.is_executable());

        let mut subcommand_names = Vec::new();
        for child in dispatcher.children(banlist).into_iter().flatten() {
            if let Some(node) = dispatcher.node(*child) {
                subcommand_names.push(node.name());
            }
        }
        assert!(subcommand_names.contains(&"ips"));
        assert!(subcommand_names.contains(&"players"));
    }
}
