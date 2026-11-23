//! Vanilla kick command.

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::entity::Entity as _;
use steel_utils::{Identifier, translations};
use text_components::TextComponent;
use text_components::content::Content;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("kick"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("kick").then(
        argument("targets", SteelArgumentType::players())
            .executes(kick_players)
            .then(argument("reason", SteelArgumentType::message()).executes(kick_players)),
    )
}

fn kick_players(ctx: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let mut count: i32 = 0;

    let targets = ctx.players("targets")?;
    let message = match ctx.message("reason") {
        Ok(message) => {
            let parsed = message.resolve_component(ctx.source())?;

            if let Content::Text { text } = &parsed.content {
                TextComponent::from_snbt(text).unwrap_or(parsed)
            } else {
                parsed
            }
        }
        Err(_) => translations::MULTIPLAYER_DISCONNECT_KICKED
            .msg()
            .component(),
    };

    for players in targets {
        players.disconnect(message.clone());
        ctx.source().send_success(
            &translations::COMMANDS_KICK_SUCCESS
                .message([players.display_name(), message.clone()])
                .component(),
            true,
        );
        count += 1;
    }

    Ok(count)
}
