use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use serde::{Deserialize, Serialize};

use super::{
    PermissionEntry, PermissionKeyError, PermissionMetadataEntry, PermissionMetadataExpression,
    PermissionMetadataExpressionError, PermissionMetadataSet, PermissionMetadataValue,
    PermissionRuleExpression, PermissionRuleExpressionError, PermissionSegment, PermissionSet,
};

/// Built-in operator group assigned by `/op`.
pub(crate) const OP_GROUP: &str = "op";

/// Parsed `groups.toml` permission configuration.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PermissionGroupsConfig {
    /// Groups every player receives.
    pub default_groups: Vec<String>,
    /// Named groups available for assignment.
    pub groups: BTreeMap<String, PermissionGroupConfig>,
}

impl Default for PermissionGroupsConfig {
    fn default() -> Self {
        let mut groups = BTreeMap::new();
        groups.insert("default".to_owned(), PermissionGroupConfig::default());
        groups.insert(
            OP_GROUP.to_owned(),
            PermissionGroupConfig {
                allow: vec!["*".to_owned()],
                ..PermissionGroupConfig::default()
            },
        );
        Self {
            default_groups: vec!["default".to_owned()],
            groups,
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
    /// Permission rule expressions explicitly allowed by this group.
    pub allow: Vec<String>,
    /// Permission rule expressions explicitly denied by this group.
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

/// Validated, parsed permission groups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionGroups {
    default_groups: Vec<String>,
    groups: BTreeMap<String, PermissionGroup>,
}

impl PermissionGroups {
    /// Validates and parses permission group configuration.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid names, rules, default groups, or inheritance.
    pub fn from_config(config: PermissionGroupsConfig) -> Result<Self, PermissionConfigError> {
        for group in &config.default_groups {
            validate_group_name(group)?;
            if !config.groups.contains_key(group) {
                return Err(PermissionConfigError::MissingDefaultGroup(group.to_owned()));
            }
        }
        if !config.groups.contains_key(OP_GROUP) {
            return Err(PermissionConfigError::MissingRequiredGroup(
                OP_GROUP.to_owned(),
            ));
        }
        validate_group_inheritance(&config)?;

        let mut groups = BTreeMap::new();
        for (name, group) in config.groups {
            validate_group_name(&name)?;
            let permissions = parse_group_permissions(&name, group.allow, group.deny)?;
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
) -> Result<PermissionSet, PermissionConfigError> {
    let mut permissions = PermissionSet::new();
    for rule in allow {
        let expression = parse_group_rule(group, rule)?;
        let (key, context) = expression.into_parts();
        permissions.push(PermissionEntry::allow_with_context(key, context));
    }
    for rule in deny {
        let expression = parse_group_rule(group, rule)?;
        let (key, context) = expression.into_parts();
        permissions.push(PermissionEntry::deny_with_context(key, context));
    }
    Ok(permissions)
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

fn parse_group_rule(
    group: &str,
    rule: String,
) -> Result<PermissionRuleExpression, PermissionConfigError> {
    PermissionRuleExpression::parse(rule).map_err(|source| {
        PermissionConfigError::InvalidPermissionExpression {
            group: group.to_owned(),
            source,
        }
    })
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PermissionConfigError {
    /// A default group does not exist in the group map.
    MissingDefaultGroup(String),
    /// A built-in required group does not exist in the group map.
    MissingRequiredGroup(String),
    /// A group inherits a group that does not exist.
    MissingInheritedGroup {
        /// Group containing the invalid edge.
        group: String,
        /// Missing parent group.
        inherited: String,
    },
    /// Group inheritance contains a cycle, including the repeated final name.
    InheritedGroupCycle(Vec<String>),
    /// A group contains an invalid permission rule expression.
    InvalidPermissionExpression {
        /// Group containing the invalid expression.
        group: String,
        /// Parse error.
        source: PermissionRuleExpressionError,
    },
    /// A group contains an invalid metadata expression.
    InvalidMetadataExpression {
        /// Group containing the invalid expression.
        group: String,
        /// Parse error.
        source: PermissionMetadataExpressionError,
    },
    /// A group name is not a valid command-usable segment.
    InvalidGroupName {
        /// Invalid group name.
        group: String,
        /// Parse error.
        source: PermissionKeyError,
    },
}

impl fmt::Display for PermissionConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDefaultGroup(group) => {
                write!(
                    formatter,
                    "default permission group '{group}' is not configured"
                )
            }
            Self::MissingRequiredGroup(group) => {
                write!(
                    formatter,
                    "required permission group '{group}' is not configured"
                )
            }
            Self::MissingInheritedGroup { group, inherited } => write!(
                formatter,
                "permission group '{group}' inherits unknown group '{inherited}'"
            ),
            Self::InheritedGroupCycle(cycle) => write!(
                formatter,
                "permission group inheritance cycle: {}",
                cycle.join(" -> ")
            ),
            Self::InvalidPermissionExpression { group, source } => write!(
                formatter,
                "permission group '{group}' contains invalid permission expression: {source}"
            ),
            Self::InvalidMetadataExpression { group, source } => write!(
                formatter,
                "permission group '{group}' contains invalid metadata expression: {source}"
            ),
            Self::InvalidGroupName { group, source } => {
                write!(
                    formatter,
                    "permission group name '{group}' is invalid: {source}"
                )
            }
        }
    }
}

impl Error for PermissionConfigError {}
