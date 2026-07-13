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

impl EntitySelector {
    fn new(
        kind: SelectorKind,
        max_results: usize,
        includes_entities: bool,
        current_entity: bool,
        order: SelectorOrder,
    ) -> Self {
        Self {
            kind,
            max_results,
            includes_entities,
            current_entity,
            world_limited: false,
            order,
            position: SelectorPosition::default(),
            delta: SelectorDelta::default(),
            distance: None,
            level: None,
            x_rotation: None,
            y_rotation: None,
            filters: Vec::new(),
            uses_advanced_options: false,
        }
    }

    fn for_selector_type(selector_type: SelectorType) -> Self {
        let (max_results, includes_entities, current_entity, order) = match selector_type {
            SelectorType::AllPlayers => (usize::MAX, false, false, SelectorOrder::Arbitrary),
            SelectorType::AllEntities => (usize::MAX, true, false, SelectorOrder::Arbitrary),
            SelectorType::NearestEntity => (1, true, false, SelectorOrder::Nearest),
            SelectorType::NearestPlayer => (1, false, false, SelectorOrder::Nearest),
            SelectorType::RandomPlayer => (1, false, false, SelectorOrder::Random),
            SelectorType::SelfEntity => (1, true, true, SelectorOrder::Arbitrary),
        };
        let mut selector = Self::new(
            SelectorKind::Selector(selector_type),
            max_results,
            includes_entities,
            current_entity,
            order,
        );
        if matches!(
            selector_type,
            SelectorType::AllEntities | SelectorType::NearestEntity
        ) {
            selector.filters.push(SelectorFilter::Alive);
        }
        selector
    }

