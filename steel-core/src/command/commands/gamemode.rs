//! Handler for the "gamemode" command.
use crate::command::arguments::gamemode::GameModeArgument;
use crate::command::arguments::player::PlayerArgument;
use crate::command::commands::{
    CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn, argument,
};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use crate::player::Player;
use std::sync::Arc;
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
                argument("targets", PlayerArgument::new()).executes(GameModeTargetCommandExecutor),
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

        // Set the player's game mode
        if !player.set_game_mode(gamemode) {
            // Player was already in the requested game mode
            return Ok(());
        }

        // Send success message
        context.sender.send_message(
            &translations::COMMANDS_GAMEMODE_SUCCESS_SELF
                .message([get_gamemode_translation(gamemode)])
                .into(),
        );

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
            if target.set_game_mode(gamemode) {
                // Send message to target
                target.send_message(
                    &translations::COMMANDS_GAMEMODE_SUCCESS_SELF
                        .message([mode_translation])
                        .into(),
                );

                // Send feedback to sender if sender is not the target
                let sender_is_target = if let Some(sender_player) = context.sender.get_player() {
                    sender_player.id == target.id
                } else {
                    false
                };

                if !sender_is_target {
                    context.sender.send_message(
                        &translations::COMMANDS_GAMEMODE_SUCCESS_OTHER
                            .message([
                                TextComponent::plain(target.gameprofile.name.clone()),
                                TextComponent::from(mode_translation),
                            ])
                            .into(),
                    );
                }
            }
        }

        Ok(())
    }
}

fn get_gamemode_translation(gamemode: GameType) -> &'static Translation<0> {
    match gamemode {
        GameType::Survival => &translations::GAME_MODE_SURVIVAL,
        GameType::Creative => &translations::GAME_MODE_CREATIVE,
        GameType::Adventure => &translations::GAME_MODE_ADVENTURE,
        GameType::Spectator => &translations::GAME_MODE_SPECTATOR,
    }
}
