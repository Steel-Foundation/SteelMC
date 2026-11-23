use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use steel_utils::Identifier;

use super::{
    PermissionCollectionError, PermissionCollections, PermissionEntry, PermissionKey,
    PermissionKeyError, PermissionMetadataEntry, PermissionMetadataExpression,
    PermissionMetadataExpressionError, PermissionMetadataSet, PermissionMetadataValue,
    PermissionRuleContext, PermissionRuleExpression, PermissionRuleExpressionError,
    PermissionSegment, PermissionSet, PermissionState, generated_collections,
};

/// Parsed `groups.toml` permission configuration.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PermissionGroupsConfig {
    /// Groups every player receives.
    pub default_groups: Vec<String>,
    /// Group assigned by `/op`.
    pub op_group: String,
    /// Groups granted to command blocks.
    pub command_block_groups: Vec<String>,
    /// Named groups available for assignment.
    pub groups: BTreeMap<String, PermissionGroupConfig>,
}

impl Default for PermissionGroupsConfig {
    fn default() -> Self {
        Self {
            default_groups: vec![generated_collections::DEFAULT_GROUP.to_owned()],
            op_group: generated_collections::OP_GROUP.to_owned(),
            command_block_groups: generated_collections::COMMAND_BLOCK_GROUPS
                .iter()
                .map(|group| (*group).to_owned())
                .collect(),
            groups: generated_collections::default_groups(),
        }
    }
}

/// One configured permission group.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PermissionGroupConfig {
    /// Priority used between equally specific group rules. Higher wins.
    pub priority: i32,
    /// Parent groups inherited by this group.
    pub inherits: Vec<String>,
    /// Permission rule expressions or collection references explicitly allowed by this group.
    pub allow: Vec<String>,
    /// Permission rule expressions or collection references explicitly denied by this group.
    pub deny: Vec<String>,
    /// Ordered permission metadata rules.
    pub metadata: Vec<PermissionMetadataRuleConfig>,
}

/// One `groups.toml` permission metadata rule.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PermissionMetadataRuleConfig {
    /// Metadata expression affected by this rule.
    pub key: String,
    /// Configured typed value.
    pub value: PermissionMetadataValue,
}

/// One `allow`/`deny` rule inside a permission group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionGroupRule {
    /// A reference to a code-owned permission collection.
    Collection(Identifier),
    /// A permission rule expression.
    Expression(PermissionRuleExpression),
}

impl PermissionGroupRule {
    /// Parses a collection reference (`#namespace:name`) or a permission rule expression.
    ///
    /// # Errors
    ///
    /// Returns an error when the collection reference or rule expression is invalid.
    pub fn parse(value: impl Into<String>) -> Result<Self, PermissionGroupRuleError> {
        let value = value.into();
        if let Some(reference) = value.strip_prefix('#') {
            return Identifier::from_str(reference)
                .map(Self::Collection)
                .map_err(
                    |reason| PermissionGroupRuleError::InvalidCollectionReference { value, reason },
                );
        }
        PermissionRuleExpression::parse(value)
            .map(Self::Expression)
            .map_err(PermissionGroupRuleError::from)
    }

    /// Builds a global rule for one concrete permission identifier.
    #[must_use]
    pub fn permission(permission: Identifier) -> Self {
        Self::Expression(PermissionRuleExpression::new(
            PermissionKey::from_identifier(&permission),
            PermissionRuleContext::Global,
        ))
    }

    /// Builds a global wildcard rule; used for generated built-in authorities.
    pub(crate) fn wildcard(namespace: &str, prefix: &str) -> Self {
        Self::Expression(PermissionRuleExpression::new(
            PermissionKey::wildcard(namespace, prefix),
            PermissionRuleContext::Global,
        ))
    }

    /// Returns the permission rule expression, or `None` for a collection reference.
    #[must_use]
    pub const fn expression(&self) -> Option<&PermissionRuleExpression> {
        match self {
            Self::Expression(expression) => Some(expression),
            Self::Collection(_) => None,
        }
    }