    fn validate_for_argument(
        &self,
        single: bool,
        players_only: bool,
    ) -> Result<(), SelectorParseError> {
        if single && self.max_results > 1 {
            let message = if players_only {
                TextComponent::from(&translations::ARGUMENT_PLAYER_TOOMANY).to_string()
            } else {
                TextComponent::from(&translations::ARGUMENT_ENTITY_TOOMANY).to_string()
            };
            return Err(SelectorParseError::invalid(message));
        }
        if players_only && self.includes_entities && !self.current_entity {
            return Err(SelectorParseError::invalid(
                TextComponent::from(&translations::ARGUMENT_PLAYER_ENTITIES).to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn find_players(
        &self,
        source: &CommandSource,
    ) -> Result<Vec<Arc<Player>>, CommandSyntaxError> {
        self.check_selector_permission(source)?;
        let server = source.server();
        let position = selector_position(self, source);
        let aabb = self.absolute_aabb(position);
        let mut players = match &self.kind {
            SelectorKind::PlayerName(name) => server
                .get_players()
                .into_iter()
                .filter(|player| player.get_world().domain() == source.world().domain())
                .filter(|player| player_name_matches(&player.gameprofile.name, name))
                .collect::<Vec<_>>(),
            SelectorKind::EntityUuid(uuid) => server
                .get_players()
                .into_iter()
                .filter(|player| player.get_world().domain() == source.world().domain())
                .filter(|player| player.uuid() == *uuid)
                .collect::<Vec<_>>(),
            SelectorKind::Selector(SelectorType::SelfEntity) => {
                let Some(player) = source.player() else {
                    return Ok(Vec::new());
                };
                if self.matches_entity(player.as_ref(), position, aabb, source)? {
                    vec![Arc::clone(player)]
                } else {
                    Vec::new()
                }
            }
            SelectorKind::Selector(_) => self.candidate_players(source),
        };

        if !matches!(self.kind, SelectorKind::Selector(SelectorType::SelfEntity)) {
            let mut filtered = Vec::new();
            for player in players {
                if self.matches_entity(player.as_ref(), position, aabb, source)? {
                    filtered.push(player);
                    if self.stops_filtering_after_match_count(filtered.len()) {
                        break;
                    }
                }
            }
            players = filtered;
        }
        self.sort_and_limit_players(position, &mut players);
        Ok(players)
    }

    pub(crate) fn find_entities(
        &self,
        source: &CommandSource,
    ) -> Result<Vec<SharedEntity>, CommandSyntaxError> {
        self.check_selector_permission(source)?;
        if !self.includes_entities {
            return Ok(self
                .find_players(source)?
                .into_iter()
                .map(|player| player as SharedEntity)
                .collect());
        }
        let server = source.server();
        let position = selector_position(self, source);
        let aabb = self.absolute_aabb(position);
        let mut entities = match &self.kind {
            SelectorKind::PlayerName(name) => server
                .get_players()
                .into_iter()
                .filter(|player| player.get_world().domain() == source.world().domain())
                .filter(|player| player_name_matches(&player.gameprofile.name, name))
                .map(|player| player as SharedEntity)
                .collect::<Vec<_>>(),
            SelectorKind::EntityUuid(uuid) => find_entity_by_uuid(source, uuid)
                .into_iter()
                .collect::<Vec<_>>(),
            SelectorKind::Selector(SelectorType::SelfEntity) => {
                let Some(entity) = source.entity() else {
                    return Ok(Vec::new());
                };
                if self.matches_entity(entity.as_ref(), position, aabb, source)? {
                    vec![Arc::clone(entity)]
                } else {
                    Vec::new()
                }
            }
            SelectorKind::Selector(_) => self.candidate_entities(source, aabb),
        };

        if !matches!(self.kind, SelectorKind::Selector(SelectorType::SelfEntity)) {
            let mut filtered = Vec::new();
            for entity in entities {
                if self.matches_entity(entity.as_ref(), position, aabb, source)? {
                    filtered.push(entity);
                    if self.stops_filtering_after_match_count(filtered.len()) {
                        break;
                    }
                }
            }
            entities = filtered;
        }
        self.sort_and_limit_entities(position, &mut entities);
        Ok(entities)
    }

    fn check_selector_permission(&self, source: &CommandSource) -> Result<(), CommandSyntaxError> {
        if !matches!(self.kind, SelectorKind::Selector(_)) {
            return Ok(());
        }
        if !allow_selectors(source) {
            return Err(CommandSyntaxError::dynamic(TextComponent::from(
                &translations::ARGUMENT_ENTITY_SELECTOR_NOT_ALLOWED,
            )));
        }
        if self.uses_advanced_options && !allow_advanced_selectors(source) {
            return Err(CommandSyntaxError::dynamic(
                "Advanced entity selectors are not allowed",
            ));
        }
        Ok(())
    }

    fn candidate_players(&self, source: &CommandSource) -> Vec<Arc<Player>> {
        let mut players = source.server().get_players();
        if self.world_limited {
            players.retain(|player| Arc::ptr_eq(&player.get_world(), source.world()));
        } else {
            let domain = source.world().domain();
            players.retain(|player| player.get_world().domain() == domain);
        }
        players
    }

    fn candidate_entities(
        &self,
        source: &CommandSource,
        aabb: Option<WorldAabb>,
    ) -> Vec<SharedEntity> {
        if self.world_limited {
            return world_candidates(source.world(), aabb);
        }

        source
            .server()
            .worlds
            .worlds_in_domain(source.world().domain())
            .into_iter()
            .flat_map(|world| world_candidates(&world, aabb))
            .collect()
    }

    fn absolute_aabb(&self, position: DVec3) -> Option<WorldAabb> {
        if self.delta.has_any() {
            return Some(self.delta.aabb().translate(position));
        }
        let max_distance = self.distance.and_then(|distance| distance.max)?;
        Some(
            WorldAabb::from_min_max(
                DVec3::splat(-max_distance),
                DVec3::splat(max_distance + 1.0),
            )
            .translate(position),
        )
    }

    fn requires_position(&self) -> bool {
        self.distance.is_some()
            || self.delta.has_any()
            || self
                .position
                .x
                .is_some_and(|_| self.position.y.is_none() || self.position.z.is_none())
            || self
                .position
                .y
                .is_some_and(|_| self.position.x.is_none() || self.position.z.is_none())
            || self
                .position
                .z
                .is_some_and(|_| self.position.x.is_none() || self.position.y.is_none())
            || matches!(self.order, SelectorOrder::Nearest | SelectorOrder::Furthest)
    }

    fn matches_entity(
        &self,
        entity: &dyn Entity,
        position: DVec3,
        aabb: Option<WorldAabb>,
        source: &CommandSource,
    ) -> Result<bool, CommandSyntaxError> {
        if let Some(aabb) = aabb
            && !aabb.intersects(entity.bounding_box())
        {
            return Ok(false);
        }
        if let Some(distance) = self.distance
            && !distance.matches_squared(entity.position().distance_squared(position))
        {
            return Ok(false);
        }
        if let Some(level) = self.level {
            let Some(player) = entity.as_player() else {
                return Ok(false);
            };
            if !level.matches(player.experience.lock().level()) {
                return Ok(false);
            }
        }
        if let Some(range) = self.x_rotation
            && !range.matches_rotation(entity.rotation().1)
        {
            return Ok(false);
        }
        if let Some(range) = self.y_rotation
            && !range.matches_rotation(entity.rotation().0)
        {
            return Ok(false);
        }
        for filter in &self.filters {
            if !filter.matches(entity, source)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    const fn stops_filtering_after_match_count(&self, count: usize) -> bool {
        matches!(self.order, SelectorOrder::Arbitrary) && count >= self.max_results
    }

    fn sort_and_limit_players(&self, position: DVec3, players: &mut Vec<Arc<Player>>) {
        match self.order {
            SelectorOrder::Nearest => players.sort_by(|left, right| {
                left.position()
                    .distance_squared(position)
                    .total_cmp(&right.position().distance_squared(position))
            }),
            SelectorOrder::Furthest => players.sort_by(|left, right| {
                right
                    .position()
                    .distance_squared(position)
                    .total_cmp(&left.position().distance_squared(position))
            }),
            SelectorOrder::Random => players.shuffle(&mut rand::rng()),
            SelectorOrder::Arbitrary => {}
        }
        players.truncate(self.max_results);
    }

    fn sort_and_limit_entities(&self, position: DVec3, entities: &mut Vec<SharedEntity>) {
        match self.order {
            SelectorOrder::Nearest => entities.sort_by(|left, right| {
                left.position()
                    .distance_squared(position)
                    .total_cmp(&right.position().distance_squared(position))
            }),
            SelectorOrder::Furthest => entities.sort_by(|left, right| {
                right
                    .position()
                    .distance_squared(position)
                    .total_cmp(&left.position().distance_squared(position))
            }),
            SelectorOrder::Random => entities.shuffle(&mut rand::rng()),
            SelectorOrder::Arbitrary => {}
        }
        entities.truncate(self.max_results);
    }
}

impl SelectorFilter {
    fn matches(
        &self,
        entity: &dyn Entity,
        source: &CommandSource,
    ) -> Result<bool, CommandSyntaxError> {
        match self {
            Self::Alive => Ok(entity.is_alive()),
            Self::Name { value, inverted } => {
                Ok(entity_name_filter_matches(value, *inverted, entity))
            }
            Self::GameMode { value, inverted } => {
                Ok(game_mode_filter_matches(*value, *inverted, entity))
            }
            Self::EntityType { value, inverted } => {
                let matches = entity.entity_type() == *value;
                Ok(matches != *inverted)
            }
            Self::EntityTypeTag { value, inverted } => {
                let matches = REGISTRY.entity_types.is_in_tag(entity.entity_type(), value);
                Ok(matches != *inverted)
            }
            Self::Tag { value, inverted } => {
                let tags = entity.tags();
                let matches = if value.is_empty() {
                    tags.is_empty()
                } else {
                    tags.iter().any(|tag| tag == value)
                };
                Ok(matches != *inverted)
            }
            Self::Team { value, inverted } => {
                let holder_name = entity.scoreboard_name();
                let scoreboard = source_scoreboard(source)?;
                Ok(team_filter_matches(
                    value,
                    *inverted,
                    &holder_name,
                    scoreboard,
                ))
            }
            Self::Nbt { value, inverted } => {
                Ok(entity_nbt_filter_matches(value, *inverted, entity))
            }
            Self::Scores(scores) => {
                let holder_name = entity.scoreboard_name();
                let scoreboard = source_scoreboard(source)?;
                Ok(score_filter_matches(scores, &holder_name, scoreboard))
            }
        }
    }
}

fn entity_nbt_filter_matches(expected: &NbtCompound, inverted: bool, entity: &dyn Entity) -> bool {
    let actual = entity.nbt_for_data_compare();
    compare_nbt_compounds(expected, &actual, true) != inverted
}

fn entity_name_filter_matches(value: &str, inverted: bool, entity: &dyn Entity) -> bool {
    (entity.plain_text_name() == value) != inverted
}

fn game_mode_filter_matches(value: GameType, inverted: bool, entity: &dyn Entity) -> bool {
    let Some(player) = entity.as_player() else {
        return false;
    };
    (player.game_mode() == value) != inverted
}

fn team_filter_matches(
    expected: &str,
    inverted: bool,
    holder_name: &str,
    scoreboard: &Scoreboard,
) -> bool {
    let holder = ScoreHolder::new(holder_name.to_owned());
    let current = scoreboard.holder_team_name(&holder).unwrap_or_default();
    (current == expected) != inverted
}

const fn player_name_matches(actual: &str, expected: &str) -> bool {
    actual.eq_ignore_ascii_case(expected)
}

fn score_filter_matches(
    scores: &[(String, IntRange)],
    holder_name: &str,
    scoreboard: &Scoreboard,
) -> bool {
    let holder = ScoreHolder::new(holder_name.to_owned());
    scores.iter().all(|(objective_name, range)| {
        let Some(objective) = scoreboard.objective(objective_name) else {
            return false;
        };
        scoreboard
            .score(&holder, &objective)
            .is_some_and(|score| range.matches(score))
    })
}

fn source_scoreboard(source: &CommandSource) -> Result<&Scoreboard, CommandSyntaxError> {
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

fn world_candidates(world: &World, aabb: Option<WorldAabb>) -> Vec<SharedEntity> {
    aabb.map_or_else(
        || world.entity_manager().get_accessible_entities(),
        |aabb| world.entity_manager().get_entities_in_aabb(&aabb),
    )
}

pub(crate) fn parse_entity_selector<S>(
    reader: &mut StringReader<'_>,
    source: &S,
    single: bool,
    players_only: bool,
) -> Result<EntitySelector, CommandSyntaxError>
where
    S: CommandArgumentSource + ?Sized,
{
    let start = reader.checkpoint();
    let raw = read_selector_argument(reader)?;
    let allow_selectors = allow_selectors(source);
    let allow_advanced = allow_advanced_selectors(source);
    let selector = parse_selector_plan_with_permissions(&raw, allow_selectors, allow_advanced)
        .map_err(|error| selector_syntax_error(reader, start, &raw, error))?;
    selector
        .validate_for_argument(single, players_only)
        .map_err(|error| selector_syntax_error(reader, start, &raw, error))?;
    Ok(selector)
}

pub(super) fn parse_entity_selector_text(raw: &str) -> Result<EntitySelector, CommandSyntaxError> {
    let mut command_reader = StringReader::new(raw);
    if command_reader.peek() != Some('@') {
        let name = command_reader.read_string()?;
        if command_reader.can_read() {
            return Err(
                command_reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(
                    TextComponent::plain("unexpected trailing selector data"),
                ))),
            );
        }
        return parse_name_or_uuid_value(name).map_err(|error| {
            let mut error_reader = StringReader::new(raw);
            let start = error_reader.checkpoint();
            selector_syntax_error(&mut error_reader, start, raw, error)
        });
    }

    let mut reader = StringReader::new(raw);
    let start = reader.checkpoint();
    parse_selector_plan_with_permissions(raw, true, true)
        .map_err(|error| selector_syntax_error(&mut reader, start, raw, error))
}

fn selector_syntax_error(
    reader: &mut StringReader<'_>,
    start: ReaderCursor,
    raw: &str,
    error: SelectorParseError,
) -> CommandSyntaxError {
    reader.restore(start);
    let end = error.cursor.min(raw.len());
    if raw.is_char_boundary(end) {
        for _ in raw[..end].chars() {
            reader.skip();
        }
    }
    reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(error.message())))
}

fn allow_selectors<S>(source: &S) -> bool
where
    S: CommandArgumentSource + ?Sized,
{
    source.allows_entity_selectors()
}

fn allow_advanced_selectors<S>(source: &S) -> bool
where
    S: CommandArgumentSource + ?Sized,
{
    source.allows_advanced_entity_selectors()
}

fn selector_suggestions(allow_selectors: bool) -> Vec<&'static str> {
    if !allow_selectors {
        return Vec::new();
    }
    vec!["@a", "@e", "@p", "@r", "@s", "@n"]
}

struct SelectorSuggestionData {
    allow_selectors: bool,
    allow_advanced: bool,
    player_names: Vec<String>,
    team_names: Vec<String>,
}

pub(crate) fn suggest_entity_selector<S>(
    builder: &mut SuggestionsBuilder<'_>,
    source: &S,
    single: bool,
    players_only: bool,
) where
    S: CommandArgumentSource + ?Sized,
{
    let data = SelectorSuggestionData {
        allow_selectors: allow_selectors(source),
        allow_advanced: allow_advanced_selectors(source),
        player_names: source.selector_player_names(),
        team_names: source.selector_team_names(),
    };
    for suggestion in
        selector_argument_suggestions(builder.remaining(), players_only, single, &data)
    {
        builder.suggest(suggestion);
    }
}

fn selector_argument_suggestions(
    prefix: &str,
    players_only: bool,
    single: bool,
    data: &SelectorSuggestionData,
) -> Vec<String> {
    if !prefix.starts_with('@') {
        return selector_root_suggestions(prefix, players_only, single, data);
    }

    let mut chars = prefix.chars();
    if chars.next() != Some('@') {
        return Vec::new();
    }
    let Some(selector_type) = chars.next() else {
        return selector_root_suggestions(prefix, players_only, single, data);
    };
    if !selector_type_allowed_for_suggestions(selector_type) {
        return selector_root_suggestions(prefix, players_only, single, data);
    }
    if chars.next().is_some_and(|ch| ch != '[') {
        return selector_root_suggestions(prefix, players_only, single, data);
    }

    if let Some(option_start) = prefix.find('[') {
        if !data.allow_advanced {
            return Vec::new();
        }
        return selector_option_suggestions(prefix, selector_type, option_start, data);
    }

    if !data.allow_advanced {
        return selector_root_suggestions(prefix, players_only, single, data);
    }
    let open_options = format!("@{selector_type}[");
    if open_options.starts_with(prefix) {
        vec![open_options]
    } else {
        selector_root_suggestions(prefix, players_only, single, data)
    }
}

fn selector_root_suggestions(
    prefix: &str,
    _players_only: bool,
    _single: bool,
    data: &SelectorSuggestionData,
) -> Vec<String> {
    let mut suggestions = selector_suggestions(data.allow_selectors)
        .into_iter()
        .filter(|selector| selector.starts_with(prefix))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    suggestions.extend(
        data.player_names
            .iter()
            .filter(|name| matches_generic_suggestion(prefix, name))
            .cloned(),
    );
    suggestions
}

const fn selector_type_allowed_for_suggestions(selector_type: char) -> bool {
    matches!(selector_type, 'a' | 'e' | 'n' | 'p' | 'r' | 's')
}

fn selector_option_suggestions(
    prefix: &str,
    selector_type: char,
    option_start: usize,
    data: &SelectorSuggestionData,
) -> Vec<String> {
    if selector_options_have_top_level_close(&prefix[option_start + 1..]) {
        return Vec::new();
    }

    let option_prefix = &prefix[..=option_start];
    let inside = &prefix[option_start + 1..];
    let (completed_entries, current_entry) = split_current_selector_option_entry(inside);
    let expression_prefix = format!("{option_prefix}{completed_entries}");
    if let Some((key, value_prefix)) = current_entry.split_once('=') {
        let value_expression_prefix = format!("{expression_prefix}{key}=");
        let mut suggestions = selector_option_value_suggestions(
            &value_expression_prefix,
            key.trim(),
            value_prefix,
            completed_entries,
            data,
        );
        suggestions.retain(|suggestion| suggestion != prefix);
        if selector_option_entry_is_complete(selector_type, inside) {
            suggestions.extend(selector_option_delimiter_suggestions(prefix));
        }
        return suggestions;
    }

    let used_set_once_options = completed_set_once_selector_options(completed_entries);
    let mut suggestions = Vec::new();
    if completed_entries.is_empty() && current_entry.trim().is_empty() {
        suggestions.push(format!("{option_prefix}]"));
    }
    suggestions.extend(
        SELECTOR_OPTION_KEYS
            .iter()
            .copied()
            .filter(|key| selector_option_supported_for_suggestions(key))
            .filter(|key| selector_option_available_for_type(key, selector_type))
            .filter(|key| !used_set_once_options.iter().any(|used| used == key))
            .filter(|key| selector_option_available_for_completed_entries(key, completed_entries))
            .filter(|key| matches_generic_suggestion(current_entry.trim_start(), key))
            .map(|key| format!("{expression_prefix}{key}=")),
    );
    suggestions
}

fn selector_option_entry_is_complete(selector_type: char, inside: &str) -> bool {
    parse_selector_plan_with_permissions(&format!("@{selector_type}[{inside}]"), true, true).is_ok()
}

fn selector_option_delimiter_suggestions(prefix: &str) -> Vec<String> {
    [',', ']']
        .iter()
        .map(|delimiter| format!("{prefix}{delimiter}"))
        .collect()
}

fn selector_option_supported_for_suggestions(key: &str) -> bool {
    !UNSUPPORTED_SELECTOR_OPTION_KEYS.contains(&key)
}

fn selector_options_have_top_level_close(input: &str) -> bool {
    let mut state = SelectorSuggestionSplitState::default();
    for (_, ch) in input.char_indices() {
        if state.accepts_top_level_close(ch) {
            return true;
        }
    }
    false
}

fn split_current_selector_option_entry(input: &str) -> (&str, &str) {
    let mut state = SelectorSuggestionSplitState::default();
    let mut separator = None;
    for (index, ch) in input.char_indices() {
        if state.accepts_top_level_separator(ch) {
            separator = Some(index);
        }
    }

    separator.map_or(("", input), |index| (&input[..=index], &input[index + 1..]))
}

fn selector_option_entries(input: &str) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut state = SelectorSuggestionSplitState::default();
    let mut entry_start = 0;
    for (index, ch) in input.char_indices() {
        if state.accepts_top_level_separator(ch) {
            let entry = input[entry_start..index].trim();
            if !entry.is_empty() {
                entries.push(entry);
            }
            entry_start = index + ch.len_utf8();
        }
    }

    let entry = input[entry_start..].trim();
    if !entry.is_empty() {
        entries.push(entry);
    }
    entries
}

#[derive(Default)]
struct SelectorSuggestionSplitState {
    depth: usize,
    quote: Option<char>,
    escaping: bool,
}

impl SelectorSuggestionSplitState {
    const fn accepts_top_level_separator(&mut self, ch: char) -> bool {
        self.accepts_top_level_char(ch, ',')
    }

