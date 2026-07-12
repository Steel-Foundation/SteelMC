//! Permission command arguments and discovery-only suggestions.

use std::collections::{BTreeMap, BTreeSet};

use steel_protocol::packets::game::{
    ArgumentStringTypeBehavior, ArgumentType as ProtocolArgumentType,
    SuggestionType as ProtocolSuggestionType,
};
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::{
    command::brigadier::{
        CommandSyntaxError, CommandSyntaxErrorKind, StringReader, SuggestionsBuilder,
    },
    permission::{
        PermissionMetadataExpression, PermissionRuleContext, PermissionRuleExpression,
        PermissionSegment,
    },
};

use super::{
    CommandArgumentSource,
    argument::{SteelArgumentParser, SteelArgumentSuggestionContext},
};

// SAFETY: This Steel-owned key uniquely identifies the concrete parsed value.
unsafe impl DowncastType for PermissionRuleExpression {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:command/value/permission_rule_expression");
}

// SAFETY: This Steel-owned key uniquely identifies the concrete parsed value.
unsafe impl DowncastType for PermissionMetadataExpression {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:command/value/permission_metadata_expression");
}

/// Validated permission group name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PermissionGroupName(Box<str>);

impl PermissionGroupName {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete parsed value.
unsafe impl DowncastType for PermissionGroupName {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:command/value/permission_group");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PermissionSuggestionScope {
    All,
    UserOwned,
    GroupOwned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PermissionRuleParser {
    suggestions: PermissionSuggestionScope,
}

impl PermissionRuleParser {
    pub(super) const fn all() -> Self {
        Self {
            suggestions: PermissionSuggestionScope::All,
        }
    }

    pub(super) const fn user_owned() -> Self {
        Self {
            suggestions: PermissionSuggestionScope::UserOwned,
        }
    }

    pub(super) const fn group_owned() -> Self {
        Self {
            suggestions: PermissionSuggestionScope::GroupOwned,
        }
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete parser.
unsafe impl DowncastType for PermissionRuleParser {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:command/parser/permission_rule");
}

impl SteelArgumentParser for PermissionRuleParser {
    type Value = PermissionRuleExpression;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        _source: &dyn CommandArgumentSource,
    ) -> Result<Self::Value, CommandSyntaxError> {
        let value = reader.read_unquoted_token();
        PermissionRuleExpression::parse(value).map_err(|error| {
            reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(
                error.to_string().into(),
            )))
        })
    }

    fn list_suggestions(
        &self,
        context: &dyn SteelArgumentSuggestionContext,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        let expressions = match self.suggestions {
            PermissionSuggestionScope::All => context.source().permission_rule_suggestions(),
            PermissionSuggestionScope::UserOwned => context
                .argument("targets")
                .and_then(|value| value.downcast_ref::<super::GameProfileArgument>())
                .map_or_else(Vec::new, |targets| {
                    context.source().user_permission_rule_suggestions(targets)
                }),
            PermissionSuggestionScope::GroupOwned => context
                .argument("group")
                .and_then(|value| value.downcast_ref::<PermissionGroupName>())
                .map_or_else(Vec::new, |group| {
                    context
                        .source()
                        .group_permission_rule_suggestions(group.as_str())
                }),
        };
        suggest_expression(builder, context.source(), expressions);
    }

    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>) {
        permission_expression_argument()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PermissionMetadataParser {
    suggestions: PermissionSuggestionScope,
}

impl PermissionMetadataParser {
    pub(super) const fn all() -> Self {
        Self {
            suggestions: PermissionSuggestionScope::All,
        }
    }

    pub(super) const fn user_owned() -> Self {
        Self {
            suggestions: PermissionSuggestionScope::UserOwned,
        }
    }

    pub(super) const fn group_owned() -> Self {
        Self {
            suggestions: PermissionSuggestionScope::GroupOwned,
        }
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete parser.
unsafe impl DowncastType for PermissionMetadataParser {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:command/parser/permission_metadata");
}

impl SteelArgumentParser for PermissionMetadataParser {
    type Value = PermissionMetadataExpression;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        _source: &dyn CommandArgumentSource,
    ) -> Result<Self::Value, CommandSyntaxError> {
        let value = reader.read_unquoted_token();
        PermissionMetadataExpression::parse(value).map_err(|error| {
            reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(
                error.to_string().into(),
            )))
        })
    }

    fn list_suggestions(
        &self,
        context: &dyn SteelArgumentSuggestionContext,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        let expressions = match self.suggestions {
            PermissionSuggestionScope::All => context.source().permission_metadata_suggestions(),
            PermissionSuggestionScope::UserOwned => context
                .argument("targets")
                .and_then(|value| value.downcast_ref::<super::GameProfileArgument>())
                .map_or_else(Vec::new, |targets| {
                    context
                        .source()
                        .user_permission_metadata_suggestions(targets)
                }),
            PermissionSuggestionScope::GroupOwned => context
                .argument("group")
                .and_then(|value| value.downcast_ref::<PermissionGroupName>())
                .map_or_else(Vec::new, |group| {
                    context
                        .source()
                        .group_permission_metadata_suggestions(group.as_str())
                }),
        };
        suggest_expression(builder, context.source(), expressions);
    }

    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>) {
        permission_expression_argument()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PermissionGroupParser {
    pub(super) require_existing: bool,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete parser.
unsafe impl DowncastType for PermissionGroupParser {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:command/parser/permission_group");
}

impl SteelArgumentParser for PermissionGroupParser {
    type Value = PermissionGroupName;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        source: &dyn CommandArgumentSource,
    ) -> Result<Self::Value, CommandSyntaxError> {
        let value = reader.read_unquoted_string();
        PermissionSegment::parse(value).map_err(|error| {
            reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(
                error.to_string().into(),
            )))
        })?;
        if self.require_existing
            && !source
                .permission_group_names()
                .iter()
                .any(|group| group == value)
        {
            return Err(reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(
                format!("Unknown permission group '{value}'").into(),
            ))));
        }
        Ok(PermissionGroupName(value.into()))
    }

    fn list_suggestions(
        &self,
        context: &dyn SteelArgumentSuggestionContext,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        let prefix = builder.remaining_lowercase().to_owned();
        for group in context.source().permission_group_names() {
            if group.to_lowercase().starts_with(&prefix) {
                builder.suggest(group);
            }
        }
    }

    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>) {
        permission_group_argument()
    }
}

