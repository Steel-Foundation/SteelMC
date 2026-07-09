use crate::command::brigadier::{
    ArgumentSuggestionContext, ArgumentType, CommandArgumentParser, CommandSyntaxError,
    CommandSyntaxErrorKind, ContainsPrimitiveArgumentValue, PrimitiveArgumentValue, StringReader,
    SuggestionsBuilder,
};
use steel_registry::{
    REGISTRY, RegistryExt as _, TIMELINE_REGISTRY, WORLD_CLOCK_REGISTRY, timeline::TimelineRef,
    world_clock::WorldClockRef,
};
use steel_utils::Identifier;
use steel_utils::translations;
use text_components::TextComponent;

use super::ExecutionCommandSource;

/// An argument parser stored by Steel's command runtime.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SteelArgumentType {
    /// One of Brigadier's built-in primitive parsers.
    Primitive(ArgumentType),
    /// A Minecraft duration measured in ticks with an optional unit suffix.
    Time { minimum: i32 },
    /// A registered world clock.
    WorldClock,
    /// A registered timeline, suggested only when it uses the selected clock.
    Timeline {
        clock_argument: Option<&'static str>,
    },
    /// A resource location naming a command-visible marker for the selected clock.
    TimeMarker {
        clock_argument: Option<&'static str>,
    },
}

impl SteelArgumentType {
    pub(crate) const fn time(minimum: i32) -> Self {
        Self::Time { minimum }
    }

    pub(crate) const fn world_clock() -> Self {
        Self::WorldClock
    }

    pub(crate) const fn timeline(clock_argument: Option<&'static str>) -> Self {
        Self::Timeline { clock_argument }
    }

    pub(crate) const fn time_marker(clock_argument: Option<&'static str>) -> Self {
        Self::TimeMarker { clock_argument }
    }
}

impl From<ArgumentType> for SteelArgumentType {
    fn from(argument: ArgumentType) -> Self {
        Self::Primitive(argument)
    }
}

/// A parsed argument retained by Steel's command runtime.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SteelArgumentValue {
    /// A value produced by a Brigadier primitive parser.
    Primitive(PrimitiveArgumentValue),
    /// A Minecraft duration resolved to ticks.
    Time(i32),
    /// A parsed resource location.
    Identifier(Identifier),
    /// A resolved registered world clock.
    WorldClock(WorldClockRef),
    /// A resolved registered timeline.
    Timeline(TimelineRef),
}

impl ContainsPrimitiveArgumentValue for SteelArgumentValue {
    fn primitive_value(&self) -> Option<&PrimitiveArgumentValue> {
        match self {
            Self::Primitive(value) => Some(value),
            Self::Time(_) | Self::Identifier(_) | Self::WorldClock(_) | Self::Timeline(_) => None,
        }
    }
}

impl<S> CommandArgumentParser<S> for SteelArgumentType
where
    S: ExecutionCommandSource,
{
    type Value = SteelArgumentValue;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        _source: &S,
    ) -> Result<Self::Value, CommandSyntaxError> {
        match self {
            Self::Primitive(argument) => argument
                .parse_value(reader)
                .map(SteelArgumentValue::Primitive),
            Self::Time { minimum } => parse_time(reader, *minimum).map(SteelArgumentValue::Time),
            Self::WorldClock => {
                let key = parse_identifier(reader)?;
                REGISTRY.world_clocks.by_key(&key).map_or_else(
                    || Err(unknown_resource(reader, &key, &WORLD_CLOCK_REGISTRY)),
                    |clock| Ok(SteelArgumentValue::WorldClock(clock)),
                )
            }
            Self::Timeline { .. } => {
                let key = parse_identifier(reader)?;
                REGISTRY.timelines.by_key(&key).map_or_else(
                    || Err(unknown_resource(reader, &key, &TIMELINE_REGISTRY)),
                    |timeline| Ok(SteelArgumentValue::Timeline(timeline)),
                )
            }
            Self::TimeMarker { .. } => parse_identifier(reader).map(SteelArgumentValue::Identifier),
        }
    }

    fn list_suggestions(
        &self,
        context: &ArgumentSuggestionContext<'_, S, Self::Value>,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        match self {
            Self::Primitive(argument) => argument.suggest(builder),
            Self::Time { .. } => suggest_time_units(builder),
            Self::WorldClock => {
                suggest_resources(
                    REGISTRY.world_clocks.iter().map(|(_, clock)| &clock.key),
                    builder,
                );
            }
            Self::Timeline { clock_argument } => {
                let Some(clock) = selected_clock(context, *clock_argument) else {
                    return;
                };
                suggest_resources(
                    REGISTRY
                        .timelines
                        .iter()
                        .filter(|(_, timeline)| timeline.clock == clock)
                        .map(|(_, timeline)| &timeline.key),
                    builder,
                );
            }
            Self::TimeMarker { clock_argument } => {
                let Some(clock) = selected_clock(context, *clock_argument) else {
                    return;
                };
                suggest_resources(
                    REGISTRY
                        .timelines
                        .iter()
                        .filter(|(_, timeline)| timeline.clock == clock)
                        .flat_map(|(_, timeline)| timeline.time_markers)
                        .filter(|marker| marker.show_in_commands == Some(true))
                        .map(|marker| &marker.key),
                    builder,
                );
            }
        }
    }
}

