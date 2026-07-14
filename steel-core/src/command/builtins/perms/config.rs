//! Permission group config editing helpers.

use std::{error::Error, fmt};

use crate::permission::{
    OP_GROUP, PermissionGroupConfig, PermissionGroupsConfig, PermissionMetadataExpression,
    PermissionMetadataRuleConfig, PermissionMetadataValue, PermissionRuleExpression,
    PermissionState,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PermissionGroupEditError {
    AlreadyExists(String),
    Missing(String),
    Required(String),
    Default(String),
    InheritedBy { group: String, child: String },
    SelfInheritance(String),
}

impl fmt::Display for PermissionGroupEditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyExists(group) => {
                write!(formatter, "permission group '{group}' already exists")
            }
            Self::Missing(group) => write!(formatter, "unknown permission group '{group}'"),
            Self::Required(group) => write!(formatter, "permission group '{group}' is required"),
            Self::Default(group) => write!(
                formatter,
                "permission group '{group}' is still a default group"
            ),
            Self::InheritedBy { group, child } => {
                write!(
                    formatter,
                    "permission group '{group}' is inherited by '{child}'"
                )
            }
            Self::SelfInheritance(group) => {
                write!(
                    formatter,
                    "permission group '{group}' cannot inherit itself"
                )
            }
        }
    }
}

impl Error for PermissionGroupEditError {}

pub(super) fn create_group(
    config: &mut PermissionGroupsConfig,
    group: &str,
) -> Result<bool, PermissionGroupEditError> {
    if config.groups.contains_key(group) {
        return Err(PermissionGroupEditError::AlreadyExists(group.to_owned()));
    }
    config
        .groups
        .insert(group.to_owned(), PermissionGroupConfig::default());
    Ok(true)
}

pub(super) fn delete_group(
    config: &mut PermissionGroupsConfig,
    group: &str,
) -> Result<bool, PermissionGroupEditError> {
    if group == OP_GROUP {
        return Err(PermissionGroupEditError::Required(group.to_owned()));
    }
    if !config.groups.contains_key(group) {
        return Err(PermissionGroupEditError::Missing(group.to_owned()));
    }
    if config.default_groups.iter().any(|default| default == group) {
        return Err(PermissionGroupEditError::Default(group.to_owned()));
    }
    if let Some((child, _)) = config
        .groups
        .iter()
        .find(|(_, child)| child.inherits.iter().any(|parent| parent == group))
    {
        return Err(PermissionGroupEditError::InheritedBy {
            group: group.to_owned(),
            child: child.clone(),
        });
    }
    config.groups.remove(group);
    Ok(true)
}

pub(super) fn set_default_group(
    config: &mut PermissionGroupsConfig,
    group: &str,
    add: bool,
) -> Result<bool, PermissionGroupEditError> {
    if !config.groups.contains_key(group) {
        return Err(PermissionGroupEditError::Missing(group.to_owned()));
    }
    let present = config.default_groups.iter().any(|default| default == group);
    if add {
        if present {
            return Ok(false);
        }
        config.default_groups.push(group.to_owned());
        return Ok(true);
    }
    if !present {
        return Ok(false);
    }
    config.default_groups.retain(|default| default != group);
    Ok(true)
}

pub(super) fn set_permission(
    config: &mut PermissionGroupsConfig,
    group: &str,
    expression: &PermissionRuleExpression,
    state: Option<PermissionState>,
) -> Result<bool, PermissionGroupEditError> {
    let Some(group_config) = config.groups.get_mut(group) else {
        return Err(PermissionGroupEditError::Missing(group.to_owned()));
    };
    let value = expression.to_string();
    let allow_count = group_config
        .allow
        .iter()
        .filter(|rule| permission_rule_matches(rule, expression))
        .count();
    let deny_count = group_config
        .deny
        .iter()
        .filter(|rule| permission_rule_matches(rule, expression))
        .count();
    group_config
        .allow
        .retain(|rule| !permission_rule_matches(rule, expression));
    group_config
        .deny
        .retain(|rule| !permission_rule_matches(rule, expression));
    match state {
        Some(PermissionState::Allow) => {
            group_config.allow.push(value);
            Ok(allow_count != 1 || deny_count != 0)
        }
        Some(PermissionState::Deny) => {
            group_config.deny.push(value);
            Ok(deny_count != 1 || allow_count != 0)
        }
        None => Ok(allow_count != 0 || deny_count != 0),
    }
}

