//! `/execute if` and `/execute unless` conditions.

use steel_utils::translations;
use text_components::TextComponent;

use super::super::super::{
    brigadier::{CommandNodeBuilder, CommandRedirectTarget, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
};

type Builder = CommandNodeBuilder<CommandSource, SteelCommandRuntime>;

const EXECUTE_ROOT: CommandRedirectTarget = CommandRedirectTarget::CommandRoot;

pub(super) fn conditionals(name: &'static str, expected: bool) -> Builder {
    literal(name)
        .then(entity_condition(expected))
        .then(loaded_condition(expected))
}

fn entity_condition(expected: bool) -> Builder {
    literal("entity").then(
        argument("entities", SteelArgumentType::entities())
            .forks(EXECUTE_ROOT, move |context| {
                let matches = !context.optional_entities("entities")?.is_empty();
                Ok(conditional_sources(context.source(), expected, matches))
            })
            .executes(move |context| {
                let count =
                    i32::try_from(context.optional_entities("entities")?.len()).map_err(|_| {
                        CommandSyntaxError::dynamic("Entity count exceeds the command result range")
                    })?;
                execute_numeric_condition(context, expected, count)
            }),
    )
}

fn loaded_condition(expected: bool) -> Builder {
    literal("loaded").then(
        argument("pos", SteelArgumentType::block_pos())
            .forks(EXECUTE_ROOT, move |context| {
                let matches = loaded_matches(context)?;
                Ok(conditional_sources(context.source(), expected, matches))
            })
            .executes(move |context| {
                execute_boolean_condition(context, expected, loaded_matches(context)?)
            }),
    )
}

fn loaded_matches(
    context: &SteelCommandContext<CommandSource>,
) -> Result<bool, CommandSyntaxError> {
    let position = context
        .coordinates("pos")
        .ok_or_else(|| missing_argument("pos"))?
        .block_pos(context.source());
    Ok(context
        .source()
        .world()
        .is_entity_ticking_chunk_loaded(position))
}

fn conditional_sources(
    source: &CommandSource,
    expected: bool,
    matches: bool,
) -> Vec<CommandSource> {
    if matches == expected {
        vec![source.clone()]
    } else {
        Vec::new()
    }
}

fn execute_boolean_condition(
    context: &SteelCommandContext<CommandSource>,
    expected: bool,
    matches: bool,
) -> Result<i32, CommandSyntaxError> {
    if matches != expected {
        return Err(conditional_failed());
    }
    context.source().send_success(&TextComponent::from(
        &translations::COMMANDS_EXECUTE_CONDITIONAL_PASS,
    ));
    Ok(1)
}

fn execute_numeric_condition(
    context: &SteelCommandContext<CommandSource>,
    expected: bool,
    count: i32,
) -> Result<i32, CommandSyntaxError> {
    if expected {
        if count == 0 {
            return Err(conditional_failed());
        }
        let message = translations::COMMANDS_EXECUTE_CONDITIONAL_PASS_COUNT
            .message([TextComponent::from(count.to_string())])
            .component();
        context.source().send_success(&message);
        return Ok(count);
    }

    if count != 0 {
        let message = translations::COMMANDS_EXECUTE_CONDITIONAL_FAIL_COUNT
            .message([TextComponent::from(count.to_string())])
            .component();
        return Err(CommandSyntaxError::dynamic(message));
    }
    context.source().send_success(&TextComponent::from(
        &translations::COMMANDS_EXECUTE_CONDITIONAL_PASS,
    ));
    Ok(1)
}

fn conditional_failed() -> CommandSyntaxError {
    CommandSyntaxError::dynamic(TextComponent::from(
        &translations::COMMANDS_EXECUTE_CONDITIONAL_FAIL,
    ))
}

fn missing_argument(name: &str) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(format!(
        "Parsed value for {name} is missing from the command context"
    ))
}