    /// Returns the referenced collection, or `None` for a permission rule expression.
    #[must_use]
    pub const fn collection(&self) -> Option<&Identifier> {
        match self {
            Self::Collection(identifier) => Some(identifier),
            Self::Expression(_) => None,
        }
    }
}

impl fmt::Display for PermissionGroupRule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Collection(identifier) => write!(formatter, "#{identifier}"),
            Self::Expression(expression) => expression.fmt(formatter),
        }
    }
}

/// Invalid permission group rule.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PermissionGroupRuleError {
    /// The permission rule expression is invalid.
    #[error(transparent)]
    InvalidExpression(#[from] PermissionRuleExpressionError),
    /// The collection reference is invalid.
    #[error("invalid permission collection reference '{value}': {reason}")]
    InvalidCollectionReference {
        /// Invalid rule text.
        value: String,
        /// Identifier parse failure reason.
        reason: &'static str,
    },
}

/// Validated, parsed permission groups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionGroups {
    default_groups: Vec<String>,
    groups: BTreeMap<String, PermissionGroup>,
}

impl PermissionGroups {
    /// Validates and parses permission group configuration, expanding collection references.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid names, rules, default groups, or inheritance.
    pub fn from_config(
        config: PermissionGroupsConfig,
        collections: &PermissionCollections,
    ) -> Result<Self, PermissionConfigError> {
        for group in &config.default_groups {
            validate_group_name(group)?;
            if !config.groups.contains_key(group) {
                return Err(PermissionConfigError::MissingDefaultGroup(group.to_owned()));
            }
        }
        validate_group_name(&config.op_group)?;
        if !config.groups.contains_key(&config.op_group) {
            return Err(PermissionConfigError::MissingRequiredGroup(config.op_group));
        }
        for group in &config.command_block_groups {
            validate_group_name(group)?;
            if !config.groups.contains_key(group) {
                return Err(PermissionConfigError::MissingRequiredGroup(group.clone()));
            }
        }
        validate_group_inheritance(&config)?;

        let mut groups = BTreeMap::new();
        for (name, group) in config.groups {
            validate_group_name(&name)?;
            let permissions = parse_group_permissions(&name, group.allow, group.deny, collections)?;
            let metadata = parse_group_metadata(&name, group.metadata)?;
            groups.insert(
                name,
                PermissionGroup {
                    priority: group.priority,
                    inherits: group.inherits,
                    permissions,
                    metadata,
                },
            );
        }
        Ok(Self {
            default_groups: config.default_groups,
            groups,
        })
    }

    /// Returns the configured default group names.
    #[must_use]
    pub fn default_groups(&self) -> &[String] {
        &self.default_groups
    }

    /// Returns configured groups keyed by name.
    #[must_use]
    pub const fn groups(&self) -> &BTreeMap<String, PermissionGroup> {
        &self.groups
    }

    /// Returns whether a group exists.
    #[must_use]
    pub fn contains_group(&self, group: &str) -> bool {
        self.groups.contains_key(group)
    }

    /// Builds effective permissions from defaults, assigned groups, and subject overrides.
    ///
    /// Unknown assigned groups have no effect. Each inherited group contributes at most once.
    #[must_use]
    pub fn effective_permissions(
        &self,
        assigned_groups: &[String],
        subject_permissions: &PermissionSet,
    ) -> PermissionSet {
        let mut effective = PermissionSet::new();
        let mut appended_groups = BTreeSet::new();

        for group in &self.default_groups {
            self.append_group(group, &mut effective, &mut appended_groups);
        }
        for group in assigned_groups {
            self.append_group(group, &mut effective, &mut appended_groups);
        }
        for entry in subject_permissions.entries() {
            effective.push(entry.clone());
        }
        effective
    }

    /// Builds effective metadata from defaults, assigned groups, and subject overrides.
    ///
    /// Unknown assigned groups have no effect. Each inherited group contributes at most once.
    #[must_use]
    pub fn effective_metadata(
        &self,
        assigned_groups: &[String],
        subject_metadata: &PermissionMetadataSet,
    ) -> PermissionMetadataSet {
        let mut effective = PermissionMetadataSet::new();
        let mut appended_groups = BTreeSet::new();

        for group in &self.default_groups {
            self.append_group_metadata(group, &mut effective, &mut appended_groups);
        }
        for group in assigned_groups {
            self.append_group_metadata(group, &mut effective, &mut appended_groups);
        }
        for entry in subject_metadata.entries() {
            effective.push(entry.clone());
        }
        effective
    }