pub(super) fn set_metadata(
    config: &mut PermissionGroupsConfig,
    group: &str,
    expression: &PermissionMetadataExpression,
    value: Option<PermissionMetadataValue>,
) -> Result<bool, PermissionGroupEditError> {
    let Some(group_config) = config.groups.get_mut(group) else {
        return Err(PermissionGroupEditError::Missing(group.to_owned()));
    };
    let expression_text = expression.to_string();
    let previous = group_config
        .metadata
        .iter()
        .find(|rule| permission_metadata_matches(&rule.key, expression))
        .map(|rule| rule.value.clone());
    group_config
        .metadata
        .retain(|rule| !permission_metadata_matches(&rule.key, expression));
    let Some(value) = value else {
        return Ok(previous.is_some());
    };
    let changed = previous.as_ref() != Some(&value);
    group_config.metadata.push(PermissionMetadataRuleConfig {
        key: expression_text,
        value,
    });
    Ok(changed)
}

fn permission_rule_matches(configured: &str, expression: &PermissionRuleExpression) -> bool {
    PermissionRuleExpression::parse(configured).is_ok_and(|parsed| parsed == *expression)
}

fn permission_metadata_matches(
    configured: &str,
    expression: &PermissionMetadataExpression,
) -> bool {
    PermissionMetadataExpression::parse(configured).is_ok_and(|parsed| parsed == *expression)
}

pub(super) fn set_priority(
    config: &mut PermissionGroupsConfig,
    group: &str,
    priority: i32,
) -> Result<bool, PermissionGroupEditError> {
    let Some(group_config) = config.groups.get_mut(group) else {
        return Err(PermissionGroupEditError::Missing(group.to_owned()));
    };
    if group_config.priority == priority {
        return Ok(false);
    }
    group_config.priority = priority;
    Ok(true)
}

pub(super) fn set_inheritance(
    config: &mut PermissionGroupsConfig,
    group: &str,
    parent: &str,
    add: bool,
) -> Result<bool, PermissionGroupEditError> {
    if group == parent {
        return Err(PermissionGroupEditError::SelfInheritance(group.to_owned()));
    }
    if !config.groups.contains_key(parent) {
        return Err(PermissionGroupEditError::Missing(parent.to_owned()));
    }
    let Some(group_config) = config.groups.get_mut(group) else {
        return Err(PermissionGroupEditError::Missing(group.to_owned()));
    };
    let present = group_config
        .inherits
        .iter()
        .any(|inherited| inherited == parent);
    if add {
        if present {
            return Ok(false);
        }
        group_config.inherits.push(parent.to_owned());
        return Ok(true);
    }
    if !present {
        return Ok(false);
    }
    group_config
        .inherits
        .retain(|inherited| inherited != parent);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn permission(value: &str) -> PermissionRuleExpression {
        match PermissionRuleExpression::parse(value) {
            Ok(expression) => expression,
            Err(error) => panic!("test permission expression should parse: {error}"),
        }
    }

    #[test]
    fn permission_edits_replace_the_exact_context_only() {
        let mut config = PermissionGroupsConfig::default();
        let global = permission("steel.build");
        let survival = permission("steel.build{domain=survival}");

        assert_eq!(
            set_permission(
                &mut config,
                "default",
                &global,
                Some(PermissionState::Allow)
            ),
            Ok(true)
        );
        assert_eq!(
            set_permission(
                &mut config,
                "default",
                &survival,
                Some(PermissionState::Deny)
            ),
            Ok(true)
        );
        assert_eq!(
            set_permission(&mut config, "default", &global, None),
            Ok(true)
        );
        assert!(config.groups["default"].allow.is_empty());
        assert_eq!(
            config.groups["default"].deny,
            ["steel.build{domain=survival}"]
        );
    }

    #[test]
    fn permission_edits_replace_semantically_equal_context_orderings() {
        let mut config = PermissionGroupsConfig::default();
        let Some(group) = config.groups.get_mut("default") else {
            panic!("default permission group should exist");
        };
        group
            .allow
            .push("steel.build{plugin:region=spawn,domain=survival}".to_owned());
        let expression = permission("steel.build{domain=survival,plugin:region=spawn}");

        assert_eq!(
            set_permission(
                &mut config,
                "default",
                &expression,
                Some(PermissionState::Deny)
            ),
            Ok(true)
        );
        assert!(config.groups["default"].allow.is_empty());
        assert_eq!(
            config.groups["default"].deny,
            ["steel.build{domain=survival,plugin:region=spawn}"]
        );
    }

    #[test]
    fn required_default_and_inherited_groups_cannot_be_deleted() {
        let mut config = PermissionGroupsConfig::default();
        assert!(matches!(
            delete_group(&mut config, OP_GROUP),
            Err(PermissionGroupEditError::Required(_))
        ));
        assert!(matches!(
            delete_group(&mut config, "default"),
            Err(PermissionGroupEditError::Default(_))
        ));
        assert_eq!(create_group(&mut config, "parent"), Ok(true));
        assert_eq!(create_group(&mut config, "child"), Ok(true));
        assert_eq!(
            set_inheritance(&mut config, "child", "parent", true),
            Ok(true)
        );
        assert!(matches!(
            delete_group(&mut config, "parent"),
            Err(PermissionGroupEditError::InheritedBy { .. })
        ));
    }
}
