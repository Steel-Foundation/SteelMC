//! Stateful completions for block states and block predicates.

use steel_registry::{
    REGISTRY, RegistryExt as _, TaggedRegistryExt as _,
    blocks::{BlockRef, block_state_ext::BlockStateExt as _},
};
use steel_utils::Identifier;

use super::super::argument::{matches_substring, parse_identifier};
use crate::command::brigadier::{StringReader, SuggestionsBuilder};

pub(in crate::command::execution) fn suggest_blocks(builder: &mut SuggestionsBuilder<'_>) {
    suggest_blocks_and_tags(builder, true);
}

pub(in crate::command::execution) fn suggest_block_inputs(builder: &mut SuggestionsBuilder<'_>) {
    suggest_blocks_and_tags(builder, false);
}

fn suggest_blocks_and_tags(builder: &mut SuggestionsBuilder<'_>, include_tags: bool) {
    let input = builder.remaining().to_owned();
    let Some(syntax_start) = input.find(['[', '{']) else {
        if let Some(target) = SuggestedBlockTarget::parse(&input, include_tags) {
            if target.has_properties() {
                builder.suggest(format!("{input}["));
            }
            if target.has_block_entity() {
                builder.suggest(format!("{input}{{"));
            }
        } else {
            suggest_block_resources(&input, include_tags, builder);
        }
        return;
    };

    let resource = &input[..syntax_start];
    let Some(target) = SuggestedBlockTarget::parse(resource, include_tags) else {
        return;
    };
    match input.as_bytes()[syntax_start] {
        b'[' => suggest_block_properties(&input, syntax_start, &target, builder),
        b'{' => {}
        _ => unreachable!("syntax search only returns block-state delimiters"),
    }
}

fn suggest_block_resources(input: &str, include_tags: bool, builder: &mut SuggestionsBuilder<'_>) {
    let lowercase = input.to_lowercase();
    if let Some(prefix) = lowercase.strip_prefix('#') {
        if include_tags {
            for tag in REGISTRY
                .blocks
                .tag_keys()
                .filter(|tag| identifier_matches(prefix, tag))
            {
                builder.suggest(format!("#{tag}"));
            }
        }
        return;
    }
    for block in REGISTRY
        .blocks
        .iter()
        .map(|(_, block)| &block.key)
        .filter(|key| identifier_matches(&lowercase, key))
    {
        builder.suggest(block.to_string());
    }
    if include_tags {
        for tag in REGISTRY
            .blocks
            .tag_keys()
            .filter(|tag| identifier_matches(&lowercase, tag))
        {
            builder.suggest(format!("#{tag}"));
        }
    }
}

fn suggest_block_properties(
    input: &str,
    syntax_start: usize,
    target: &SuggestedBlockTarget,
    builder: &mut SuggestionsBuilder<'_>,
) {
    let body = &input[syntax_start + 1..];
    if let Some(end) = find_unquoted(body, ']') {
        if body[end + 1..].is_empty() && target.has_block_entity() {
            builder.suggest(format!("{input}{{"));
        }
        return;
    }

    let current_start = current_property_start(body);
    let current = &body[current_start..];
    let leading_whitespace = current.len() - current.trim_start().len();
    let prefix_end = syntax_start + 1 + current_start + leading_whitespace;
    let prefix = &input[..prefix_end];
    let current = &current[leading_whitespace..];
    let visited = completed_property_names(&body[..current_start]);

    let Some(equals) = find_unquoted(current, '=') else {
        let key = parsed_command_string(current).unwrap_or_else(|| current.trim().to_owned());
        if !visited.iter().any(|seen| seen == &key) && target.has_property(&key) {
            builder.suggest(format!("{input}="));
        } else if current.trim().len() == current.len() {
            let lowercase = key.to_lowercase();
            for property in target.property_names() {
                if !visited.iter().any(|seen| seen == property) && property.starts_with(&lowercase)
                {
                    builder.suggest(format!("{prefix}{property}="));
                }
            }
        }
        if current_start == 0 && current.trim().is_empty() {
            builder.suggest(format!("{prefix}]"));
        }
        return;
    };

    let Some(property) = parsed_command_string(&current[..equals]) else {
        return;
    };
    if visited.iter().any(|seen| seen == &property)
        || (!target.is_tag && !target.has_property(&property))
    {
        return;
    }
    let value_input = &current[equals + 1..];
    let value_whitespace = value_input.len() - value_input.trim_start().len();
    let value_prefix_end = prefix_end + equals + 1 + value_whitespace;
    let value_prefix = &input[..value_prefix_end];
    let value_input = &value_input[value_whitespace..];
    let value = parsed_command_string(value_input).unwrap_or_else(|| value_input.trim().to_owned());
    let value_is_exact = target.has_property_value(&property, &value);

    if target.is_tag || !value_is_exact {
        for possible in target.property_value_names(&property) {
            builder.suggest(format!("{value_prefix}{possible}"));
        }
    }
    if target.is_tag || value_is_exact {
        let has_more = target
            .property_names()
            .into_iter()
            .any(|name| name != property && !visited.iter().any(|seen| seen == name));
        if has_more {
            builder.suggest(format!("{input},"));
        }
        builder.suggest(format!("{input}]"));
    }
}

