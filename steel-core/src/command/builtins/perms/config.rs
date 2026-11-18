//! Permission group config editing helpers.

use crate::permission::{
    PermissionGroupConfig, PermissionGroupRule, PermissionGroupsConfig,
    PermissionMetadataExpression, PermissionMetadataRuleConfig, PermissionMetadataValue,
    PermissionState,
};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub(super) enum PermissionGroupEditError {
    #[error("permission group '{0}' already exists")]
    AlreadyExists(String),
    #[error("unknown permission group '{0}'")]
    Missing(String),
    #[error("permission group '{0}' is required")]
    Required(String),
    #[error("permission group '{0}' is still a default group")]
    Default(String),
    #[error("permission group '{group}' is inherited by '{child}'")]
    InheritedBy { group: String, child: String },
    #[error("permission group '{0}' cannot inherit itself")]
    SelfInheritance(String),
}

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
    let is_required = config.op_group == group
        || config
            .command_block_groups
            .iter()
            .any(|required| required == group);
    if is_required {
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
    rule: &PermissionGroupRule,
    state: Option<PermissionState>,
) -> Result<bool, PermissionGroupEditError> {
    let Some(group_config) = config.groups.get_mut(group) else {
        return Err(PermissionGroupEditError::Missing(group.to_owned()));
    };
    let value = rule.to_string();
    let allow_count = group_config
        .allow
        .iter()
        .filter(|configured| group_rule_matches(configured, rule))
        .count();
    let deny_count = group_config
        .deny
        .iter()
        .filter(|configured| group_rule_matches(configured, rule))
        .count();
    group_config
        .allow
        .retain(|configured| !group_rule_matches(configured, rule));
    group_config
        .deny
        .retain(|configured| !group_rule_matches(configured, rule));
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

fn group_rule_matches(configured: &str, rule: &PermissionGroupRule) -> bool {
    PermissionGroupRule::parse(configured).is_ok_and(|parsed| parsed == *rule)
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

    fn permission(value: &str) -> PermissionGroupRule {
        match PermissionGroupRule::parse(value) {
            Ok(rule) => rule,
            Err(error) => panic!("test permission rule should parse: {error}"),
        }
    }

    #[test]
    fn permission_edits_replace_the_exact_context_only() {
        let mut config = PermissionGroupsConfig::default();
        let Some(group) = config.groups.get_mut("all") else {
            panic!("default permission group should exist");
        };
        group.allow.clear();
        let global = permission("steel.build");
        let survival = permission("steel.build{domain=survival}");

        assert_eq!(
            set_permission(&mut config, "all", &global, Some(PermissionState::Allow)),
            Ok(true)
        );
        assert_eq!(
            set_permission(&mut config, "all", &survival, Some(PermissionState::Deny)),
            Ok(true)
        );
        assert_eq!(set_permission(&mut config, "all", &global, None), Ok(true));
        assert_eq!(config.groups["all"].allow.len(), 0);
        assert_eq!(config.groups["all"].deny, ["steel.build{domain=survival}"]);
    }

    #[test]
    fn permission_edits_replace_collection_references_by_id() {
        let mut config = PermissionGroupsConfig::default();
        let Some(group) = config.groups.get_mut("all") else {
            panic!("default permission group should exist");
        };
        group.allow.clear();
        let collection = permission("#minecraft:all");

        assert_eq!(
            set_permission(
                &mut config,
                "all",
                &collection,
                Some(PermissionState::Allow)
            ),
            Ok(true)
        );
        assert_eq!(
            set_permission(&mut config, "all", &collection, Some(PermissionState::Deny)),
            Ok(true)
        );
        assert_eq!(config.groups["all"].allow.len(), 0);
        assert_eq!(config.groups["all"].deny, ["#minecraft:all"]);
    }

    #[test]
    fn permission_edits_replace_semantically_equal_context_orderings() {
        let mut config = PermissionGroupsConfig::default();
        let Some(group) = config.groups.get_mut("all") else {
            panic!("default permission group should exist");
        };
        group.allow.clear();
        group
            .allow
            .push("steel.build{plugin:region=spawn,domain=survival}".to_owned());
        let expression = permission("steel.build{domain=survival,plugin:region=spawn}");

        assert_eq!(
            set_permission(&mut config, "all", &expression, Some(PermissionState::Deny)),
            Ok(true)
        );
        assert_eq!(config.groups["all"].allow.len(), 0);
        assert_eq!(
            config.groups["all"].deny,
            ["steel.build{domain=survival,plugin:region=spawn}"]
        );
    }

    #[test]
    fn required_default_and_inherited_groups_cannot_be_deleted() {
        let mut config = PermissionGroupsConfig::default();
        let op_group = config.op_group.clone();
        assert!(matches!(
            delete_group(&mut config, &op_group),
            Err(PermissionGroupEditError::Required(_))
        ));
        assert!(matches!(
            delete_group(&mut config, "all"),
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