    const fn accepts_top_level_close(&mut self, ch: char) -> bool {
        self.accepts_top_level_char(ch, ']')
    }

    const fn accepts_top_level_char(&mut self, ch: char, target: char) -> bool {
        if let Some(quote) = self.quote {
            if self.escaping {
                self.escaping = false;
                return false;
            }
            if ch == '\\' {
                self.escaping = true;
                return false;
            }
            if ch == quote {
                self.quote = None;
            }
            return false;
        }

        match ch {
            '"' | '\'' => self.quote = Some(ch),
            '{' | '[' | '(' => self.depth = self.depth.saturating_add(1),
            ']' if self.depth == 0 => return target == ']',
            '}' | ')' | ']' => self.depth = self.depth.saturating_sub(1),
            _ if ch == target && self.depth == 0 => return true,
            _ => {}
        }
        false
    }
}

fn completed_set_once_selector_options(completed_entries: &str) -> Vec<&str> {
    selector_option_entries(completed_entries)
        .into_iter()
        .filter_map(|entry| entry.split_once('=').map(|(key, _)| key.trim()))
        .filter(|key| SET_ONCE_SELECTOR_OPTIONS.contains(key))
        .collect()
}

fn selector_option_available_for_type(key: &str, selector_type: char) -> bool {
    !matches!((key, selector_type), ("limit" | "sort", 's'))
}

fn selector_option_available_for_completed_entries(key: &str, completed_entries: &str) -> bool {
    match key {
        "name" | "gamemode" | "team" => completed_invertable_option_state(completed_entries, key)
            .suggestion_mode()
            .allows_any(),
        "type" => completed_entity_type_suggestion_state(completed_entries)
            .mode
            .allows_any(),
        _ => true,
    }
}

fn selector_option_value_suggestions(
    expression_prefix: &str,
    key: &str,
    value_prefix: &str,
    completed_entries: &str,
    data: &SelectorSuggestionData,
) -> Vec<String> {
    match key {
        "sort" => prefixed_values(
            expression_prefix,
            value_prefix,
            [SORT_NEAREST, SORT_FURTHEST, SORT_RANDOM, SORT_ARBITRARY],
        ),
        "gamemode" => invertible_prefixed_values(
            expression_prefix,
            value_prefix,
            GAME_MODE_SUGGESTIONS,
            completed_invertable_option_state(completed_entries, key).suggestion_mode(),
        ),
        "type" => entity_type_suggestions(
            expression_prefix,
            value_prefix,
            &completed_entity_type_suggestion_state(completed_entries),
        ),
        "team" => team_suggestions(
            expression_prefix,
            value_prefix,
            data,
            completed_invertable_option_state(completed_entries, key).suggestion_mode(),
        ),
        _ => Vec::new(),
    }
}

fn completed_invertable_option_state(completed_entries: &str, key: &str) -> InvertableOptionState {
    let mut state = InvertableOptionState::default();
    for value in completed_option_values(completed_entries, key) {
        let _ = state.parse_element(value.trim_start().starts_with('!'), key);
    }
    state
}

fn completed_option_values<'a>(
    completed_entries: &'a str,
    key: &'a str,
) -> impl Iterator<Item = &'a str> {
    selector_option_entries(completed_entries)
        .into_iter()
        .filter_map(|entry| entry.split_once('='))
        .filter(move |(entry_key, _)| entry_key.trim() == key)
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
}

