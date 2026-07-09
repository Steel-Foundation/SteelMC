use crate::command::brigadier::{
    ArgumentSuggestionContext, ArgumentType, CommandArgumentParser, CommandSyntaxError,
    ContainsPrimitiveArgumentValue, PrimitiveArgumentValue, StringReader, SuggestionsBuilder,
};

use super::ExecutionCommandSource;

/// An argument parser stored by Steel's command runtime.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SteelArgumentType {
    /// One of Brigadier's built-in primitive parsers.
    Primitive(ArgumentType),
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
}

impl ContainsPrimitiveArgumentValue for SteelArgumentValue {
    fn primitive_value(&self) -> Option<&PrimitiveArgumentValue> {
        match self {
            Self::Primitive(value) => Some(value),
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
        }
    }

    fn list_suggestions(
        &self,
        _context: &ArgumentSuggestionContext<'_, S, Self::Value>,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        match self {
            Self::Primitive(argument) => argument.suggest(builder),
        }
    }
}
