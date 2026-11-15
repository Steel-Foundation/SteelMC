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
    generated_groups,
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

impl PermissionGroupsConfig {
    /// Inserts the generated vanilla groups as empty configurable groups.
    ///
    /// Existing groups keep their configured edits. The vanilla defaults are
    /// provided by the derived fallback, not by these groups' own rules.
    pub fn apply_generated_groups(&mut self) {
        generated_groups::insert_groups(&mut self.groups);
    }
}

impl Default for PermissionGroupsConfig {
    fn default() -> Self {
        let mut config = Self {
            default_groups: vec![generated_groups::DEFAULT_GROUP.to_owned()],
            op_group: generated_groups::OP_GROUP.to_owned(),
            command_block_groups: generated_groups::COMMAND_BLOCK_GROUPS
                .iter()
                .map(|group| (*group).to_owned())
                .collect(),
            groups: BTreeMap::new(),
        };
        config.apply_generated_groups();
        config
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
    /// Validates and parses permission group configuration without command fallbacks.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid names, rules, default groups, or inheritance.
    pub fn from_config(config: PermissionGroupsConfig) -> Result<Self, PermissionConfigError> {
        Self::from_config_with_fallbacks(config, &BTreeMap::new())
    }

    /// Validates and parses permission group configuration.
    ///
    /// `command_fallbacks` maps a vanilla group name to the command permission keys
    /// declared for it. Those keys join the group's derived allow-only fallback.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid names, rules, default groups, or inheritance.
    pub fn from_config_with_fallbacks(
        config: PermissionGroupsConfig,
        command_fallbacks: &BTreeMap<String, Vec<String>>,
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
            let permissions = parse_group_permissions(&name, group.allow, group.deny)?;
            let metadata = parse_group_metadata(&name, group.metadata)?;
            let fallback = parse_group_fallback(
                &name,
                group.priority,
                command_fallbacks.get(&name).map(Vec::as_slice),
            )?;
            groups.insert(
                name,
                PermissionGroup {
                    priority: group.priority,
                    inherits: group.inherits,
                    permissions,
                    metadata,
                    fallback,
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
    /// A permission unset by every group and override resolves against the groups' pristine
    /// allow-only fallbacks.
    #[must_use]
    pub fn effective_permissions(
        &self,
        assigned_groups: &[String],
        subject_permissions: &PermissionSet,
    ) -> PermissionSet {
        let mut effective = PermissionSet::new();
        let mut appended_groups = BTreeSet::new();
        let mut fallback = PermissionSet::new();
        let mut appended_fallbacks = BTreeSet::new();

        for group in &self.default_groups {
            self.append_group(group, &mut effective, &mut appended_groups);
            self.append_group_fallback(group, &mut fallback, &mut appended_fallbacks);
        }
        for group in assigned_groups {
            self.append_group(group, &mut effective, &mut appended_groups);
            self.append_group_fallback(group, &mut fallback, &mut appended_fallbacks);
        }
        for entry in subject_permissions.entries() {
            effective.push(entry.clone());
        }
        effective.set_fallback(fallback);
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

    fn append_group_fallback(
        &self,
        group_name: &str,
        fallback: &mut PermissionSet,
        appended_groups: &mut BTreeSet<String>,
    ) {
        if !appended_groups.insert(group_name.to_owned()) {
            return;
        }
        let Some(group) = self.groups.get(group_name) else {
            return;
        };
        for parent in &group.inherits {
            self.append_group_fallback(parent, fallback, appended_groups);
        }
        for entry in group.fallback.entries() {
            fallback.push_group(entry.clone(), group_name, group.priority);
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

/// Builds a group's derived allow-only fallback from the generated vanilla
/// defaults and the command permissions declared for the group.
fn parse_group_fallback(
    group: &str,
    priority: i32,
    command_keys: Option<&[String]>,
) -> Result<PermissionSet, PermissionConfigError> {
    let mut fallback = PermissionSet::new();
    if let Some(keys) = generated_groups::fallback(group) {
        push_fallback_rules(
            &mut fallback,
            group,
            priority,
            keys.iter().map(|key| (*key).to_owned()),
        )?;
    }
    if let Some(keys) = command_keys {
        push_fallback_rules(&mut fallback, group, priority, keys.iter().cloned())?;
    }
    Ok(fallback)
}

fn push_fallback_rules(
    fallback: &mut PermissionSet,
    group: &str,
    priority: i32,
    rules: impl IntoIterator<Item = String>,
) -> Result<(), PermissionConfigError> {
    for rule in rules {
        let expression = parse_group_rule(group, rule)?;
        let (key, context) = expression.into_parts();
        fallback.push_group(
            PermissionEntry::allow_with_context(key, context),
            group,
            priority,
        );
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
    fallback: PermissionSet,
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

    /// Returns this group's pristine allow-only fallback entries.
    #[must_use]
    pub const fn fallback(&self) -> &PermissionSet {
        &self.fallback
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