fn prefixed_values<const N: usize>(
    expression_prefix: &str,
    value_prefix: &str,
    values: [&'static str; N],
) -> Vec<String> {
    values
        .into_iter()
        .filter(|value| value.starts_with(value_prefix))
        .map(|value| format!("{expression_prefix}{value}"))
        .collect()
}

fn invertible_prefixed_values(
    expression_prefix: &str,
    value_prefix: &str,
    values: &[&'static str],
    mode: InvertableSuggestionMode,
) -> Vec<String> {
    let mut suggestions = Vec::new();
    for value in values {
        if mode.allows_positive() {
            push_prefixed_value(&mut suggestions, expression_prefix, value_prefix, value);
        }
        if mode.allows_negative() {
            push_prefixed_value(
                &mut suggestions,
                expression_prefix,
                value_prefix,
                &format!("!{value}"),
            );
        }
    }
    suggestions
}

fn push_prefixed_value(
    suggestions: &mut Vec<String>,
    expression_prefix: &str,
    value_prefix: &str,
    value: &str,
) {
    if matches_generic_suggestion(value_prefix, value) {
        suggestions.push(format!("{expression_prefix}{value}"));
    }
}

#[derive(Clone, Debug)]
struct EntityTypeSuggestionState {
    mode: InvertableSuggestionMode,
    tags_seen: Vec<Identifier>,
}

fn completed_entity_type_suggestion_state(completed_entries: &str) -> EntityTypeSuggestionState {
    let mut state = InvertableOptionState::default();
    let mut tags_seen = Vec::new();
    for value in completed_option_values(completed_entries, "type") {
        let value = value.trim_start();
        if let Some(tag) = value.strip_prefix("!#").or_else(|| value.strip_prefix('#')) {
            if let Some(tag) = parse_resource_identifier_value(tag)
                && !tags_seen.iter().any(|seen| seen == &tag)
            {
                tags_seen.push(tag);
            }
            state.negative_seen = true;
        } else {
            let _ = state.parse_element(value.starts_with('!'), "type");
        }
    }

    EntityTypeSuggestionState {
        mode: state.suggestion_mode(),
        tags_seen,
    }
}

fn entity_type_suggestions(
    expression_prefix: &str,
    value_prefix: &str,
    state: &EntityTypeSuggestionState,
) -> Vec<String> {
    if !state.mode.allows_any() {
        return Vec::new();
    }

    let mut suggestions = Vec::new();
    push_entity_type_tag_suggestions(&mut suggestions, expression_prefix, value_prefix, "", state);
    push_entity_type_tag_suggestions(
        &mut suggestions,
        expression_prefix,
        value_prefix,
        "!",
        state,
    );
    if value_prefix.starts_with('#') || value_prefix.starts_with("!#") {
        return suggestions;
    }

    if state.mode.allows_positive() {
        push_entity_type_id_suggestions(&mut suggestions, expression_prefix, value_prefix, "");
    }
    if state.mode.allows_negative() {
        push_entity_type_id_suggestions(&mut suggestions, expression_prefix, value_prefix, "!");
    }

    suggestions
}

fn push_entity_type_id_suggestions(
    suggestions: &mut Vec<String>,
    expression_prefix: &str,
    value_prefix: &str,
    inversion: &str,
) {
    let resource_prefix = if inversion.is_empty() {
        if value_prefix.starts_with('!') || value_prefix.starts_with('#') {
            return;
        }
        value_prefix
    } else if let Some(prefix) = value_prefix.strip_prefix(inversion) {
        prefix
    } else if inversion.starts_with(value_prefix) {
        ""
    } else {
        return;
    };

    let stripped_prefix = resource_prefix
        .strip_prefix("minecraft:")
        .unwrap_or(resource_prefix);
    suggestions.extend(
        REGISTRY
            .entity_types
            .iter()
            .map(|(_, entity_type)| entity_type.key.to_string())
            .filter(|key| {
                let text = key.strip_prefix("minecraft:").unwrap_or(key);
                matches_suggestion_substring(stripped_prefix, text)
            })
            .map(|key| format!("{expression_prefix}{inversion}{key}")),
    );
}

fn push_entity_type_tag_suggestions(
    suggestions: &mut Vec<String>,
    expression_prefix: &str,
    value_prefix: &str,
    inversion: &str,
    state: &EntityTypeSuggestionState,
) {
    let marker = format!("{inversion}#");
    if !marker.starts_with(value_prefix) && !value_prefix.starts_with(&marker) {
        return;
    }

    let tag_prefix = value_prefix.strip_prefix(&marker).unwrap_or_default();
    let tag_prefix = tag_prefix.strip_prefix("minecraft:").unwrap_or(tag_prefix);
    let mut tag_keys = REGISTRY.entity_types.tag_keys().collect::<Vec<_>>();
    tag_keys.sort_by(|left, right| {
        left.namespace
            .cmp(&right.namespace)
            .then_with(|| left.path.cmp(&right.path))
    });
    suggestions.extend(
        tag_keys
            .into_iter()
            .filter(|key| !state.tags_seen.iter().any(|seen| seen == *key))
            .filter(|key| {
                if key.namespace == Identifier::VANILLA_NAMESPACE {
                    return matches_suggestion_substring(tag_prefix, &key.path);
                }

                let text = key.to_string();
                matches_suggestion_substring(tag_prefix, &text)
            })
            .map(|key| format!("{expression_prefix}{marker}{key}")),
    );
}

fn team_suggestions(
    expression_prefix: &str,
    value_prefix: &str,
    data: &SelectorSuggestionData,
    mode: InvertableSuggestionMode,
) -> Vec<String> {
    let mut suggestions = Vec::new();
    for team_name in &data.team_names {
        if mode.allows_positive() {
            push_prefixed_value(&mut suggestions, expression_prefix, value_prefix, team_name);
        }
        if mode.allows_negative() {
            push_prefixed_value(
                &mut suggestions,
                expression_prefix,
                value_prefix,
                &format!("!{team_name}"),
            );
        }
    }
    suggestions
}

fn read_selector_argument(reader: &mut StringReader<'_>) -> Result<String, CommandSyntaxError> {
    if reader.peek() != Some('@') {
        return reader.read_string();
    }

    let mut value = String::new();
    let mut option_depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    while let Some(ch) = reader.peek() {
        if option_depth == 0 && !value.is_empty() && java::is_whitespace(ch) {
            break;
        }
        value.push(ch);
        reader.skip();
        if escaped {
            escaped = false;
            continue;
        }
        if quote.is_some() {
            if ch == '\\' {
                escaped = true;
            } else if quote == Some(ch) {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '[' => option_depth += 1,
            ']' if option_depth > 0 => {
                option_depth -= 1;
                if option_depth == 0 {
                    break;
                }
            }
            _ => {}
        }
    }
    if value.is_empty() {
        return Err(reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(
            TextComponent::from(&translations::ARGUMENT_ENTITY_INVALID),
        ))));
    }
    Ok(value)
}

fn matches_suggestion_substring(pattern: &str, input: &str) -> bool {
    if input.starts_with(pattern) {
        return true;
    }
    input.char_indices().any(|(index, character)| {
        matches!(character, '.' | '_' | '/')
            && input[index + character.len_utf8()..].starts_with(pattern)
    })
}

fn matches_generic_suggestion(pattern: &str, input: &str) -> bool {
    matches_suggestion_substring(&pattern.to_lowercase(), &input.to_lowercase())
}

fn is_valid_selector_name(name: &str) -> bool {
    !name.is_empty() && name.encode_utf16().count() <= 16
}

#[cfg(test)]
fn parse_selector_plan(
    raw: &str,
    allow_selectors: bool,
) -> Result<EntitySelector, SelectorParseError> {
    parse_selector_plan_with_permissions(raw, allow_selectors, allow_selectors)
}

fn parse_selector_plan_with_permissions(
    raw: &str,
    allow_selectors: bool,
    allow_advanced_selectors: bool,
) -> Result<EntitySelector, SelectorParseError> {
    let mut reader = SelectorReader::new(raw);
    if reader.peek() == Some('@') {
        if !allow_selectors {
            return Err(SelectorParseError::not_allowed(reader.cursor()));
        }
        reader.read();
        parse_selector_type(&mut reader, allow_advanced_selectors)
    } else {
        parse_name_or_uuid(&mut reader)
    }
}

fn parse_name_or_uuid(
    reader: &mut SelectorReader<'_>,
) -> Result<EntitySelector, SelectorParseError> {
    let name = reader.read_remaining();
    parse_name_or_uuid_value(name)
}

fn parse_name_or_uuid_value(name: String) -> Result<EntitySelector, SelectorParseError> {
    if let Ok(uuid) = Uuid::parse_str(&name) {
        return Ok(EntitySelector::new(
            SelectorKind::EntityUuid(uuid),
            1,
            true,
            false,
            SelectorOrder::Arbitrary,
        ));
    }
    if !is_valid_selector_name(&name) {
        return Err(SelectorParseError::invalid_at(
            "invalid player name or UUID",
            0,
        ));
    }
    Ok(EntitySelector::new(
        SelectorKind::PlayerName(name),
        1,
        false,
        false,
        SelectorOrder::Arbitrary,
    ))
}

fn parse_selector_type(
    reader: &mut SelectorReader<'_>,
    allow_advanced_selectors: bool,
) -> Result<EntitySelector, SelectorParseError> {
    let selector_start = reader.cursor();
    let Some(selector_type) = reader.read() else {
        return Err(SelectorParseError::invalid_at(
            "missing selector type",
            selector_start,
        ));
    };

    let selector_type = match selector_type {
        'a' => SelectorType::AllPlayers,
        'e' => SelectorType::AllEntities,
        'n' => SelectorType::NearestEntity,
        'p' => SelectorType::NearestPlayer,
        'r' => SelectorType::RandomPlayer,
        's' => SelectorType::SelfEntity,
        other => {
            return Err(SelectorParseError::invalid_at(
                format!("unknown selector type '@{other}'"),
                selector_start,
            ));
        }
    };
    let mut selector = EntitySelector::for_selector_type(selector_type);

    if reader.peek() == Some('[') {
        reader.read();
        parse_options(reader, &mut selector, allow_advanced_selectors)?;
    }
    if reader.can_read() {
        return Err(SelectorParseError::invalid_at(
            "unexpected trailing selector data",
            reader.cursor(),
        ));
    }
    Ok(selector)
}

fn parse_options(
    reader: &mut SelectorReader<'_>,
    selector: &mut EntitySelector,
    allow_advanced_selectors: bool,
) -> Result<(), SelectorParseError> {
    let mut state = SelectorOptionState::default();
    reader.skip_whitespace();
    while reader.peek().is_some_and(|ch| ch != ']') {
        if !allow_advanced_selectors {
            return Err(SelectorParseError::advanced_not_allowed(reader.cursor()));
        }
        reader.skip_whitespace();
        let key_cursor = reader.cursor();
        let key = reader.read_key()?;
        selector.uses_advanced_options = true;
        reader.skip_whitespace();
        reader.expect('=')?;
        reader.skip_whitespace();
        parse_option(reader, selector, &mut state, &key, key_cursor)?;
        reader.skip_whitespace();
        match reader.peek() {
            Some(',') => {
                reader.read();
                reader.skip_whitespace();
            }
            Some(']') => break,
            Some(_) => {
                return Err(SelectorParseError::invalid_at(
                    "expected ',' or ']' after selector option",
                    reader.cursor(),
                ));
            }
            None => {
                return Err(SelectorParseError::invalid_at(
                    "expected ']' to end selector options",
                    reader.cursor(),
                ));
            }
        }
    }
    reader.expect(']')?;
    Ok(())
}

fn parse_option(
    reader: &mut SelectorReader<'_>,
    selector: &mut EntitySelector,
    state: &mut SelectorOptionState,
    key: &str,
    key_cursor: usize,
) -> Result<(), SelectorParseError> {
    match key {
        "name" => parse_name_option(reader, selector, state),
        "distance" => parse_distance_option(reader, selector, state, key_cursor),
        "level" => parse_level_option(reader, selector, state, key_cursor),
        "x" => {
            ensure_set_once(&mut state.x, "x", key_cursor)?;
            selector.world_limited = true;
            selector.position.x = Some(reader.read_f64()?);
            Ok(())
        }
        "y" => {
            ensure_set_once(&mut state.y, "y", key_cursor)?;
            selector.world_limited = true;
            selector.position.y = Some(reader.read_f64()?);
            Ok(())
        }
        "z" => {
            ensure_set_once(&mut state.z, "z", key_cursor)?;
            selector.world_limited = true;
            selector.position.z = Some(reader.read_f64()?);
            Ok(())
        }
        "dx" => {
            ensure_set_once(&mut state.dx, "dx", key_cursor)?;
            selector.world_limited = true;
            selector.delta.x = Some(reader.read_f64()?);
            Ok(())
        }
        "dy" => {
            ensure_set_once(&mut state.dy, "dy", key_cursor)?;
            selector.world_limited = true;
            selector.delta.y = Some(reader.read_f64()?);
            Ok(())
        }
        "dz" => {
            ensure_set_once(&mut state.dz, "dz", key_cursor)?;
            selector.world_limited = true;
            selector.delta.z = Some(reader.read_f64()?);
            Ok(())
        }
        "x_rotation" => {
            ensure_set_once(&mut state.x_rotation, "x_rotation", key_cursor)?;
            let value_cursor = reader.cursor();
            selector.x_rotation =
                Some(parse_float_range(&reader.read_range_value(), value_cursor)?);
            Ok(())
        }
        "y_rotation" => {
            ensure_set_once(&mut state.y_rotation, "y_rotation", key_cursor)?;
            let value_cursor = reader.cursor();
            selector.y_rotation =
                Some(parse_float_range(&reader.read_range_value(), value_cursor)?);
            Ok(())
        }
        "limit" => parse_limit_option(reader, selector, state, key_cursor),
        "sort" => parse_sort_option(reader, selector, state, key_cursor),
        "gamemode" => parse_gamemode_option(reader, selector, state),
        "type" => parse_type_option(reader, selector, state),
        "tag" => {
            parse_tag_option(reader, selector);
            Ok(())
        }
        "team" => parse_team_option(reader, selector, state),
        "nbt" => parse_nbt_option(reader, selector),
        "scores" => parse_scores_option(reader, selector, state, key_cursor),
        "predicate" => Err(SelectorParseError::unsupported(
            "predicate needs a reloadable or plugin predicate registry",
            key_cursor,
        )),
        "advancements" => Err(SelectorParseError::unsupported(
            "advancements needs player advancement foundation",
            key_cursor,
        )),
        _ => Err(SelectorParseError::invalid_at(
            format!("unknown selector option '{key}'"),
            key_cursor,
        )),
    }
}

fn parse_name_option(
    reader: &mut SelectorReader<'_>,
    selector: &mut EntitySelector,
    state: &mut SelectorOptionState,
) -> Result<(), SelectorParseError> {
    let value_cursor = reader.cursor();
    let inverted = reader.read_inversion();
    state
        .name
        .parse_element(inverted, "name")
        .map_err(|error| {
            SelectorParseError::invalid_at(
                match error.kind {
                    SelectorParseErrorKind::Invalid(message) => *message,
                    SelectorParseErrorKind::NotAllowed
                    | SelectorParseErrorKind::AdvancedNotAllowed
                    | SelectorParseErrorKind::Unsupported(_) => {
                        TextComponent::from("invalid name option")
                    }
                },
                value_cursor,
            )
        })?;
    let value = reader.read_string()?;
    selector
        .filters
        .push(SelectorFilter::Name { value, inverted });
    Ok(())
}

fn parse_scores_option(
    reader: &mut SelectorReader<'_>,
    selector: &mut EntitySelector,
    state: &mut SelectorOptionState,
    key_cursor: usize,
) -> Result<(), SelectorParseError> {
    ensure_set_once(&mut state.scores, "scores", key_cursor)?;
    let scores = reader.read_scores()?;
    if !scores.is_empty() {
        selector.filters.push(SelectorFilter::Scores(scores));
    }
    Ok(())
}

fn parse_distance_option(
    reader: &mut SelectorReader<'_>,
    selector: &mut EntitySelector,
    state: &mut SelectorOptionState,
    key_cursor: usize,
) -> Result<(), SelectorParseError> {
    ensure_set_once(&mut state.distance, "distance", key_cursor)?;
    let value_cursor = reader.cursor();
    let range = parse_double_range(&reader.read_range_value(), value_cursor)?;
    if range.min.is_some_and(|value| value < 0.0) || range.max.is_some_and(|value| value < 0.0) {
        return Err(SelectorParseError::invalid_at(
            "distance cannot be negative",
            key_cursor,
        ));
    }
    selector.distance = Some(range);
    selector.world_limited = true;
    Ok(())
}

fn parse_level_option(
    reader: &mut SelectorReader<'_>,
    selector: &mut EntitySelector,
    state: &mut SelectorOptionState,
    key_cursor: usize,
) -> Result<(), SelectorParseError> {
    ensure_set_once(&mut state.level, "level", key_cursor)?;
    let value_cursor = reader.cursor();
    let range = parse_int_range(&reader.read_range_value(), value_cursor)?;
    if range.min.is_some_and(|value| value < 0) || range.max.is_some_and(|value| value < 0) {
        return Err(SelectorParseError::invalid_at(
            "level cannot be negative",
            key_cursor,
        ));
    }
    selector.level = Some(range);
    selector.includes_entities = false;
    Ok(())
}

fn parse_limit_option(
    reader: &mut SelectorReader<'_>,
    selector: &mut EntitySelector,
    state: &mut SelectorOptionState,
    key_cursor: usize,
) -> Result<(), SelectorParseError> {
    if selector.current_entity {
        return Err(SelectorParseError::invalid_at(
            "limit cannot be used with @s",
            key_cursor,
        ));
    }
    ensure_set_once(&mut state.limit, "limit", key_cursor)?;
    let value = reader.read_i32()?;
    if value < 1 {
        return Err(SelectorParseError::invalid_at(
            "limit must be at least 1",
            key_cursor,
        ));
    }
    selector.max_results = value as usize;
    Ok(())
}

fn parse_sort_option(
    reader: &mut SelectorReader<'_>,
    selector: &mut EntitySelector,
    state: &mut SelectorOptionState,
    key_cursor: usize,
) -> Result<(), SelectorParseError> {
    if selector.current_entity {
        return Err(SelectorParseError::invalid_at(
            "sort cannot be used with @s",
            key_cursor,
        ));
    }
    ensure_set_once(&mut state.sort, "sort", key_cursor)?;
    let value = reader.read_required_unquoted_string()?;
    selector.order = match value.as_str() {
        SORT_NEAREST => SelectorOrder::Nearest,
        SORT_FURTHEST => SelectorOrder::Furthest,
        SORT_RANDOM => SelectorOrder::Random,
        SORT_ARBITRARY => SelectorOrder::Arbitrary,
        _ => {
            return Err(SelectorParseError::invalid_at(
                format!("unknown sort '{value}'"),
                key_cursor,
            ));
        }
    };
    Ok(())
}

fn parse_gamemode_option(
    reader: &mut SelectorReader<'_>,
    selector: &mut EntitySelector,
    state: &mut SelectorOptionState,
) -> Result<(), SelectorParseError> {
    let value_cursor = reader.cursor();
    let inverted = reader.read_inversion();
    state.gamemode.parse_element(inverted, "gamemode")?;
    let value = reader.read_required_unquoted_string()?;
    let Some(game_mode) = parse_game_mode(&value) else {
        return Err(SelectorParseError::invalid_at(
            format!("invalid game mode '{value}'"),
            value_cursor,
        ));
    };
    selector.includes_entities = false;
    selector.filters.push(SelectorFilter::GameMode {
        value: game_mode,
        inverted,
    });
    Ok(())
}

fn parse_type_option(
    reader: &mut SelectorReader<'_>,
    selector: &mut EntitySelector,
    state: &mut SelectorOptionState,
) -> Result<(), SelectorParseError> {
    let value_cursor = reader.cursor();
    let inverted = reader.read_inversion();
    if reader.peek() == Some('#') {
        reader.read();
        reader.skip_whitespace();
        let value = read_identifier(reader, value_cursor)?;
        state.entity_type.parse_tag(&value, "type")?;
        selector
            .filters
            .push(SelectorFilter::EntityTypeTag { value, inverted });
        return Ok(());
    }

    state.entity_type.parse_element(inverted, "type")?;
    let key = read_identifier(reader, value_cursor)?;
    let Some(entity_type) = REGISTRY.entity_types.by_key(&key) else {
        return Err(SelectorParseError::invalid_at(
            format!("invalid entity type '{key}'"),
            value_cursor,
        ));
    };
    if entity_type == &vanilla_entities::PLAYER && !inverted {
        selector.includes_entities = false;
    }
    selector.filters.push(SelectorFilter::EntityType {
        value: entity_type,
        inverted,
    });
    Ok(())
}

fn parse_tag_option(reader: &mut SelectorReader<'_>, selector: &mut EntitySelector) {
    let inverted = reader.read_inversion();
    let value = reader.read_unquoted_string();
    selector
        .filters
        .push(SelectorFilter::Tag { value, inverted });
}

fn parse_team_option(
    reader: &mut SelectorReader<'_>,
    selector: &mut EntitySelector,
    state: &mut SelectorOptionState,
) -> Result<(), SelectorParseError> {
    let inverted = reader.read_inversion();
    state.team.parse_element(inverted, "team")?;
    let value = reader.read_unquoted_string();
    selector
        .filters
        .push(SelectorFilter::Team { value, inverted });
    Ok(())
}

fn parse_nbt_option(
    reader: &mut SelectorReader<'_>,
    selector: &mut EntitySelector,
) -> Result<(), SelectorParseError> {
    let inverted = reader.read_inversion();
    let value = reader.read_nbt()?;
    selector
        .filters
        .push(SelectorFilter::Nbt { value, inverted });
    Ok(())
}

fn read_identifier(
    reader: &mut SelectorReader<'_>,
    value_cursor: usize,
) -> Result<Identifier, SelectorParseError> {
    let value = reader.read_identifier_string();
    parse_resource_identifier_value(&value).ok_or_else(|| {
        SelectorParseError::invalid_at(format!("invalid identifier '{value}'"), value_cursor)
    })
}

fn parse_resource_identifier_value(value: &str) -> Option<Identifier> {
    let (namespace, path) = value.split_once(':').map_or(
        (Identifier::VANILLA_NAMESPACE, value),
        |(namespace, path)| {
            if namespace.is_empty() {
                (Identifier::VANILLA_NAMESPACE, path)
            } else {
                (namespace, path)
            }
        },
    );
    Identifier::validate(namespace, path)
        .then(|| Identifier::new(namespace.to_owned(), path.to_owned()))
}

fn ensure_set_once(seen: &mut bool, option: &str, cursor: usize) -> Result<(), SelectorParseError> {
    if *seen {
        return Err(SelectorParseError::invalid_at(
            format!("option '{option}' cannot be repeated"),
            cursor,
        ));
    }
    *seen = true;
    Ok(())
}

fn selector_position(selector: &EntitySelector, source: &CommandSource) -> DVec3 {
    let base = if selector.requires_position() {
        source.position()
    } else {
        DVec3::ZERO
    };
    selector.position.apply(base)
}

fn find_entity_by_uuid(source: &CommandSource, uuid: &Uuid) -> Option<SharedEntity> {
    source
        .server()
        .worlds
        .worlds_in_domain(source.world().domain())
        .into_iter()
        .find_map(|world| {
            let entity = world.get_entity_by_uuid(uuid)?;
            world.get_accessible_entity_by_id(entity.id())
        })
}

fn create_delta_aabb(x: f64, y: f64, z: f64) -> WorldAabb {
    let min = DVec3::new(
        if x < 0.0 { x } else { 0.0 },
        if y < 0.0 { y } else { 0.0 },
        if z < 0.0 { z } else { 0.0 },
    );
    let max = DVec3::new(
        if x < 0.0 { 0.0 } else { x } + 1.0,
        if y < 0.0 { 0.0 } else { y } + 1.0,
        if z < 0.0 { 0.0 } else { z } + 1.0,
    );
    WorldAabb::from_min_max(min, max)
}

fn parse_game_mode(value: &str) -> Option<GameType> {
    match value {
        "survival" => Some(GameType::Survival),
        "creative" => Some(GameType::Creative),
        "adventure" => Some(GameType::Adventure),
        "spectator" => Some(GameType::Spectator),
        _ => None,
    }
}

fn parse_double_range(raw: &str, cursor: usize) -> Result<DoubleRange, SelectorParseError> {
    let (min, max) = parse_range(raw, cursor, str::parse::<f64>)?;
    if let (Some(min), Some(max)) = (min, max)
        && min > max
    {
        return Err(SelectorParseError::invalid_at(
            "range minimum exceeds maximum",
            cursor,
        ));
    }
    Ok(DoubleRange { min, max })
}

fn parse_float_range(raw: &str, cursor: usize) -> Result<FloatRange, SelectorParseError> {
    let (min, max) = parse_range(raw, cursor, str::parse::<f32>)?;
    Ok(FloatRange { min, max })
}

fn parse_int_range(raw: &str, cursor: usize) -> Result<IntRange, SelectorParseError> {
    let (min, max) = parse_range(raw, cursor, str::parse::<i32>)?;
    if let (Some(min), Some(max)) = (min, max)
        && min > max
    {
        return Err(SelectorParseError::invalid_at(
            "range minimum exceeds maximum",
            cursor,
        ));
    }
    Ok(IntRange { min, max })
}

fn parse_range<T: Copy, E>(
    raw: &str,
    cursor: usize,
    parse: impl Fn(&str) -> Result<T, E>,
) -> Result<(Option<T>, Option<T>), SelectorParseError> {
    if raw.is_empty() {
        return Err(SelectorParseError::invalid_at(
            "missing range value",
            cursor,
        ));
    }
    let Some((left, right)) = raw.split_once("..") else {
        let value = parse(raw)
            .map_err(|_| SelectorParseError::invalid_at("invalid range value", cursor))?;
        return Ok((Some(value), Some(value)));
    };
    if left.is_empty() && right.is_empty() {
        return Err(SelectorParseError::invalid_at("empty range", cursor));
    }
    let right_cursor = cursor + left.len() + "..".len();
    let min = if left.is_empty() {
        None
    } else {
        Some(
            parse(left)
                .map_err(|_| SelectorParseError::invalid_at("invalid range minimum", cursor))?,
        )
    };
    let max =
        if right.is_empty() {
            None
        } else {
            Some(parse(right).map_err(|_| {
                SelectorParseError::invalid_at("invalid range maximum", right_cursor)
            })?)
        };
    Ok((min, max))
}

fn wrap_degrees(value: f32) -> f32 {
    let mut value = value % 360.0;
    if value >= 180.0 {
        value -= 360.0;
    }
    if value < -180.0 {
        value += 360.0;
    }
    value
}

#[derive(Clone)]
struct SelectorReader<'a> {
    input: &'a str,
    cursor: usize,
}

