//! Vanilla-style entity selector parsing and resolution.
//!
//! Vanilla's server-wide candidate scope maps to one Steel domain. Selectors
//! never expose players, entities, teams, or scores from another domain.

use std::sync::Arc;

use glam::DVec3;
use rand::seq::SliceRandom;
use simdnbt::owned::NbtCompound;
use steel_registry::{
    REGISTRY, RegistryExt as _, TaggedRegistryExt as _, entity_type::EntityTypeRef,
    vanilla_entities,
};
use steel_utils::{
    Identifier,
    geometry::WorldAabb,
    java,
    nbt::{compare_nbt_compounds, parse_snbt_compound_argument},
    translations,
    types::GameType,
};
use text_components::TextComponent;
use uuid::Uuid;

use crate::{
    command::brigadier::{
        CommandSyntaxError, CommandSyntaxErrorKind, ReaderCursor, StringReader, SuggestionsBuilder,
    },
    entity::{Entity, SharedEntity},
    player::Player,
    scoreboard::{ScoreHolder, Scoreboard},
    world::World,
};

use super::{CommandArgumentSource, CommandSource};

const SORT_NEAREST: &str = "nearest";
const SORT_FURTHEST: &str = "furthest";
const SORT_RANDOM: &str = "random";
const SORT_ARBITRARY: &str = "arbitrary";
const SELECTOR_OPTION_KEYS: &[&str] = &[
    "name",
    "distance",
    "level",
    "x",
    "y",
    "z",
    "dx",
    "dy",
    "dz",
    "x_rotation",
    "y_rotation",
    "limit",
    "sort",
    "gamemode",
    "type",
    "tag",
    "team",
    "nbt",
    "scores",
    "advancements",
    "predicate",
];
const UNSUPPORTED_SELECTOR_OPTION_KEYS: &[&str] = &[
    // Needs player advancement progress before it can resolve faithfully.
    "advancements",
    // Needs a real reloadable/plugin predicate registry.
    "predicate",
];
const SET_ONCE_SELECTOR_OPTIONS: &[&str] = &[
    "distance",
    "level",
    "x",
    "y",
    "z",
    "dx",
    "dy",
    "dz",
    "x_rotation",
    "y_rotation",
    "limit",
    "sort",
    "scores",
    "advancements",
];
const GAME_MODE_SUGGESTIONS: &[&str] = &["survival", "creative", "adventure", "spectator"];

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EntitySelector {
    kind: SelectorKind,
    max_results: usize,
    includes_entities: bool,
    current_entity: bool,
    world_limited: bool,
    order: SelectorOrder,
    position: SelectorPosition,
    delta: SelectorDelta,
    distance: Option<DoubleRange>,
    level: Option<IntRange>,
    x_rotation: Option<FloatRange>,
    y_rotation: Option<FloatRange>,
    filters: Vec<SelectorFilter>,
    uses_advanced_options: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum SelectorKind {
    Selector(SelectorType),
    PlayerName(String),
    EntityUuid(Uuid),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectorType {
    AllPlayers,
    AllEntities,
    NearestEntity,
    NearestPlayer,
    RandomPlayer,
    SelfEntity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectorOrder {
    Nearest,
    Furthest,
    Random,
    Arbitrary,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct SelectorPosition {
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
}

impl SelectorPosition {
    fn apply(&self, base: DVec3) -> DVec3 {
        DVec3::new(
            self.x.unwrap_or(base.x),
            self.y.unwrap_or(base.y),
            self.z.unwrap_or(base.z),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct SelectorDelta {
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
}

impl SelectorDelta {
    const fn has_any(self) -> bool {
        self.x.is_some() || self.y.is_some() || self.z.is_some()
    }

    fn aabb(self) -> WorldAabb {
        create_delta_aabb(
            self.x.unwrap_or(0.0),
            self.y.unwrap_or(0.0),
            self.z.unwrap_or(0.0),
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
enum SelectorFilter {
    Alive,
    Name {
        value: String,
        inverted: bool,
    },
    GameMode {
        value: GameType,
        inverted: bool,
    },
    EntityType {
        value: EntityTypeRef,
        inverted: bool,
    },
    EntityTypeTag {
        value: Identifier,
        inverted: bool,
    },
    Tag {
        value: String,
        inverted: bool,
    },
    Team {
        value: String,
        inverted: bool,
    },
    Nbt {
        value: NbtCompound,
        inverted: bool,
    },
    Scores(Vec<(String, IntRange)>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DoubleRange {
    min: Option<f64>,
    max: Option<f64>,
}

impl DoubleRange {
    fn matches_squared(self, value: f64) -> bool {
        if let Some(min) = self.min
            && value < min * min
        {
            return false;
        }
        if let Some(max) = self.max
            && value > max * max
        {
            return false;
        }
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FloatRange {
    min: Option<f32>,
    max: Option<f32>,
}

impl FloatRange {
    fn matches_rotation(self, value: f32) -> bool {
        let min = wrap_degrees(self.min.unwrap_or(0.0));
        let max = wrap_degrees(self.max.unwrap_or(359.0));
        let value = wrap_degrees(value);
        if min > max {
            value >= min || value <= max
        } else {
            value >= min && value <= max
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct IntRange {
    min: Option<i32>,
    max: Option<i32>,
}

impl IntRange {
    #[cfg(test)]
    const fn exactly(value: i32) -> Self {
        Self {
            min: Some(value),
            max: Some(value),
        }
    }

    const fn matches(self, value: i32) -> bool {
        if let Some(min) = self.min
            && value < min
        {
            return false;
        }
        if let Some(max) = self.max
            && value > max
        {
            return false;
        }
        true
    }
}

#[derive(Clone, Debug, Default)]
struct InvertableOptionState {
    positive_seen: bool,
    negative_seen: bool,
}

impl InvertableOptionState {
    fn parse_element(&mut self, inverted: bool, option: &str) -> Result<(), SelectorParseError> {
        if inverted {
            if self.positive_seen {
                return Err(SelectorParseError::invalid(format!(
                    "option '{option}' cannot be repeated after a positive value"
                )));
            }
            self.negative_seen = true;
        } else {
            if self.positive_seen || self.negative_seen {
                return Err(SelectorParseError::invalid(format!(
                    "option '{option}' cannot add a positive value after another value"
                )));
            }
            self.positive_seen = true;
        }
        Ok(())
    }

    const fn suggestion_mode(&self) -> InvertableSuggestionMode {
        if self.positive_seen {
            InvertableSuggestionMode::None
        } else if self.negative_seen {
            InvertableSuggestionMode::NegativeOnly
        } else {
            InvertableSuggestionMode::Any
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InvertableSuggestionMode {
    Any,
    NegativeOnly,
    None,
}

impl InvertableSuggestionMode {
    const fn allows_positive(self) -> bool {
        matches!(self, Self::Any)
    }

    const fn allows_negative(self) -> bool {
        matches!(self, Self::Any | Self::NegativeOnly)
    }

    const fn allows_any(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Clone, Debug, Default)]
struct EntityTypeOptionState {
    invertible: InvertableOptionState,
    tags_seen: Vec<Identifier>,
}

impl EntityTypeOptionState {
    fn parse_element(&mut self, inverted: bool, option: &str) -> Result<(), SelectorParseError> {
        self.invertible.parse_element(inverted, option)
    }

    fn parse_tag(&mut self, tag: &Identifier, option: &str) -> Result<(), SelectorParseError> {
        if self.tags_seen.iter().any(|existing| existing == tag) {
            return Err(SelectorParseError::invalid(format!(
                "option '{option}' cannot repeat tag '#{tag}'"
            )));
        }
        self.invertible.parse_element(true, option)?;
        self.tags_seen.push(tag.clone());
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
struct SelectorOptionState {
    name: InvertableOptionState,
    team: InvertableOptionState,
    gamemode: InvertableOptionState,
    entity_type: EntityTypeOptionState,
    distance: bool,
    level: bool,
    x: bool,
    y: bool,
    z: bool,
    dx: bool,
    dy: bool,
    dz: bool,
    x_rotation: bool,
    y_rotation: bool,
    limit: bool,
    sort: bool,
    scores: bool,
}

#[derive(Clone, Debug)]
struct SelectorParseError {
    kind: SelectorParseErrorKind,
    cursor: usize,
}

#[derive(Clone, Debug)]
enum SelectorParseErrorKind {
    NotAllowed,
    AdvancedNotAllowed,
    Invalid(Box<TextComponent>),
    Unsupported(String),
}

impl SelectorParseError {
    const fn not_allowed(cursor: usize) -> Self {
        Self {
            kind: SelectorParseErrorKind::NotAllowed,
            cursor,
        }
    }

    const fn advanced_not_allowed(cursor: usize) -> Self {
        Self {
            kind: SelectorParseErrorKind::AdvancedNotAllowed,
            cursor,
        }
    }

    fn invalid(message: impl Into<TextComponent>) -> Self {
        Self {
            kind: SelectorParseErrorKind::Invalid(Box::new(message.into())),
            cursor: 0,
        }
    }

    fn invalid_at(message: impl Into<TextComponent>, cursor: usize) -> Self {
        Self {
            kind: SelectorParseErrorKind::Invalid(Box::new(message.into())),
            cursor,
        }
    }

    fn unsupported(option: impl Into<String>, cursor: usize) -> Self {
        Self {
            kind: SelectorParseErrorKind::Unsupported(option.into()),
            cursor,
        }
    }

    fn message(self) -> TextComponent {
        match self.kind {
            SelectorParseErrorKind::NotAllowed => {
                TextComponent::from(&translations::ARGUMENT_ENTITY_SELECTOR_NOT_ALLOWED)
            }
            SelectorParseErrorKind::AdvancedNotAllowed => {
                TextComponent::from("Advanced entity selectors are not allowed")
            }
            SelectorParseErrorKind::Invalid(message) => *message,
            SelectorParseErrorKind::Unsupported(option) => {
                TextComponent::from(format!("Unsupported entity selector option: {option}"))
            }
        }
    }
}

mod model;
mod parser;
mod suggestions;

use model::create_delta_aabb;
pub(crate) use parser::*;
pub(crate) use suggestions::*;

#[cfg(test)]
mod tests;
