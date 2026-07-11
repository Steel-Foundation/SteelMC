//! Loaded-world command arguments.

use std::{fmt, sync::Arc};

use steel_utils::{Identifier, translations};

use super::{CommandArgumentSource, CommandSource, argument::parse_identifier};
use crate::{
    command::brigadier::{CommandSyntaxError, StringReader, SuggestionsBuilder},
    world::World,
};

/// A fully qualified world key or a world path relative to the source domain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WorldArgument {
    Key(Identifier),
    Relative(Box<str>),
}

impl WorldArgument {
    pub(crate) fn resolve(&self, source: &CommandSource) -> Result<Arc<World>, CommandSyntaxError> {
        let world = match self {
            Self::Key(key) => source.server().worlds.get(key),
            Self::Relative(path) => {
                let key = Identifier::new(source.world().domain().to_owned(), path.to_string());
                source.server().worlds.get(&key)
            }
        };
        world.map_or_else(
            || {
                let message = translations::ARGUMENT_DIMENSION_INVALID
                    .message([self.to_string()])
                    .component();
                Err(CommandSyntaxError::dynamic(message))
            },
            |world| Ok(Arc::clone(world)),
        )
    }
}

impl fmt::Display for WorldArgument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Key(key) => write!(formatter, "{key}"),
            Self::Relative(path) => formatter.write_str(path),
        }
    }
}

pub(super) fn parse_world_argument(
    reader: &mut StringReader<'_>,
) -> Result<WorldArgument, CommandSyntaxError> {
    let start_byte = reader.read_so_far().len();
    let key = parse_identifier(reader)?;
    let raw = &reader.read_so_far()[start_byte..];
    if raw.contains(':') {
        Ok(WorldArgument::Key(key))
    } else {
        Ok(WorldArgument::Relative(key.path.to_string().into()))
    }
}

pub(super) fn suggest_worlds<S>(builder: &mut SuggestionsBuilder<'_>, source: &S)
where
    S: CommandArgumentSource + ?Sized,
{
    let prefix = builder.remaining_lowercase().to_owned();
    for world in source
        .command_world_names()
        .into_iter()
        .filter(|world| world.starts_with(&prefix))
    {
        builder.suggest(world);
    }
}
