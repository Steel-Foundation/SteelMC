use crate::command::brigadier::{
    ArgumentSuggestionContext, ArgumentType, CommandArgumentParser, CommandSyntaxError,
    CommandSyntaxErrorKind, ContainsPrimitiveArgumentValue, PrimitiveArgumentValue, StringReader,
    SuggestionsBuilder,
};
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
}

impl SteelArgumentType {
    pub(crate) const fn time(minimum: i32) -> Self {
        Self::Time { minimum }
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
}

impl ContainsPrimitiveArgumentValue for SteelArgumentValue {
    fn primitive_value(&self) -> Option<&PrimitiveArgumentValue> {
        match self {
            Self::Primitive(value) => Some(value),
            Self::Time(_) => None,
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
        }
    }

    fn list_suggestions(
        &self,
        _context: &ArgumentSuggestionContext<'_, S, Self::Value>,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        match self {
            Self::Primitive(argument) => argument.suggest(builder),
            Self::Time { .. } => suggest_time_units(builder),
        }
    }
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