fn selected_clock<S>(
    context: &ArgumentSuggestionContext<'_, S, SteelArgumentValue>,
    clock_argument: Option<&str>,
) -> Option<WorldClockRef>
where
    S: ExecutionCommandSource,
{
    let Some(clock_argument) = clock_argument else {
        return context.source().default_world_clock();
    };
    match context.argument(clock_argument) {
        Some(SteelArgumentValue::WorldClock(clock)) => Some(*clock),
        Some(
            SteelArgumentValue::Primitive(_)
            | SteelArgumentValue::Time(_)
            | SteelArgumentValue::Identifier(_)
            | SteelArgumentValue::Timeline(_),
        )
        | None => None,
    }
}

fn parse_identifier(reader: &mut StringReader<'_>) -> Result<Identifier, CommandSyntaxError> {
    let start = reader.checkpoint();
    let start_byte = reader.read_so_far().len();
    while reader.peek().is_some_and(is_allowed_in_identifier) {
        reader.skip();
    }
    let raw = &reader.read_so_far()[start_byte..];
    let (namespace, path) =
        raw.split_once(':')
            .map_or((Identifier::VANILLA_NAMESPACE, raw), |(namespace, path)| {
                if namespace.is_empty() {
                    (Identifier::VANILLA_NAMESPACE, path)
                } else {
                    (namespace, path)
                }
            });
    if namespace != ".."
        && Identifier::validate_namespace(namespace)
        && Identifier::validate_path(path)
    {
        return Ok(Identifier::new(namespace.to_owned(), path.to_owned()));
    }

    reader.restore(start);
    Err(reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(
        TextComponent::from(&translations::ARGUMENT_ID_INVALID),
    ))))
}

const fn is_allowed_in_identifier(character: char) -> bool {
    character.is_ascii_digit()
        || character.is_ascii_lowercase()
        || matches!(character, '_' | ':' | '/' | '.' | '-')
}

fn unknown_resource(
    reader: &StringReader<'_>,
    key: &Identifier,
    registry: &Identifier,
) -> CommandSyntaxError {
    let message = translations::ARGUMENT_RESOURCE_NOT_FOUND
        .message([key.to_string(), registry.to_string()])
        .component();
    reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message)))
}

fn suggest_resources<'a>(
    resources: impl Iterator<Item = &'a Identifier>,
    builder: &mut SuggestionsBuilder<'_>,
) {
    let contents = builder.remaining_lowercase();
    let has_namespace = contents.contains(':');
    let suggestions = resources.filter_map(|resource| {
        let full_name = resource.to_string();
        let matches = if has_namespace {
            matches_substring(contents, &full_name)
        } else {
            matches_substring(contents, resource.namespace.as_ref())
                || matches_substring(contents, resource.path.as_ref())
        };
        matches.then_some(full_name)
    });
    let suggestions = suggestions.collect::<Vec<_>>();
    for suggestion in suggestions {
        builder.suggest(suggestion);
    }
}

fn matches_substring(pattern: &str, input: &str) -> bool {
    if input.starts_with(pattern) {
        return true;
    }
    input.char_indices().any(|(index, character)| {
        matches!(character, '.' | '_' | '/')
            && input[index + character.len_utf8()..].starts_with(pattern)
    })
}

fn parse_time(reader: &mut StringReader<'_>, minimum: i32) -> Result<i32, CommandSyntaxError> {
    let value = reader.read_float()?;
    let unit = reader.read_unquoted_string();
    let factor = match unit {
        "d" => 24_000.0,
        "s" => 20.0,
        "t" | "" => 1.0,
        _ => {
            return Err(reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(
                TextComponent::from(&translations::ARGUMENT_TIME_INVALID_UNIT),
            ))));
        }
    };
    let ticks = java_round(value * factor);
    if ticks < minimum {
        let message = translations::ARGUMENT_TIME_TICK_COUNT_TOO_LOW
            .message([minimum.to_string(), ticks.to_string()])
            .component();
        return Err(reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message))));
    }
    Ok(ticks)
}

fn suggest_time_units(builder: &mut SuggestionsBuilder<'_>) {
    let mut reader = StringReader::new(builder.remaining());
    if reader.read_float().is_err() {
        return;
    }
    let number = reader.read_so_far();
    let unit = reader.read_unquoted_string();
    for candidate in ["d", "s", "t"] {
        if candidate.starts_with(unit) {
            builder.suggest(format!("{number}{candidate}"));
        }
    }
}

fn java_round(value: f32) -> i32 {
    (value + 0.5).floor() as i32
}