const fn permission_expression_argument() -> (ProtocolArgumentType, Option<ProtocolSuggestionType>)
{
    // Permission expressions contain characters that Brigadier's word parser
    // treats as delimiters. These arguments are terminal, so a greedy string
    // lets the client cover Steel's full no-whitespace expression syntax.
    (
        ProtocolArgumentType::String {
            behavior: ArgumentStringTypeBehavior::GreedyPhrase,
        },
        Some(ProtocolSuggestionType::AskServer),
    )
}

const fn permission_group_argument() -> (ProtocolArgumentType, Option<ProtocolSuggestionType>) {
    (
        ProtocolArgumentType::String {
            behavior: ArgumentStringTypeBehavior::SingleWord,
        },
        Some(ProtocolSuggestionType::AskServer),
    )
}

fn suggest_expression(
    builder: &mut SuggestionsBuilder<'_>,
    source: &dyn CommandArgumentSource,
    expressions: Vec<String>,
) {
    let prefix = builder.remaining();
    for expression in &expressions {
        if expression.starts_with(prefix) {
            builder.suggest(expression.clone());
        }
    }

    let Some((base, context_prefix)) = prefix.split_once('{') else {
        return;
    };
    if base.is_empty() || context_prefix.ends_with('}') {
        return;
    }

    let known_contexts = known_custom_contexts(
        expressions
            .iter()
            .chain(source.permission_rule_suggestions().iter())
            .chain(source.permission_metadata_suggestions().iter()),
    );
    suggest_context(builder, source, base, context_prefix, &known_contexts);
}

fn suggest_context(
    builder: &mut SuggestionsBuilder<'_>,
    source: &dyn CommandArgumentSource,
    base: &str,
    context_prefix: &str,
    known_contexts: &BTreeMap<String, BTreeSet<String>>,
) {
    let (completed, current) = context_prefix
        .rsplit_once(',')
        .map_or(("", context_prefix), |(completed, current)| {
            (completed, current)
        });
    let completed_keys = completed
        .split(',')
        .filter_map(|entry| entry.split_once('=').map(|(key, _)| key))
        .collect::<BTreeSet<_>>();
    let entry_prefix = if completed.is_empty() {
        format!("{base}{{")
    } else {
        format!("{base}{{{completed},")
    };

    let Some((key, value_prefix)) = current.split_once('=') else {
        for key in ["domain", "world"]
            .into_iter()
            .chain(known_contexts.keys().map(String::as_str))
        {
            if !completed_keys.contains(key) && key.starts_with(current) {
                builder.suggest(format!("{entry_prefix}{key}="));
            }
        }
        return;
    };

    let values = match key {
        "domain" => source
            .domain_names()
            .into_iter()
            .map(str::to_owned)
            .collect(),
        "world" => source.permission_context_world_names(),
        custom => known_contexts
            .get(custom)
            .map_or_else(Vec::new, |values| values.iter().cloned().collect()),
    };
    for value in values {
        if value.starts_with(value_prefix) {
            builder.suggest(format!("{entry_prefix}{key}={value}}}"));
        }
    }
}

fn known_custom_contexts<'a>(
    expressions: impl Iterator<Item = &'a String>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut contexts = BTreeMap::new();
    for expression in expressions {
        if let Ok(expression) = PermissionRuleExpression::parse(expression) {
            collect_custom_contexts(expression.context(), &mut contexts);
        } else if let Ok(expression) = PermissionMetadataExpression::parse(expression) {
            collect_custom_contexts(expression.context(), &mut contexts);
        }
    }
    contexts
}

fn collect_custom_contexts(
    context: &PermissionRuleContext,
    contexts: &mut BTreeMap<String, BTreeSet<String>>,
) {
    match context {
        PermissionRuleContext::Custom { key, value } => {
            contexts
                .entry(key.as_str().to_owned())
                .or_default()
                .insert(value.as_str().to_owned());
        }
        PermissionRuleContext::All(nested) => {
            for context in nested.iter() {
                collect_custom_contexts(context, contexts);
            }
        }
        PermissionRuleContext::Global
        | PermissionRuleContext::Domain(_)
        | PermissionRuleContext::World(_) => {}
    }
}
