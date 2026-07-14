use steel_utils::Identifier;

use super::{
    PermissionConfigError, PermissionEntry, PermissionGroupConfig, PermissionGroups,
    PermissionGroupsConfig, PermissionKey, PermissionMetadataEntry, PermissionMetadataRuleConfig,
    PermissionMetadataSet, PermissionMetadataValue, PermissionRuleContext,
    PermissionRuleExpression, PermissionRuleExpressionError, PermissionSet, PermissionState,
    parse_permission_metadata_key,
};

fn key(value: &str) -> PermissionKey {
    match PermissionKey::parse(value) {
        Ok(key) => key,
        Err(error) => panic!("test permission key '{value}' should parse: {error}"),
    }
}

fn groups(config: PermissionGroupsConfig) -> PermissionGroups {
    match PermissionGroups::from_config(config) {
        Ok(groups) => groups,
        Err(error) => panic!("test group config should resolve: {error}"),
    }
}

#[test]
fn groups_inherit_contextual_metadata_and_subject_values_break_ties() {
    let mut config = PermissionGroupsConfig::default();
    let mut parent = PermissionGroupConfig::default();
    parent.metadata.push(PermissionMetadataRuleConfig {
        key: "plugin:max_homes".to_owned(),
        value: PermissionMetadataValue::Integer(5),
    });
    parent.metadata.push(PermissionMetadataRuleConfig {
        key: "plugin:max_homes{domain=survival}".to_owned(),
        value: PermissionMetadataValue::Integer(3),
    });
    config.groups.insert("parent".to_owned(), parent);

    let mut child = PermissionGroupConfig::default();
    child.inherits.push("parent".to_owned());
    config.groups.insert("child".to_owned(), child);

    let homes = parse_permission_metadata_key("plugin:max_homes")
        .unwrap_or_else(|error| panic!("test metadata key should parse: {error}"));
    let subject = PermissionMetadataSet::from_entries([PermissionMetadataEntry::new(
        homes.clone(),
        PermissionMetadataValue::Integer(10),
    )]);
    let effective = groups(config).effective_metadata(&["child".to_owned()], &subject);

    assert_eq!(
        effective
            .resolve(&homes)
            .and_then(PermissionMetadataValue::as_i64),
        Some(10)
    );
    assert_eq!(
        effective
            .resolve_in(
                &homes,
                &super::PermissionContext::for_world(Identifier::new("survival", "overworld")),
            )
            .and_then(PermissionMetadataValue::as_i64),
        Some(3)
    );
}

#[test]
fn rule_expressions_parse_plain_and_contextual_keys() {
    let plain = PermissionRuleExpression::parse("minecraft.command.gamemode");
    let Ok(plain) = plain else {
        panic!("plain rule should parse");
    };
    assert_eq!(plain.key(), &key("minecraft.command.gamemode"));
    assert_eq!(plain.context(), &PermissionRuleContext::Global);
    assert_eq!(plain.to_string(), "minecraft.command.gamemode");

    let contextual = PermissionRuleExpression::parse(
        "minecraft.command.gamemode{plugin:region=spawn,world=lobby:spawn,domain=lobby}",
    );
    let Ok(contextual) = contextual else {
        panic!("contextual rule should parse");
    };
    assert_eq!(
        contextual.to_string(),
        "minecraft.command.gamemode{world=lobby:spawn,plugin:region=spawn}"
    );
}

#[test]
fn rule_expressions_reject_invalid_context_selectors() {
    assert!(matches!(
        PermissionRuleExpression::parse("steel.fly{plugin:region=spawn,plugin:region=market}"),
        Err(PermissionRuleExpressionError::DuplicateContextKey(key))
            if key == "plugin:region"
    ));
    assert!(matches!(
        PermissionRuleExpression::parse("steel.fly{plugin:region=spawn=bad}"),
        Err(PermissionRuleExpressionError::InvalidContextValue { key, value })
            if key == "plugin:region" && value == "spawn=bad"
    ));
    assert!(matches!(
        PermissionRuleExpression::parse("steel.fly{world=lobby/overworld}"),
        Err(PermissionRuleExpressionError::InvalidWorld(world))
            if world == "lobby/overworld"
    ));
    assert!(matches!(
        PermissionRuleExpression::parse("steel.fly{domain=}"),
        Err(PermissionRuleExpressionError::InvalidContextEntry(entry))
            if entry == "domain="
    ));
}

#[test]
fn contextual_group_rules_evaluate_in_the_parsed_context() {
    let mut config = PermissionGroupsConfig::default();
    let mut lobby = PermissionGroupConfig::default();
    lobby.allow.push("steel.fly{domain=lobby}".to_owned());
    config.groups.insert("lobby".to_owned(), lobby);
    let groups = groups(config);
    let effective = groups.effective_permissions(&["lobby".to_owned()], &PermissionSet::new());

    assert!(effective.allows_key_in(
        &key("steel.fly"),
        &super::PermissionContext::for_world(Identifier::new("lobby", "spawn"))
    ));
    assert!(!effective.allows_key_in(
        &key("steel.fly"),
        &super::PermissionContext::for_world(Identifier::new("survival", "overworld"))
    ));
}

#[test]
fn default_config_contains_editable_operator_group() {
    let groups = groups(PermissionGroupsConfig::default());

    assert!(groups.groups().contains_key("default"));
    assert!(groups.groups().contains_key("op"));
    assert_eq!(groups.default_groups(), ["default"]);
    assert!(
        groups
            .groups()
            .get("op")
            .is_some_and(|group| group.permissions().allows_key(&key("steel.admin")))
    );
}

