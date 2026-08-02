use super::*;

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