    fn append_group(
        &self,
        group_name: &str,
        effective: &mut PermissionSet,
        appended_groups: &mut BTreeSet<String>,
    ) {
        if !appended_groups.insert(group_name.to_owned()) {
            return;
        }
        let Some(group) = self.groups.get(group_name) else {
            return;
        };
        for parent in &group.inherits {
            self.append_group(parent, effective, appended_groups);
        }
        for entry in group.permissions.entries() {
            effective.push_group(entry.clone(), group_name, group.priority);
        }
    }

    fn append_group_metadata(
        &self,
        group_name: &str,
        effective: &mut PermissionMetadataSet,
        appended_groups: &mut BTreeSet<String>,
    ) {
        if !appended_groups.insert(group_name.to_owned()) {
            return;
        }
        let Some(group) = self.groups.get(group_name) else {
            return;
        };
        for parent in &group.inherits {
            self.append_group_metadata(parent, effective, appended_groups);
        }
        for entry in group.metadata.entries() {
            effective.push_group(entry.clone(), group_name, group.priority);
        }
    }
}

fn parse_group_permissions(
    group: &str,
    allow: Vec<String>,
    deny: Vec<String>,
    collections: &PermissionCollections,
) -> Result<PermissionSet, PermissionConfigError> {
    let mut permissions = PermissionSet::new();
    for rule in allow {
        push_group_rule(
            &mut permissions,
            group,
            rule,
            PermissionState::Allow,
            collections,
        )?;
    }
    for rule in deny {
        push_group_rule(
            &mut permissions,
            group,
            rule,
            PermissionState::Deny,
            collections,
        )?;
    }
    Ok(permissions)
}

fn push_group_rule(
    permissions: &mut PermissionSet,
    group: &str,
    rule: String,
    state: PermissionState,
    collections: &PermissionCollections,
) -> Result<(), PermissionConfigError> {
    let rule = PermissionGroupRule::parse(rule).map_err(|source| {
        PermissionConfigError::InvalidPermissionExpression {
            group: group.to_owned(),
            source,
        }
    })?;
    match rule {
        PermissionGroupRule::Expression(expression) => {
            let (key, context) = expression.into_parts();
            permissions.push(PermissionEntry::new_with_context(key, context, state));
        }
        PermissionGroupRule::Collection(identifier) => {
            for expression in collections.expand(&identifier) {
                let (key, context) = expression.into_parts();
                permissions.push(PermissionEntry::new_with_context(key, context, state));
            }
        }
    }
    Ok(())
}

fn parse_group_metadata(
    group: &str,
    rules: Vec<PermissionMetadataRuleConfig>,
) -> Result<PermissionMetadataSet, PermissionConfigError> {
    let mut metadata = PermissionMetadataSet::new();
    for rule in rules {
        let expression = PermissionMetadataExpression::parse(rule.key).map_err(|source| {
            PermissionConfigError::InvalidMetadataExpression {
                group: group.to_owned(),
                source,
            }
        })?;
        let (key, context) = expression.into_parts();
        metadata.push(PermissionMetadataEntry::new_with_context(
            key, context, rule.value,
        ));
    }
    Ok(metadata)
}

fn validate_group_name(group: &str) -> Result<(), PermissionConfigError> {
    PermissionSegment::parse(group).map_or_else(
        |source| {
            Err(PermissionConfigError::InvalidGroupName {
                group: group.to_owned(),
                source,
            })
        },
        |_| Ok(()),
    )
}