#[test]
fn group_configuration_rejects_invalid_names_and_missing_operator_group() {
    let mut invalid_name = PermissionGroupsConfig::default();
    invalid_name
        .groups
        .insert("Admin Group".to_owned(), PermissionGroupConfig::default());
    assert!(matches!(
        PermissionGroups::from_config(invalid_name),
        Err(PermissionConfigError::InvalidGroupName { group, .. })
            if group == "Admin Group"
    ));

    let mut missing_op = PermissionGroupsConfig::default();
    missing_op.groups.remove("op");
    assert_eq!(
        PermissionGroups::from_config(missing_op),
        Err(PermissionConfigError::MissingRequiredGroup("op".to_owned()))
    );
}

#[test]
fn groups_inherit_parent_permissions_once() {
    let mut config = PermissionGroupsConfig::default();
    let mut builder = PermissionGroupConfig::default();
    builder.allow.push("steel.build".to_owned());
    config.groups.insert("builder".to_owned(), builder);

    let mut moderator = PermissionGroupConfig::default();
    moderator.inherits.push("builder".to_owned());
    moderator.allow.push("steel.kick".to_owned());
    config.groups.insert("moderator".to_owned(), moderator);

    let groups = groups(config);
    let effective = groups.effective_permissions(
        &["moderator".to_owned(), "builder".to_owned()],
        &PermissionSet::new(),
    );

    assert!(effective.allows_key(&key("steel.build")));
    assert!(effective.allows_key(&key("steel.kick")));
    assert_eq!(
        effective
            .entries()
            .iter()
            .filter(|entry| entry.key() == &key("steel.build"))
            .count(),
        1
    );
    assert_eq!(
        effective
            .resolve_key_detailed(&key("steel.build"))
            .and_then(|resolution| resolution.source().group_name().map(str::to_owned)),
        Some("builder".to_owned())
    );
}

#[test]
fn inherited_rules_keep_their_defining_group_priority() {
    let mut config = PermissionGroupsConfig::default();
    let mut parent = PermissionGroupConfig {
        priority: 50,
        ..PermissionGroupConfig::default()
    };
    parent.deny.push("steel.fly".to_owned());
    config.groups.insert("parent".to_owned(), parent);

    let mut child = PermissionGroupConfig::default();
    child.inherits.push("parent".to_owned());
    child.allow.push("steel.fly".to_owned());
    config.groups.insert("child".to_owned(), child);

    let effective =
        groups(config).effective_permissions(&["child".to_owned()], &PermissionSet::new());
    let resolution = effective.resolve_key_detailed(&key("steel.fly"));
    let Some(resolution) = resolution else {
        panic!("inherited rule should resolve");
    };
    assert_eq!(resolution.state(), PermissionState::Deny);
    assert_eq!(resolution.source().group_name(), Some("parent"));
    assert_eq!(resolution.source().group_priority(), Some(50));
}

#[test]
fn subject_rules_override_equal_group_rules() {
    let mut config = PermissionGroupsConfig::default();
    let mut group = PermissionGroupConfig::default();
    group.deny.push("steel.fly".to_owned());
    config.groups.insert("restricted".to_owned(), group);
    let subject = PermissionSet::from_entries([PermissionEntry::allow(key("steel.fly"))]);

    let effective = groups(config).effective_permissions(&["restricted".to_owned()], &subject);
    assert!(effective.allows_key(&key("steel.fly")));
}

#[test]
fn group_inheritance_rejects_missing_parents_and_cycles() {
    let mut missing = PermissionGroupsConfig::default();
    let mut child = PermissionGroupConfig::default();
    child.inherits.push("missing".to_owned());
    missing.groups.insert("child".to_owned(), child);
    assert!(matches!(
        PermissionGroups::from_config(missing),
        Err(PermissionConfigError::MissingInheritedGroup { group, inherited })
            if group == "child" && inherited == "missing"
    ));

    let mut cyclic = PermissionGroupsConfig::default();
    let mut first = PermissionGroupConfig::default();
    first.inherits.push("second".to_owned());
    cyclic.groups.insert("first".to_owned(), first);
    let mut second = PermissionGroupConfig::default();
    second.inherits.push("first".to_owned());
    cyclic.groups.insert("second".to_owned(), second);
    assert_eq!(
        PermissionGroups::from_config(cyclic),
        Err(PermissionConfigError::InheritedGroupCycle(vec![
            "first".to_owned(),
            "second".to_owned(),
            "first".to_owned(),
        ]))
    );
}

#[test]
fn default_groups_apply_and_unknown_assignments_are_ignored() {
    let mut config = PermissionGroupsConfig::default();
    let Some(default) = config.groups.get_mut("default") else {
        panic!("default config should contain its default group");
    };
    default.allow.push("steel.join".to_owned());
    let effective =
        groups(config).effective_permissions(&["deleted_group".to_owned()], &PermissionSet::new());

    assert!(effective.allows_key(&key("steel.join")));
    assert_eq!(effective.entries().len(), 1);
}

#[test]
fn groups_config_round_trips_through_toml() {
    let config = PermissionGroupsConfig::default();
    let serialized = match toml::to_string(&config) {
        Ok(serialized) => serialized,
        Err(error) => panic!("default config should serialize: {error}"),
    };
    let deserialized = match toml::from_str::<PermissionGroupsConfig>(&serialized) {
        Ok(deserialized) => deserialized,
        Err(error) => panic!("serialized config should parse: {error}"),
    };

    assert_eq!(deserialized, config);
}
