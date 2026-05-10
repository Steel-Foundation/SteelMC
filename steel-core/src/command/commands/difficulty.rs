//! Handler for the "difficulty" command

use crate::command::commands::{
    CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn, literal,
};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use steel_protocol::packets::game::CChangeDifficulty;
use steel_utils::translations;
use steel_utils::types::Difficulty;
use text_components::TextComponent;
use text_components::translation::Translation;

/// Handler for the "difficulty" command
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["difficulty"],
        "Gets or sets the world difficulty.",
        "minecraft:command.difficulty",
    )
    .executes(QueryExecutor)
    .then(literal("peaceful").executes(SetExecutor(Difficulty::Peaceful)))
    .then(literal("easy").executes(SetExecutor(Difficulty::Easy)))
    .then(literal("normal").executes(SetExecutor(Difficulty::Normal)))
    .then(literal("hard").executes(SetExecutor(Difficulty::Hard)))
}

/// Returns the string key for a [`Difficulty`] variant
const fn difficulty_key(difficulty: Difficulty) -> &'static str {
    match difficulty {
        Difficulty::Peaceful => "peaceful",
        Difficulty::Easy => "easy",
        Difficulty::Normal => "normal",
        Difficulty::Hard => "hard",
    }
}

/// Returns the translatable display name for a [`Difficulty`] variant
fn difficulty_display_name(difficulty: Difficulty) -> &'static Translation<0> {
    match difficulty {
        Difficulty::Peaceful => &translations::OPTIONS_DIFFICULTY_PEACEFUL,
        Difficulty::Easy => &translations::OPTIONS_DIFFICULTY_EASY,
        Difficulty::Normal => &translations::OPTIONS_DIFFICULTY_NORMAL,
        Difficulty::Hard => &translations::OPTIONS_DIFFICULTY_HARD,
    }
}

/// Queries the current world difficulty
struct QueryExecutor;

impl CommandExecutor<()> for QueryExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let difficulty = context.world.level_data.read().data().difficulty;
        let display_name = difficulty_display_name(difficulty);

        context.sender.send_message(
            &translations::COMMANDS_DIFFICULTY_QUERY
                .message([TextComponent::from(display_name)])
                .into(),
        );

        Ok(())
    }
}

/// Sets the world difficulty to the specified value
struct SetExecutor(Difficulty);

impl CommandExecutor<()> for SetExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let difficulty = self.0;

        let domain = context.world.domain().to_owned();
        let worlds = context.server.worlds.worlds_in_domain(&domain);

        if worlds
            .iter()
            .all(|world| world.level_data.read().data().difficulty == difficulty)
        {
            return Err(CommandError::CommandFailed(Box::new(
                translations::COMMANDS_DIFFICULTY_FAILURE
                    .message([TextComponent::plain(difficulty_key(difficulty))])
                    .into(),
            )));
        }

        for world in worlds {
            let mut level_data = world.level_data.write();
            level_data.data_mut().difficulty = difficulty;
            let locked = level_data.data().difficulty_locked;
            drop(level_data);

            world.broadcast_to_all(CChangeDifficulty { difficulty, locked });
        }

        let display_name = difficulty_display_name(difficulty);
        context.sender.send_message(
            &translations::COMMANDS_DIFFICULTY_SUCCESS
                .message([TextComponent::from(display_name)])
                .into(),
        );

        Ok(())
    }
}
