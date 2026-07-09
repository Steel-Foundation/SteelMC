use steel_protocol::packets::game::CChangeDifficulty;
use steel_utils::{Identifier, translations, types::Difficulty};
use text_components::{TextComponent, translation::Translation};

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{CommandSource, SteelCommandContext, SteelCommandRuntime, literal},
    registration::CommandRegistration,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("difficulty"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("difficulty")
        .executes(query_difficulty)
        .then(difficulty_literal("peaceful", Difficulty::Peaceful))
        .then(difficulty_literal("easy", Difficulty::Easy))
        .then(difficulty_literal("normal", Difficulty::Normal))
        .then(difficulty_literal("hard", Difficulty::Hard))
}

fn difficulty_literal(
    name: &'static str,
    difficulty: Difficulty,
) -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal(name).executes(move |context| set_difficulty(context, difficulty))
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Command executors use a shared fallible callback signature."
)]
fn query_difficulty(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let difficulty = context.source().world().level_data.read().data().difficulty;
    let message = translations::COMMANDS_DIFFICULTY_QUERY
        .message([TextComponent::from(difficulty_display_name(difficulty))])
        .component();
    context.source().send_success(&message);
    Ok(i32::from(u8::from(difficulty)))
}

fn set_difficulty(
    context: &SteelCommandContext<CommandSource>,
    difficulty: Difficulty,
) -> Result<i32, CommandSyntaxError> {
    let domain = context.source().world().domain();
    let worlds = context.source().server().worlds.worlds_in_domain(domain);
    if worlds
        .iter()
        .all(|world| world.level_data.read().data().difficulty == difficulty)
    {
        return Err(CommandSyntaxError::dynamic(
            translations::COMMANDS_DIFFICULTY_FAILURE
                .message([TextComponent::from(difficulty_display_name(difficulty))])
                .component(),
        ));
    }

    for world in worlds {
        let mut level_data = world.level_data.write();
        level_data.data_mut().difficulty = difficulty;
        let locked = level_data.data().difficulty_locked;
        drop(level_data);
        world.broadcast_to_all(CChangeDifficulty { difficulty, locked });
    }

    let message = translations::COMMANDS_DIFFICULTY_SUCCESS
        .message([TextComponent::from(difficulty_display_name(difficulty))])
        .component();
    context.source().send_success(&message);
    Ok(0)
}

const fn difficulty_display_name(difficulty: Difficulty) -> &'static Translation<0> {
    match difficulty {
        Difficulty::Peaceful => &translations::OPTIONS_DIFFICULTY_PEACEFUL,
        Difficulty::Easy => &translations::OPTIONS_DIFFICULTY_EASY,
        Difficulty::Normal => &translations::OPTIONS_DIFFICULTY_NORMAL,
        Difficulty::Hard => &translations::OPTIONS_DIFFICULTY_HARD,
    }
}
