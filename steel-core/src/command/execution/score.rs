//! Deferred scoreboard command arguments.

use uuid::Uuid;

use super::{CommandArgumentSource, CommandSource};
use crate::{
    command::{
        brigadier::{
            CommandSyntaxError, CommandSyntaxErrorKind, ReaderCursor, StringReader,
            SuggestionsBuilder,
        },
        execution::selector::{EntitySelector, parse_entity_selector, suggest_entity_selector},
    },
    entity::Entity as _,
    scoreboard::{ScoreHolder, Scoreboard},
};
use steel_utils::translations;
use text_components::{TextComponent, translation::Translation};

/// A score holder expression retained until command execution.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ScoreHolderArgument {
    /// A direct holder name, resolved against online players at execution time.
    Name(Box<str>),
    /// A UUID with its original token retained for name-only fallback.
    Uuid { uuid: Uuid, raw: Box<str> },
    /// An entity selector resolved to entity scoreboard names at execution time.
    Selector(Box<EntitySelector>),
    /// The scoreboard's tracked-holder wildcard.
    Wildcard,
}

/// Wildcard expansion used by a score-holder consumer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScoreHolderWildcard {
    /// Wildcards resolve to no holders.
    Empty,
    /// Wildcards resolve to every holder tracked by the current domain scoreboard.
    Tracked,
}

impl ScoreHolderArgument {
    pub(crate) fn resolve(
        &self,
        source: &CommandSource,
        wildcard: ScoreHolderWildcard,
    ) -> Result<Vec<ScoreHolder>, CommandSyntaxError> {
        let holders = match self {
            Self::Name(name) => vec![resolve_name(name, source)],
            Self::Uuid { uuid, raw } => resolve_uuid(uuid, raw, source),
            Self::Selector(selector) => selector
                .find_entities(source)?
                .into_iter()
                .map(|entity| ScoreHolder::new(entity.scoreboard_name()))
                .collect(),
            Self::Wildcard => match wildcard {
                ScoreHolderWildcard::Empty => Vec::new(),
                ScoreHolderWildcard::Tracked => source
                    .server()
                    .scoreboards
                    .get(source.world().domain())
                    .map_or_else(Vec::new, Scoreboard::tracked_holders),
            },
        };
        Ok(holders)
    }
}

/// Inclusive integer bounds used by vanilla range arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IntRange {
    minimum: Option<i32>,
    maximum: Option<i32>,
}

impl IntRange {
    const fn new(minimum: Option<i32>, maximum: Option<i32>) -> Self {
        Self { minimum, maximum }
    }

    pub(crate) const fn matches(self, value: i32) -> bool {
        if let Some(minimum) = self.minimum
            && value < minimum
        {
            return false;
        }
        if let Some(maximum) = self.maximum
            && value > maximum
        {
            return false;
        }
        true
    }
}

pub(super) fn parse_score_holder<S>(
    reader: &mut StringReader<'_>,
    source: &S,
    multiple: bool,
) -> Result<ScoreHolderArgument, CommandSyntaxError>
where
    S: CommandArgumentSource + ?Sized,
{
    if reader.peek() == Some('@') {
        return parse_entity_selector(reader, source, !multiple, false)
            .map(Box::new)
            .map(ScoreHolderArgument::Selector);
    }

    let start = reader.read_so_far().len();
    while reader.can_read() && reader.peek() != Some(' ') {
        reader.skip();
    }
    let raw = &reader.input()[start..reader.read_so_far().len()];
    if raw == "*" {
        return Ok(ScoreHolderArgument::Wildcard);
    }
    if let Ok(uuid) = Uuid::parse_str(raw) {
        return Ok(ScoreHolderArgument::Uuid {
            uuid,
            raw: raw.into(),
        });
    }
    Ok(ScoreHolderArgument::Name(raw.into()))
}

pub(super) fn suggest_score_holders<S>(builder: &mut SuggestionsBuilder<'_>, source: &S)
where
    S: CommandArgumentSource + ?Sized,
{
    // Vanilla's SUGGEST_SCORE_HOLDERS provider is selector-aware but does not
    // filter suggestions to the single-result parser variant.
    suggest_entity_selector(builder, source, false, false);
}

pub(super) fn parse_int_range(
    reader: &mut StringReader<'_>,
) -> Result<IntRange, CommandSyntaxError> {
    let start = reader.checkpoint();
    if !reader.can_read() {
        return Err(range_error(
            reader,
            start,
            &translations::ARGUMENT_RANGE_EMPTY,
        ));
    }

    let minimum = read_optional_integer(reader)
        .map_err(|()| range_error(reader, start, &translations::ARGUMENT_RANGE_INTS))?;
    let maximum = if reader.can_read_length(2)
        && reader.peek() == Some('.')
        && peek_next(reader) == Some('.')
    {
        reader.skip();
        reader.skip();
        read_optional_integer(reader)
            .map_err(|()| range_error(reader, start, &translations::ARGUMENT_RANGE_INTS))?
    } else {
        minimum
    };

    if minimum.is_none() && maximum.is_none() {
        return Err(range_error(
            reader,
            start,
            &translations::ARGUMENT_RANGE_EMPTY,
        ));
    }
    if minimum
        .zip(maximum)
        .is_some_and(|(minimum, maximum)| minimum > maximum)
    {
        return Err(range_error(
            reader,
            start,
            &translations::ARGUMENT_RANGE_SWAPPED,
        ));
    }
    Ok(IntRange::new(minimum, maximum))
}

fn resolve_name(name: &str, source: &CommandSource) -> ScoreHolder {
    if name.starts_with('#') {
        return ScoreHolder::new(name.to_owned());
    }
    source
        .server()
        .get_players()
        .into_iter()
        .filter(|player| player.get_world().domain() == source.world().domain())
        .find(|player| player.gameprofile.name.eq_ignore_ascii_case(name))
        .map_or_else(
            || ScoreHolder::new(name.to_owned()),
            |player| ScoreHolder::new(player.scoreboard_name()),
        )
}

fn resolve_uuid(uuid: &Uuid, raw: &str, source: &CommandSource) -> Vec<ScoreHolder> {
    let holders = source
        .server()
        .worlds
        .worlds_in_domain(source.world().domain())
        .into_iter()
        .filter_map(|world| world.get_entity_by_uuid(uuid))
        .map(|entity| ScoreHolder::new(entity.scoreboard_name()))
        .collect::<Vec<_>>();
    if holders.is_empty() {
        vec![ScoreHolder::new(raw.to_owned())]
    } else {
        holders
    }
}

fn read_optional_integer(reader: &mut StringReader<'_>) -> Result<Option<i32>, ()> {
    let start = reader.read_so_far().len();
    while reader
        .peek()
        .is_some_and(|character| is_range_number_character(character, peek_next(reader)))
    {
        reader.skip();
    }
    let raw = &reader.input()[start..reader.read_so_far().len()];
    if raw.is_empty() {
        return Ok(None);
    }
    raw.parse().map(Some).map_err(|_| ())
}

fn peek_next(reader: &StringReader<'_>) -> Option<char> {
    reader.remaining().chars().nth(1)
}

fn is_range_number_character(character: char, next: Option<char>) -> bool {
    character.is_ascii_digit() || character == '-' || (character == '.' && next != Some('.'))
}

fn range_error(
    reader: &mut StringReader<'_>,
    start: ReaderCursor,
    translation: &'static Translation<0>,
) -> CommandSyntaxError {
    reader.restore(start);
    reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(
        TextComponent::from(translation),
    )))
}