fn validate_group_inheritance(
    config: &PermissionGroupsConfig,
) -> Result<(), PermissionConfigError> {
    for (group, group_config) in &config.groups {
        for parent in &group_config.inherits {
            validate_group_name(parent)?;
            if !config.groups.contains_key(parent) {
                return Err(PermissionConfigError::MissingInheritedGroup {
                    group: group.to_owned(),
                    inherited: parent.to_owned(),
                });
            }
        }
    }

    let mut visited = BTreeSet::new();
    let mut stack = Vec::new();
    for (group, group_config) in &config.groups {
        validate_group_inheritance_node(group, group_config, config, &mut visited, &mut stack)?;
    }
    Ok(())
}

fn validate_group_inheritance_node<'a>(
    group: &'a str,
    group_config: &'a PermissionGroupConfig,
    config: &'a PermissionGroupsConfig,
    visited: &mut BTreeSet<&'a str>,
    stack: &mut Vec<&'a str>,
) -> Result<(), PermissionConfigError> {
    if visited.contains(group) {
        return Ok(());
    }
    if let Some(cycle_start) = stack.iter().position(|ancestor| *ancestor == group) {
        let mut cycle = stack[cycle_start..]
            .iter()
            .map(|group| (*group).to_owned())
            .collect::<Vec<_>>();
        cycle.push(group.to_owned());
        return Err(PermissionConfigError::InheritedGroupCycle(cycle));
    }

    stack.push(group);
    for parent in &group_config.inherits {
        let Some(parent_config) = config.groups.get(parent) else {
            return Err(PermissionConfigError::MissingInheritedGroup {
                group: group.to_owned(),
                inherited: parent.to_owned(),
            });
        };
        validate_group_inheritance_node(parent, parent_config, config, visited, stack)?;
    }
    stack.pop();
    visited.insert(group);
    Ok(())
}

/// One validated permission group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionGroup {
    priority: i32,
    inherits: Vec<String>,
    permissions: PermissionSet,
    metadata: PermissionMetadataSet,
}

impl PermissionGroup {
    /// Returns the group's conflict priority.
    #[must_use]
    pub const fn priority(&self) -> i32 {
        self.priority
    }

    /// Returns the parent groups inherited by this group.
    #[must_use]
    pub fn inherits(&self) -> &[String] {
        &self.inherits
    }

    /// Returns this group's own permission entries.
    #[must_use]
    pub const fn permissions(&self) -> &PermissionSet {
        &self.permissions
    }

    /// Returns this group's own metadata entries.
    #[must_use]
    pub const fn metadata(&self) -> &PermissionMetadataSet {
        &self.metadata
    }
}

/// Invalid permission group configuration.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PermissionConfigError {
    /// A default group does not exist in the group map.
    #[error("default permission group '{0}' is not configured")]
    MissingDefaultGroup(String),
    /// A built-in required group does not exist in the group map.
    #[error("required permission group '{0}' is not configured")]
    MissingRequiredGroup(String),
    /// A group inherits a group that does not exist.
    #[error("permission group '{group}' inherits unknown group '{inherited}'")]
    MissingInheritedGroup {
        /// Group containing the invalid edge.
        group: String,
        /// Missing parent group.
        inherited: String,
    },
    /// Group inheritance contains a cycle, including the repeated final name.
    #[error("permission group inheritance cycle: {}", .0.join(" -> "))]
    InheritedGroupCycle(Vec<String>),
    /// A group contains an invalid permission rule expression.
    #[error("permission group '{group}' contains invalid permission expression: {source}")]
    InvalidPermissionExpression {
        /// Group containing the invalid expression.
        group: String,
        /// Parse error.
        source: PermissionGroupRuleError,
    },
    /// A group contains an invalid metadata expression.
    #[error("permission group '{group}' contains invalid metadata expression: {source}")]
    InvalidMetadataExpression {
        /// Group containing the invalid expression.
        group: String,
        /// Parse error.
        source: PermissionMetadataExpressionError,
    },
    /// A group name is not a valid command-usable segment.
    #[error("permission group name '{group}' is invalid: {source}")]
    InvalidGroupName {
        /// Invalid group name.
        group: String,
        /// Parse error.
        source: PermissionKeyError,
    },
    /// A permission collection configuration or reference is invalid.
    #[error(transparent)]
    Collection(#[from] PermissionCollectionError),
}
