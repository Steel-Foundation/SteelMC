use steel_utils::{
    Identifier,
    translations::{COMMANDS_LIST_NAME_AND_ID, COMMANDS_LIST_PLAYERS},
};

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{CommandSource, SteelCommandContext, SteelCommandRuntime, literal},
    registration::CommandRegistration,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("list"), |_| command()).default_access()
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("list")
        .executes(|context| list_players(context, false))
        .then(literal("uuids").executes(|context| list_players(context, true)))
}

fn list_players(
    context: &SteelCommandContext<CommandSource>,
    show_uuids: bool,
) -> Result<i32, CommandSyntaxError> {
    let player_count = context.source().server().player_count();
    let Ok(result) = i32::try_from(player_count) else {
        return Err(CommandSyntaxError::dynamic(
            "Online player count exceeds the command result range",
        ));
    };
    let max_players = context.source().server().config.max_players;
    let formatted_players = context
        .source()
        .server()
        .get_players()
        .iter()
        .map(|player| {
            if show_uuids {
                COMMANDS_LIST_NAME_AND_ID
                    .message([
                        player.gameprofile.name.clone(),
                        player.gameprofile.id.to_string(),
                    ])
                    .component()
                    .to_string()
            } else {
                player.gameprofile.name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let message = COMMANDS_LIST_PLAYERS
        .message([
            player_count.to_string(),
            max_players.to_string(),
            formatted_players,
        ])
        .component();
    context.source().send_success(&message, false);
    Ok(result)
}
