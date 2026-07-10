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
use crate::scoreboard::{Scoreboard, ScoreboardObjective};

type Builder = CommandNodeBuilder<CommandSource, SteelCommandRuntime>;

const EXECUTE_ROOT: CommandRedirectTarget = CommandRedirectTarget::CommandRoot;

pub(super) fn conditionals(name: &'static str, expected: bool) -> Builder {
    literal(name)
        .then(biome_condition(expected))
        .then(entity_condition(expected))
        .then(loaded_condition(expected))
        .then(score_condition(expected))
}

fn biome_condition(expected: bool) -> Builder {
    literal("biome").then(
        argument("pos", SteelArgumentType::block_pos()).then(
            argument("biome", SteelArgumentType::biome_or_tag())
                .forks(EXECUTE_ROOT, move |context| {
                    let matches = biome_matches(context)?;
                    Ok(conditional_sources(context.source(), expected, matches))
                })
                .executes(move |context| {
                    execute_boolean_condition(context, expected, biome_matches(context)?)
                }),
        ),
    )
}

fn biome_matches(context: &SteelCommandContext<CommandSource>) -> Result<bool, CommandSyntaxError> {
    let position = context
        .coordinates("pos")
        .ok_or_else(|| missing_argument("pos"))?
        .block_pos(context.source());
    let world = context.source().world();
    if !world.is_full_chunk_loaded_at(position) {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::ARGUMENT_POS_UNLOADED,
        )));
    }
    if !world.is_in_valid_bounds(position) {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::ARGUMENT_POS_OUTOFWORLD,
        )));
    }
    let biome = world.biome_at(position).ok_or_else(|| {
        CommandSyntaxError::dynamic(TextComponent::from(&translations::ARGUMENT_POS_UNLOADED))
    })?;
    let expected = context
        .biome_or_tag("biome")
        .ok_or_else(|| missing_argument("biome"))?;
    Ok(expected.matches(biome))
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

fn score_condition(expected: bool) -> Builder {
    literal("score").then(
        argument("target", SteelArgumentType::score_holder()).then(
            argument("targetObjective", SteelArgumentType::objective())
                .then(score_comparison("=", ScoreComparison::Equal, expected))
                .then(score_comparison("<", ScoreComparison::Less, expected))
                .then(score_comparison(
                    "<=",
                    ScoreComparison::LessOrEqual,
                    expected,
                ))
                .then(score_comparison(">", ScoreComparison::Greater, expected))
                .then(score_comparison(
                    ">=",
                    ScoreComparison::GreaterOrEqual,
                    expected,
                ))
                .then(
                    literal("matches").then(
                        argument("range", SteelArgumentType::int_range())
                            .forks(EXECUTE_ROOT, move |context| {
                                let matches = score_range_matches(context)?;
                                Ok(conditional_sources(context.source(), expected, matches))
                            })
                            .executes(move |context| {
                                execute_boolean_condition(
                                    context,
                                    expected,
                                    score_range_matches(context)?,
                                )
                            }),
                    ),
                ),
        ),
    )
}

fn score_comparison(name: &'static str, comparison: ScoreComparison, expected: bool) -> Builder {
    literal(name).then(
        argument("source", SteelArgumentType::score_holder()).then(
            argument("sourceObjective", SteelArgumentType::objective())
                .forks(EXECUTE_ROOT, move |context| {
                    let matches = scores_match(context, comparison)?;
                    Ok(conditional_sources(context.source(), expected, matches))
                })
                .executes(move |context| {
                    execute_boolean_condition(context, expected, scores_match(context, comparison)?)
                }),
        ),
    )
}

fn scores_match(
    context: &SteelCommandContext<CommandSource>,
    comparison: ScoreComparison,
) -> Result<bool, CommandSyntaxError> {
    let scoreboard = source_scoreboard(context)?;
    let target = context.score_holder("target")?;
    let target_objective = objective(context, scoreboard, "targetObjective")?;
    let source = context.score_holder("source")?;
    let source_objective = objective(context, scoreboard, "sourceObjective")?;
    let Some(target_score) = scoreboard.score(&target, &target_objective) else {
        return Ok(false);
    };
    let Some(source_score) = scoreboard.score(&source, &source_objective) else {
        return Ok(false);
    };
    Ok(comparison.matches(target_score, source_score))
}

fn score_range_matches(
    context: &SteelCommandContext<CommandSource>,
) -> Result<bool, CommandSyntaxError> {
    let scoreboard = source_scoreboard(context)?;
    let target = context.score_holder("target")?;
    let target_objective = objective(context, scoreboard, "targetObjective")?;
    let range = context
        .int_range("range")
        .ok_or_else(|| missing_argument("range"))?;
    Ok(scoreboard
        .score(&target, &target_objective)
        .is_some_and(|score| range.matches(score)))
}

fn source_scoreboard(
    context: &SteelCommandContext<CommandSource>,
) -> Result<&Scoreboard, CommandSyntaxError> {
    let source = context.source();
    source
        .server()
        .scoreboards
        .get(source.world().domain())
        .ok_or_else(|| {
            CommandSyntaxError::dynamic(format!(
                "Domain '{}' has no command scoreboard",
                source.world().domain()
            ))
        })
}

fn objective(
    context: &SteelCommandContext<CommandSource>,
    scoreboard: &Scoreboard,
    name: &str,
) -> Result<ScoreboardObjective, CommandSyntaxError> {
    let objective_name = context
        .objective_name(name)
        .ok_or_else(|| missing_argument(name))?;
    scoreboard.objective(objective_name).ok_or_else(|| {
        let message = translations::ARGUMENTS_OBJECTIVE_NOT_FOUND
            .message([TextComponent::from(objective_name.to_owned())])
            .component();
        CommandSyntaxError::dynamic(message)
    })
}

#[derive(Clone, Copy)]
enum ScoreComparison {
    Equal,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

impl ScoreComparison {
    const fn matches(self, target: i32, source: i32) -> bool {
        match self {
            Self::Equal => target == source,
            Self::Less => target < source,
            Self::LessOrEqual => target <= source,
            Self::Greater => target > source,
            Self::GreaterOrEqual => target >= source,
        }
    }
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
