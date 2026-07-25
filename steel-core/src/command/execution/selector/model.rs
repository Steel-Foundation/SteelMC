use super::*;

impl EntitySelector {
    pub(super) fn new(
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

    pub(super) fn for_selector_type(selector_type: SelectorType) -> Self {
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

    pub(super) fn validate_for_argument(
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

pub(super) fn entity_nbt_filter_matches(
    expected: &NbtCompound,
    inverted: bool,
    entity: &dyn Entity,
) -> bool {
    let actual = entity.nbt_for_data_compare();
    compare_nbt_compounds(expected, &actual, true) != inverted
}

pub(super) fn entity_name_filter_matches(value: &str, inverted: bool, entity: &dyn Entity) -> bool {
    (entity.plain_text_name() == value) != inverted
}

pub(super) fn game_mode_filter_matches(
    value: GameType,
    inverted: bool,
    entity: &dyn Entity,
) -> bool {
    let Some(player) = entity.as_player() else {
        return false;
    };
    (player.game_mode() == value) != inverted
}

pub(super) fn team_filter_matches(
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

pub(super) fn score_filter_matches(
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

pub(super) fn selector_position(selector: &EntitySelector, source: &CommandSource) -> DVec3 {
    let base = if selector.requires_position() {
        source.position()
    } else {
        DVec3::ZERO
    };
    selector.position.apply(base)
}

pub(super) fn find_entity_by_uuid(source: &CommandSource, uuid: &Uuid) -> Option<SharedEntity> {
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

pub(super) fn create_delta_aabb(x: f64, y: f64, z: f64) -> WorldAabb {
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