struct SuggestedBlockTarget {
    blocks: Vec<BlockRef>,
    is_tag: bool,
}

impl SuggestedBlockTarget {
    fn parse(input: &str, include_tags: bool) -> Option<Self> {
        if let Some(tag) = input.strip_prefix('#') {
            if !include_tags {
                return None;
            }
            let key = parse_suggestion_identifier(tag)?;
            return Some(Self {
                blocks: REGISTRY.blocks.get_tag(&key)?,
                is_tag: true,
            });
        }
        let key = parse_suggestion_identifier(input)?;
        Some(Self {
            blocks: vec![REGISTRY.blocks.by_key(&key)?],
            is_tag: false,
        })
    }

    fn property_names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        for block in &self.blocks {
            for property in block.properties {
                let name = property.get_name();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        names
    }

    fn property_value_names(&self, name: &str) -> Vec<&str> {
        let mut values = Vec::new();
        for block in &self.blocks {
            let Some(property) = block
                .properties
                .iter()
                .find(|property| property.get_name() == name)
            else {
                continue;
            };
            for value in property.get_possible_value_names() {
                if !values.contains(&value) {
                    values.push(value);
                }
            }
        }
        values
    }

    fn has_properties(&self) -> bool {
        self.blocks.iter().any(|block| !block.properties.is_empty())
    }

    fn has_property(&self, name: &str) -> bool {
        self.blocks.iter().any(|block| {
            block
                .properties
                .iter()
                .any(|property| property.get_name() == name)
        })
    }

    fn has_property_value(&self, name: &str, value: &str) -> bool {
        self.blocks.iter().any(|block| {
            block.properties.iter().any(|property| {
                property.get_name() == name && property.get_possible_value_names().contains(&value)
            })
        })
    }

    fn has_block_entity(&self) -> bool {
        self.blocks
            .iter()
            .any(|block| block.default_state().has_block_entity())
    }
}

fn parse_suggestion_identifier(input: &str) -> Option<Identifier> {
    let mut reader = StringReader::new(input);
    let identifier = parse_identifier(&mut reader).ok()?;
    (!reader.can_read()).then_some(identifier)
}

fn parsed_command_string(input: &str) -> Option<String> {
    let mut reader = StringReader::new(input);
    reader.skip_whitespace();
    let value = reader.read_string().ok()?;
    reader.skip_whitespace();
    (!reader.can_read()).then_some(value)
}

fn completed_property_names(input: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut start = 0;
    while let Some(separator) = find_unquoted(&input[start..], ',') {
        let end = start + separator;
        let property = &input[start..end];
        if let Some(equals) = find_unquoted(property, '=')
            && let Some(name) = parsed_command_string(&property[..equals])
        {
            names.push(name);
        }
        start = end + 1;
    }
    names
}

fn current_property_start(input: &str) -> usize {
    let mut start = 0;
    while let Some(separator) = find_unquoted(&input[start..], ',') {
        start += separator + 1;
    }
    start
}

fn find_unquoted(input: &str, needle: char) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in input.char_indices() {
        if let Some(terminator) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == terminator {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            _ if character == needle => return Some(index),
            _ => {}
        }
    }
    None
}

fn identifier_matches(pattern: &str, identifier: &Identifier) -> bool {
    if pattern.contains(':') {
        matches_substring(pattern, &identifier.to_string())
    } else {
        matches_substring(pattern, identifier.namespace.as_ref())
            || matches_substring(pattern, identifier.path.as_ref())
    }
}