impl<'a> SelectorReader<'a> {
    const fn new(input: &'a str) -> Self {
        Self { input, cursor: 0 }
    }

    const fn cursor(&self) -> usize {
        self.cursor
    }

    const fn can_read(&self) -> bool {
        self.cursor < self.input.len()
    }

    fn remaining(&self) -> &'a str {
        &self.input[self.cursor..]
    }

    fn peek(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    fn read(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.cursor += ch.len_utf8();
        Some(ch)
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(java::is_whitespace) {
            self.read();
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), SelectorParseError> {
        if self.peek() == Some(expected) {
            self.read();
            Ok(())
        } else {
            Err(SelectorParseError::invalid_at(
                format!("expected '{expected}'"),
                self.cursor,
            ))
        }
    }

    fn read_remaining(&mut self) -> String {
        let value = self.remaining().to_owned();
        self.cursor = self.input.len();
        value
    }

    fn read_key(&mut self) -> Result<String, SelectorParseError> {
        let start = self.cursor;
        if self.peek().is_some_and(is_quoted_string_start) {
            return self.read_quoted_string();
        }

        let key = self.read_unquoted_string();
        if key.is_empty() {
            return Err(SelectorParseError::invalid_at(
                "expected selector option name",
                start,
            ));
        }
        Ok(key)
    }

    fn read_scores(&mut self) -> Result<Vec<(String, IntRange)>, SelectorParseError> {
        self.expect('{')?;
        let mut scores = Vec::new();
        self.skip_whitespace();
        while self.peek().is_some_and(|ch| ch != '}') {
            self.skip_whitespace();
            let name = self.read_unquoted_string();
            self.skip_whitespace();
            self.expect('=')?;
            self.skip_whitespace();
            let range_cursor = self.cursor;
            let range = parse_int_range(&self.read_range_value(), range_cursor)?;
            upsert_score_filter(&mut scores, name, range);
            self.skip_whitespace();
            match self.peek() {
                Some(',') => {
                    self.read();
                    self.skip_whitespace();
                }
                Some('}') | None => {}
                Some(_) => {
                    return Err(SelectorParseError::invalid_at(
                        "expected ',' or '}' after score range",
                        self.cursor,
                    ));
                }
            }
        }
        self.expect('}')?;
        Ok(scores)
    }

    fn read_unquoted_string(&mut self) -> String {
        let start = self.cursor;
        while self.peek().is_some_and(is_brigadier_unquoted_char) {
            self.read();
        }
        self.input[start..self.cursor].to_owned()
    }

    fn read_required_unquoted_string(&mut self) -> Result<String, SelectorParseError> {
        let start = self.cursor;
        let value = self.read_unquoted_string();
        if value.is_empty() {
            return Err(SelectorParseError::invalid_at(
                "expected selector option value",
                start,
            ));
        }
        Ok(value)
    }

    fn read_identifier_string(&mut self) -> String {
        let start = self.cursor;
        while self.peek().is_some_and(is_identifier_char) {
            self.read();
        }
        self.input[start..self.cursor].to_owned()
    }

    fn read_number_string(&mut self) -> String {
        self.skip_whitespace();
        let start = self.cursor;
        while self.peek().is_some_and(is_number_char) {
            self.read();
        }
        self.input[start..self.cursor].to_owned()
    }

    fn read_range_value(&mut self) -> String {
        self.skip_whitespace();
        let start = self.cursor;
        self.read_range_number();
        if self.peek() == Some('.') && self.peek_next() == Some('.') {
            self.read();
            self.read();
            self.read_range_number();
        }
        self.input[start..self.cursor].to_owned()
    }

    fn read_range_number(&mut self) {
        while self
            .peek()
            .is_some_and(|ch| is_range_number_char(ch, self.peek_next()))
        {
            self.read();
        }
    }

    fn peek_next(&self) -> Option<char> {
        let mut chars = self.remaining().chars();
        chars.next()?;
        chars.next()
    }

    fn read_nbt(&mut self) -> Result<NbtCompound, SelectorParseError> {
        let nbt_cursor = self.cursor;
        let (nbt, consumed) =
            parse_snbt_compound_argument(&self.input[self.cursor..]).map_err(|error| {
                SelectorParseError::invalid_at(error.component(), nbt_cursor + error.cursor())
            })?;
        self.cursor += consumed;
        Ok(nbt)
    }

    fn read_inversion(&mut self) -> bool {
        self.skip_whitespace();
        if self.peek() == Some('!') {
            self.read();
            self.skip_whitespace();
            true
        } else {
            false
        }
    }

    fn read_i32(&mut self) -> Result<i32, SelectorParseError> {
        let cursor = self.cursor;
        let value = self.read_number_string();
        value.parse().map_err(|_| {
            SelectorParseError::invalid_at(format!("invalid integer '{value}'"), cursor)
        })
    }

    fn read_f64(&mut self) -> Result<f64, SelectorParseError> {
        let cursor = self.cursor;
        let value = self.read_number_string();
        value.parse().map_err(|_| {
            SelectorParseError::invalid_at(format!("invalid double '{value}'"), cursor)
        })
    }

    fn read_string(&mut self) -> Result<String, SelectorParseError> {
        self.skip_whitespace();
        match self.peek() {
            Some(ch) if is_quoted_string_start(ch) => self.read_quoted_string(),
            _ => Ok(self.read_unquoted_string()),
        }
    }

    fn read_quoted_string(&mut self) -> Result<String, SelectorParseError> {
        let start = self.cursor;
        let Some(quote) = self.read() else {
            return Err(SelectorParseError::invalid_at(
                "expected quoted string",
                start,
            ));
        };
        let mut value = String::new();
        while let Some(ch) = self.read() {
            match ch {
                ch if ch == quote => return Ok(value),
                '\\' => {
                    let Some(escaped) = self.read() else {
                        return Err(SelectorParseError::invalid_at("unclosed quote", start));
                    };
                    if escaped != quote && escaped != '\\' {
                        return Err(SelectorParseError::invalid_at(
                            format!("invalid escape '{escaped}'"),
                            self.cursor,
                        ));
                    }
                    value.push(escaped);
                }
                _ => value.push(ch),
            }
        }
        Err(SelectorParseError::invalid_at("unclosed quote", start))
    }
}

fn upsert_score_filter(scores: &mut Vec<(String, IntRange)>, name: String, range: IntRange) {
    if let Some((_, existing)) = scores
        .iter_mut()
        .find(|(existing_name, _)| existing_name == &name)
    {
        *existing = range;
        return;
    }
    scores.push((name, range));
}

const fn is_brigadier_unquoted_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '+')
}

const fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_digit() || matches!(ch, 'a'..='z' | '_' | ':' | '/' | '.' | '-')
}

const fn is_number_char(ch: char) -> bool {
    ch.is_ascii_digit() || matches!(ch, '.' | '-')
}

fn is_range_number_char(ch: char, next: Option<char>) -> bool {
    ch.is_ascii_digit() || ch == '-' || (ch == '.' && next != Some('.'))
}

const fn is_quoted_string_start(ch: char) -> bool {
    matches!(ch, '"' | '\'')
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use simdnbt::owned::{NbtCompound, NbtTag};
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::{test_support::init_test_registry, vanilla_entities};
    use steel_utils::types::GameType;
    use text_components::{TextComponent, content::Content};

    use crate::{
        command::{
            brigadier::{CommandSyntaxError, StringReader, Suggestion, SuggestionsBuilder},
            execution::{CommandArgumentSource, CommandResultCallback, ExecutionCommandSource},
        },
        entity::{Entity, EntityBase},
        scoreboard::{ScoreHolder, Scoreboard},
    };

    use super::{
        EntitySelector, IntRange, SelectorFilter, SelectorKind, SelectorParseErrorKind,
        SelectorType, entity_name_filter_matches, entity_nbt_filter_matches,
        game_mode_filter_matches, parse_entity_selector, parse_selector_plan,
        read_selector_argument, score_filter_matches, suggest_entity_selector, team_filter_matches,
    };

    struct SelectorTestEntity {
        base: EntityBase,
    }

    impl SelectorTestEntity {
        fn new() -> Self {
            Self {
                base: EntityBase::new(
                    1,
                    DVec3::ZERO,
                    vanilla_entities::ITEM.dimensions,
                    Weak::new(),
                ),
            }
        }
    }

    crate::entity::impl_test_downcast_type!(SelectorTestEntity);

    impl Entity for SelectorTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::ITEM
        }
    }

    struct TestSource {
        selectors: bool,
        advanced: bool,
        callback: CommandResultCallback,
    }

    impl TestSource {
        const fn new(selectors: bool, advanced: bool) -> Self {
            Self {
                selectors,
                advanced,
                callback: CommandResultCallback::empty(),
            }
        }
    }

    impl ExecutionCommandSource for TestSource {
        fn with_callback(&self, callback: CommandResultCallback) -> Self {
            Self {
                selectors: self.selectors,
                advanced: self.advanced,
                callback,
            }
        }

        fn callback(&self) -> CommandResultCallback {
            self.callback.clone()
        }

        fn handle_error(&self, _error: &CommandSyntaxError, _forked: bool) {}
    }

    impl CommandArgumentSource for TestSource {
        fn selector_player_names(&self) -> Vec<String> {
            vec!["Alex".to_owned(), "Steve".to_owned()]
        }

        fn selector_team_names(&self) -> Vec<String> {
            vec!["red".to_owned()]
        }

        fn allows_entity_selectors(&self) -> bool {
            self.selectors
        }

        fn allows_advanced_entity_selectors(&self) -> bool {
            self.advanced
        }
    }

    fn parse(
        input: &str,
        source: &TestSource,
        single: bool,
        players_only: bool,
    ) -> Result<EntitySelector, CommandSyntaxError> {
        parse_entity_selector(&mut StringReader::new(input), source, single, players_only)
    }

    #[test]
    fn selector_permissions_distinguish_basic_and_advanced_syntax() {
        let denied = TestSource::new(false, false);
        assert!(parse("Steve", &denied, true, true).is_ok());
        assert!(parse("@s", &denied, true, false).is_err());

        let basic = TestSource::new(true, false);
        assert!(parse("@e", &basic, false, false).is_ok());
        assert!(parse("@e[]", &basic, false, false).is_ok());
        assert!(parse("@e[distance=..5]", &basic, false, false).is_err());
    }

    #[test]
    fn selector_argument_shapes_enforce_cardinality_and_player_only_rules() {
        let source = TestSource::new(true, true);

        assert!(parse("@e", &source, true, false).is_err());
        assert!(parse("@e[limit=1]", &source, true, false).is_ok());
        assert!(parse("@e", &source, false, true).is_err());
        assert!(parse("@a", &source, false, true).is_ok());
        assert!(parse("@s", &source, true, true).is_ok());
    }

    #[test]
    fn selector_parses_core_filters_and_nested_snbt() {
        init_test_registry();
        let Ok(selector) = parse_selector_plan(
            "@e[type=pig,nbt={Tags:[\"foo\"]},scores={kills=1..},team=!red]",
            true,
        ) else {
            panic!("selector should parse");
        };

        assert!(matches!(
            selector.kind,
            SelectorKind::Selector(SelectorType::AllEntities)
        ));
        assert!(selector.filters.iter().any(|filter| matches!(
            filter,
            SelectorFilter::EntityType { value, inverted: false }
                if **value == vanilla_entities::PIG
        )));
        assert!(selector.filters.iter().any(|filter| matches!(
            filter,
            SelectorFilter::Nbt {
                inverted: false,
                ..
            }
        )));
        assert!(
            selector.filters.iter().any(
                |filter| matches!(filter, SelectorFilter::Scores(scores) if scores.len() == 1)
            )
        );
        assert!(selector.filters.iter().any(|filter| matches!(
            filter,
            SelectorFilter::Team { value, inverted: true } if value == "red"
        )));
    }

    #[test]
    fn selector_preserves_translatable_snbt_errors() {
        let Err(error) = parse_selector_plan("@e[nbt={id:}]", true) else {
            panic!("missing selector NBT value should fail");
        };

        let SelectorParseErrorKind::Invalid(component) = error.kind else {
            panic!("selector error should preserve its text component");
        };
        assert!(matches!(
            component.content,
            Content::Translate(ref message)
                if message.key == "snbt.parser.expected_unquoted_string"
        ));
    }

    #[test]
    fn selector_reader_leaves_following_command_input_unconsumed() {
        let mut reader = StringReader::new("@e[nbt={Tags:[\"foo]bar\"],data:{x:1b}}] next");
        let Ok(raw) = read_selector_argument(&mut reader) else {
            panic!("selector should be read");
        };

        assert_eq!(raw, "@e[nbt={Tags:[\"foo]bar\"],data:{x:1b}}]");
        assert_eq!(reader.remaining(), " next");
    }

    #[test]
    fn selector_reports_missing_predicate_and_advancement_foundations() {
        let source = TestSource::new(true, true);
        let predicate = parse("@e[predicate=test]", &source, false, false);
        assert!(matches!(
            predicate,
            Err(error) if error.raw_message().contains("predicate needs")
        ));
        let advancements = parse("@e[advancements={}]", &source, false, false);
        assert!(matches!(
            advancements,
            Err(error) if error.raw_message().contains("advancements needs")
        ));
    }

    #[test]
    fn selector_entity_filters_use_command_identity_and_nbt_snapshot() {
        init_test_registry();
        let entity = SelectorTestEntity::new();
        entity.set_custom_name(Some(TextComponent::plain("Named item")));
        let mut custom_data = NbtCompound::new();
        custom_data.insert("flag", NbtTag::Byte(1));
        entity.set_custom_data(custom_data);

        let mut expected_data = NbtCompound::new();
        expected_data.insert("flag", NbtTag::Byte(1));
        let mut expected = NbtCompound::new();
        expected.insert("data", NbtTag::Compound(expected_data));

        assert!(entity_name_filter_matches("Named item", false, &entity));
        assert!(entity_nbt_filter_matches(&expected, false, &entity));
        assert!(!entity_nbt_filter_matches(&expected, true, &entity));
        assert!(!game_mode_filter_matches(GameType::Creative, true, &entity));
    }

    #[test]
    fn selector_score_and_team_filters_use_one_domain_scoreboard() {
        let scoreboard = Scoreboard::new();
        let Ok(kills) = scoreboard.add_objective("kills") else {
            panic!("objective should be added");
        };
        let Ok(red) = scoreboard.add_team("red") else {
            panic!("team should be added");
        };
        let holder = ScoreHolder::new("Steve");
        assert!(scoreboard.set_score(&holder, &kills, 5).is_ok());
        assert!(scoreboard.add_holder_to_team(&holder, &red).is_ok());

        assert!(score_filter_matches(
            &[("kills".to_owned(), IntRange::exactly(5))],
            holder.name(),
            &scoreboard
        ));
        assert!(team_filter_matches(
            "red",
            false,
            holder.name(),
            &scoreboard
        ));
        assert!(!team_filter_matches("", false, holder.name(), &scoreboard));
    }

    #[test]
    fn selector_suggestions_include_supported_options_and_source_values() {
        init_test_registry();
        let source = TestSource::new(true, true);

        let Ok(mut root) = SuggestionsBuilder::new("s", 0) else {
            panic!("suggestion builder should be valid");
        };
        suggest_entity_selector(&mut root, &source, false, false);
        let Ok(root) = root.build() else {
            panic!("root suggestions should build");
        };
        assert!(
            root.list()
                .iter()
                .any(|suggestion| suggestion.text() == "Steve")
        );

        let Ok(mut single_player) = SuggestionsBuilder::new("@", 0) else {
            panic!("suggestion builder should be valid");
        };
        suggest_entity_selector(&mut single_player, &source, true, true);
        let Ok(single_player) = single_player.build() else {
            panic!("single-player selector suggestions should build");
        };
        let roots = single_player
            .list()
            .iter()
            .map(Suggestion::text)
            .collect::<Vec<_>>();
        assert_eq!(roots.len(), 6);
        for selector in ["@a", "@e", "@n", "@p", "@r", "@s"] {
            assert!(roots.contains(&selector));
        }

        let Ok(mut options) = SuggestionsBuilder::new("@e[", 0) else {
            panic!("suggestion builder should be valid");
        };
        suggest_entity_selector(&mut options, &source, false, false);
        let Ok(options) = options.build() else {
            panic!("option suggestions should build");
        };
        assert!(
            options
                .list()
                .iter()
                .any(|suggestion| suggestion.text() == "@e[nbt=")
        );
        assert!(
            !options
                .list()
                .iter()
                .any(|suggestion| suggestion.text() == "@e[predicate=")
        );

        let Ok(mut team) = SuggestionsBuilder::new("@e[team=R", 0) else {
            panic!("suggestion builder should be valid");
        };
        suggest_entity_selector(&mut team, &source, false, false);
        let Ok(team) = team.build() else {
            panic!("team suggestions should build");
        };
        assert!(
            team.list()
                .iter()
                .any(|suggestion| suggestion.text() == "@e[team=red")
        );
    }
}
