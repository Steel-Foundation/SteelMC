use super::*;

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

pub(crate) fn parse_entity_selector_text(raw: &str) -> Result<EntitySelector, CommandSyntaxError> {
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

pub(crate) fn allow_selectors<S>(source: &S) -> bool
where
    S: CommandArgumentSource + ?Sized,
{
    source.allows_entity_selectors()
}

pub(super) fn allow_advanced_selectors<S>(source: &S) -> bool
where
    S: CommandArgumentSource + ?Sized,
{
    source.allows_advanced_entity_selectors()
}

pub(super) fn read_selector_argument(
    reader: &mut StringReader<'_>,
) -> Result<String, CommandSyntaxError> {
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

fn is_valid_selector_name(name: &str) -> bool {
    !name.is_empty() && name.encode_utf16().count() <= 16
}

#[cfg(test)]
pub(super) fn parse_selector_plan(
    raw: &str,
    allow_selectors: bool,
) -> Result<EntitySelector, SelectorParseError> {
    parse_selector_plan_with_permissions(raw, allow_selectors, allow_selectors)
}

pub(super) fn parse_selector_plan_with_permissions(
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

pub(super) fn parse_resource_identifier_value(value: &str) -> Option<Identifier> {
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

pub(super) fn wrap_degrees(value: f32) -> f32 {
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

/// Why `try_parse_message_selector` did not produce a valid selector.
pub(crate) enum MessageSelectorError {
    /// The `@` was not a selector (missing/unknown type). Skip it as literal text.
    Skip,
    /// A real selector syntax error. Propagate it.
    Propagate(CommandSyntaxError),
}

/// Tries to parse a selector at the reader's current position for message arguments.
///
/// Vanilla's message argument scans for `@` and attempts to parse a selector at each one.
/// Missing or unknown selector types are treated as literal `@` characters; all other
/// selector syntax errors are fatal to the command.
pub(crate) fn try_parse_message_selector<S>(
    reader: &mut StringReader<'_>,
    source: &S,
) -> Result<EntitySelector, MessageSelectorError>
where
    S: CommandArgumentSource + ?Sized,
{
    let start = reader.checkpoint();
    let raw = read_selector_argument(reader).map_err(MessageSelectorError::Propagate)?;
    let allow_selectors = allow_selectors(source);
    let allow_advanced = allow_advanced_selectors(source);

    match parse_selector_plan_with_permissions(&raw, allow_selectors, allow_advanced) {
        Err(error) if is_skippable_selector_error(&error) => {
            reader.restore(start);
            Err(MessageSelectorError::Skip)
        }
        Err(error) => {
            Err(MessageSelectorError::Propagate(selector_syntax_error(
                reader, start, &raw, error,
            )))
        }
        Ok(selector) => match selector.validate_for_argument(false, false) {
            Ok(()) => Ok(selector),
            Err(error) => Err(MessageSelectorError::Propagate(selector_syntax_error(
                reader, start, &raw, error,
            ))),
        },
    }
}

/// Returns `true` for selector errors that should be treated as literal `@` in messages.
fn is_skippable_selector_error(error: &SelectorParseError) -> bool {
    matches!(error.kind, SelectorParseErrorKind::Invalid(_))
        && error.cursor == 1
}
