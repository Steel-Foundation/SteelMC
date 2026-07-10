//! Handler for the "gamemode" command.
use crate::command::arguments::gamemode::GameModeArgument;
use crate::command::arguments::player::PlayerArgument;
use crate::command::commands::{
    CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn, argument,
};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use crate::entity::Entity;
use crate::player::Player;
use std::sync::Arc;
use steel_registry::game_rules::GameRuleValue;
use steel_registry::vanilla_game_rules::SEND_COMMAND_FEEDBACK;
use steel_utils::translations;
use steel_utils::types::GameType;
use text_components::TextComponent;
use text_components::translation::Translation;

/// Handler for the "gamemode" command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["gamemode"],
        "Sets the game mode.",
        "minecraft:command.gamemode",
    )
    .then(
        argument("gamemode", GameModeArgument)
            .executes(GameModeCommandExecutor)
            .then(
                argument("targets", PlayerArgument::multiple())
                    .executes(GameModeTargetCommandExecutor),
            ),
    )
}

struct GameModeCommandExecutor;

impl CommandExecutor<((), GameType)> for GameModeCommandExecutor {
    fn execute(
        &self,
        args: ((), GameType),
        context: &mut CommandContext,
    ) -> Result<(), CommandError> {
        let ((), gamemode) = args;

        // Get the player executing the command
        let player = context
            .sender
            .get_player()
            .ok_or(CommandError::InvalidRequirement)?;

        if player.set_game_mode(gamemode) {
            context.sender.send_message(
                &translations::COMMANDS_GAMEMODE_SUCCESS_SELF
                    .message([TextComponent::from(get_gamemode_translation(gamemode))])
                    .into(),
            );
        }

        Ok(())
    }
}

struct GameModeTargetCommandExecutor;

impl CommandExecutor<(((), GameType), Vec<Arc<Player>>)> for GameModeTargetCommandExecutor {
    fn execute(
        &self,
        args: (((), GameType), Vec<Arc<Player>>),
        context: &mut CommandContext,
    ) -> Result<(), CommandError> {
        let (((), gamemode), targets) = args;

        let mode_translation = get_gamemode_translation(gamemode);

        for target in targets {
            if !target.set_game_mode(gamemode) {
                continue;
            }

            let sender_is_target = context
                .sender
                .get_player()
                .is_some_and(|sender| sender.id() == target.id());
            if sender_is_target {
                context.sender.send_message(
                    &translations::COMMANDS_GAMEMODE_SUCCESS_SELF
                        .message([TextComponent::from(mode_translation)])
                        .into(),
                );
                continue;
            }

            if context.world.get_game_rule(&SEND_COMMAND_FEEDBACK) == GameRuleValue::Bool(true) {
                target.send_message(
                    &translations::GAME_MODE_CHANGED
                        .message([TextComponent::from(mode_translation)])
                        .into(),
                );
            }
            context.sender.send_message(
                &translations::COMMANDS_GAMEMODE_SUCCESS_OTHER
                    .message([
                        TextComponent::plain(target.plain_text_name()),
                        TextComponent::from(mode_translation),
                    ])
                    .into(),
            );
        }

        Ok(())
    }
}

/// Retrieves the translation for a `GameType`
#[must_use]
pub fn get_gamemode_translation(gamemode: GameType) -> &'static Translation<0> {
    match gamemode {
        GameType::Survival => &translations::GAME_MODE_SURVIVAL,
        GameType::Creative => &translations::GAME_MODE_CREATIVE,
        GameType::Adventure => &translations::GAME_MODE_ADVENTURE,
        GameType::Spectator => &translations::GAME_MODE_SPECTATOR,
    }
}
