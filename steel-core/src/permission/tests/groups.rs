use steel_utils::Identifier;

use super::super::{
    MODERATORS_COLLECTION, PermissionCollections, PermissionCollectionsConfig,
    PermissionConfigError, PermissionEntry, PermissionGroupConfig, PermissionGroupRule,
    PermissionGroups, PermissionGroupsConfig, PermissionKey, PermissionMetadataEntry,
    PermissionMetadataRuleConfig, PermissionMetadataSet, PermissionMetadataValue,
    PermissionRuleContext, PermissionRuleExpression, PermissionRuleExpressionError, PermissionSet,
    PermissionState, parse_permission_metadata_key,
};

fn key(value: &str) -> PermissionKey {
    match PermissionKey::parse(value) {
        Ok(key) => key,
        Err(error) => panic!("test permission key '{value}' should parse: {error}"),
    }
}

fn groups(config: PermissionGroupsConfig) -> PermissionGroups {
    match PermissionGroups::from_config(config, &PermissionCollections::vanilla()) {
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
                &super::super::PermissionContext::for_world(Identifier::new(
                    "survival",
                    "overworld"
                )),
            )
            .and_then(PermissionMetadataValue::as_i64),
        Some(3)
    );
}

#[test]
fn rule_expressions_parse_plain_and_contextual_keys() {
    let plain = PermissionRuleExpression::parse("minecraft:command.gamemode");
    let Ok(plain) = plain else {
        panic!("plain rule should parse");
    };
    assert_eq!(plain.key(), &key("minecraft:command.gamemode"));
    assert_eq!(plain.context(), &PermissionRuleContext::Global);
    assert_eq!(plain.to_string(), "minecraft:command.gamemode");

    let contextual = PermissionRuleExpression::parse(
        "minecraft:command.gamemode{plugin:region=spawn,world=lobby:spawn,domain=lobby}",
    );
    let Ok(contextual) = contextual else {
        panic!("contextual rule should parse");
    };
    assert_eq!(
        contextual.to_string(),
        "minecraft:command.gamemode{world=lobby:spawn,plugin:region=spawn}"
    );
}

#[test]
fn rule_expressions_reject_invalid_context_selectors() {
    assert!(matches!(
        PermissionRuleExpression::parse("steel:fly{plugin:region=spawn,plugin:region=market}"),
        Err(PermissionRuleExpressionError::DuplicateContextKey(key))
            if key == "plugin:region"
    ));
    assert!(matches!(
        PermissionRuleExpression::parse("steel:fly{plugin:region=spawn=bad}"),
        Err(PermissionRuleExpressionError::InvalidContextValue { key, value })
            if key == "plugin:region" && value == "spawn=bad"
    ));
    assert!(matches!(
        PermissionRuleExpression::parse("steel:fly{world=lobby/overworld}"),
        Err(PermissionRuleExpressionError::InvalidWorld(world))
            if world == "lobby/overworld"
    ));
    assert!(matches!(
        PermissionRuleExpression::parse("steel:fly{domain=}"),
        Err(PermissionRuleExpressionError::InvalidContextEntry(entry))
            if entry == "domain="
    ));
}

#[test]
fn contextual_group_rules_evaluate_in_the_parsed_context() {
    let mut config = PermissionGroupsConfig::default();
    let mut lobby = PermissionGroupConfig::default();
    lobby.allow.push("steel:fly{domain=lobby}".to_owned());
    config.groups.insert("lobby".to_owned(), lobby);
    let groups = groups(config);
    let effective = groups.effective_permissions(&["lobby".to_owned()], &PermissionSet::new());

    assert!(effective.allows_key_in(
        &key("steel:fly"),
        &super::super::PermissionContext::for_world(Identifier::new("lobby", "spawn"))
    ));
    assert!(!effective.allows_key_in(
        &key("steel:fly"),
        &super::super::PermissionContext::for_world(Identifier::new("survival", "overworld"))
    ));
}

#[test]
fn default_config_contains_editable_operator_group() {
    let config = PermissionGroupsConfig::default();
    let op_group = config.op_group.clone();
    let default_group = config.default_groups[0].clone();
    let groups = groups(config);

    assert!(groups.groups().contains_key(&default_group));
    assert!(groups.groups().contains_key(&op_group));
    assert_eq!(groups.default_groups(), [default_group]);
    let operator = groups
        .groups()
        .get(&op_group)
        .expect("default config should contain the operator group");
    assert!(
        !operator.permissions().entries().is_empty(),
        "operator group should carry the vanilla operator collection"
    );
}

#[test]
fn group_configuration_rejects_invalid_names_and_missing_operator_group() {
    let mut invalid_name = PermissionGroupsConfig::default();
    invalid_name
        .groups
        .insert("Admin Group".to_owned(), PermissionGroupConfig::default());
    assert!(matches!(
        PermissionGroups::from_config(invalid_name, &PermissionCollections::vanilla()),
        Err(PermissionConfigError::InvalidGroupName { group, .. })
            if group == "Admin Group"
    ));

    let mut missing_op = PermissionGroupsConfig::default();
    let op_group = missing_op.op_group.clone();
    missing_op.groups.remove(&op_group);
    assert_eq!(
        PermissionGroups::from_config(missing_op, &PermissionCollections::vanilla()),
        Err(PermissionConfigError::MissingRequiredGroup(op_group))
    );
}

#[test]
fn groups_inherit_parent_permissions_once() {
    let mut config = PermissionGroupsConfig::default();
    let mut builder = PermissionGroupConfig::default();
    builder.allow.push("steel:build".to_owned());
    config.groups.insert("builder".to_owned(), builder);

    let mut moderator = PermissionGroupConfig::default();
    moderator.inherits.push("builder".to_owned());
    moderator.allow.push("steel:kick".to_owned());
    config.groups.insert("moderator".to_owned(), moderator);

    let groups = groups(config);
    let effective = groups.effective_permissions(
        &["moderator".to_owned(), "builder".to_owned()],
        &PermissionSet::new(),
    );

    assert!(effective.allows_key(&key("steel:build")));
    assert!(effective.allows_key(&key("steel:kick")));
    assert_eq!(
        effective
            .entries()
            .iter()
            .filter(|entry| entry.key() == &key("steel:build"))
            .count(),
        1
    );
    assert_eq!(
        effective
            .resolve_key_detailed(&key("steel:build"))
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
    parent.deny.push("steel:fly".to_owned());
    config.groups.insert("parent".to_owned(), parent);

    let mut child = PermissionGroupConfig::default();
    child.inherits.push("parent".to_owned());
    child.allow.push("steel:fly".to_owned());
    config.groups.insert("child".to_owned(), child);

    let effective =
        groups(config).effective_permissions(&["child".to_owned()], &PermissionSet::new());
    let resolution = effective.resolve_key_detailed(&key("steel:fly"));
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
    group.deny.push("steel:fly".to_owned());
    config.groups.insert("restricted".to_owned(), group);
    let subject = PermissionSet::from_entries([PermissionEntry::allow(key("steel:fly"))]);

    let effective = groups(config).effective_permissions(&["restricted".to_owned()], &subject);
    assert!(effective.allows_key(&key("steel:fly")));
}

#[test]
fn group_inheritance_rejects_missing_parents_and_cycles() {
    let mut missing = PermissionGroupsConfig::default();
    let mut child = PermissionGroupConfig::default();
    child.inherits.push("missing".to_owned());
    missing.groups.insert("child".to_owned(), child);
    assert!(matches!(
        PermissionGroups::from_config(missing, &PermissionCollections::vanilla()),
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
        PermissionGroups::from_config(cyclic, &PermissionCollections::vanilla()),
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
    let default_group = config.default_groups[0].clone();
    let Some(default) = config.groups.get_mut(&default_group) else {
        panic!("default config should contain its default group");
    };
    default.allow.clear();
    default.allow.push("steel:join".to_owned());
    let effective =
        groups(config).effective_permissions(&["deleted_group".to_owned()], &PermissionSet::new());

    assert!(effective.allows_key(&key("steel:join")));
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

#[test]
fn runtime_declared_collections_reach_inheriting_groups() {
    let mut declared = PermissionCollections::new();
    declared.push(
        Identifier::new("minecraft", "gamemasters"),
        PermissionGroupRule::permission(Identifier::new("steel", "command.fly")),
    );
    let collections = PermissionCollections::effective(&declared, &PermissionCollections::new());

    let config = PermissionGroupsConfig::default();
    let op_group = config.op_group.clone();
    let groups = match PermissionGroups::from_config(config, &collections) {
        Ok(groups) => groups,
        Err(error) => panic!("test group config should resolve: {error}"),
    };
    let operator = groups.effective_permissions(&[op_group], &PermissionSet::new());
    assert!(operator.allows_identifier(&Identifier::new("steel", "command.fly")));

    let mut config = PermissionGroupsConfig::default();
    config
        .groups
        .insert("custom".to_owned(), PermissionGroupConfig::default());
    let groups = match PermissionGroups::from_config(config, &collections) {
        Ok(groups) => groups,
        Err(error) => panic!("test group config should resolve: {error}"),
    };
    let custom = groups.effective_permissions(&["custom".to_owned()], &PermissionSet::new());
    assert!(!custom.allows_identifier(&Identifier::new("steel", "command.fly")));
}

#[test]
fn wildcard_collection_members_apply_through_group_references() {
    let mut config = PermissionCollectionsConfig::default();
    config
        .collections
        .entry("steel".to_owned())
        .or_default()
        .insert(
            "build".to_owned(),
            vec![
                "minecraft:command.*".to_owned(),
                "#minecraft:moderators".to_owned(),
            ],
        );
    let collections = match PermissionCollections::from_config(&config) {
        Ok(collections) => collections,
        Err(error) => panic!("test collection config should resolve: {error}"),
    };
    let expressions = collections.expand(&Identifier::new("steel", "build"));
    assert!(
        expressions
            .iter()
            .any(|expression| expression.key().as_str() == "minecraft:command.*")
    );

    let mut groups_config = PermissionGroupsConfig::default();
    groups_config.groups.insert(
        "builder".to_owned(),
        PermissionGroupConfig {
            allow: vec!["#steel:build".to_owned()],
            ..PermissionGroupConfig::default()
        },
    );
    let groups = match PermissionGroups::from_config(groups_config, &collections) {
        Ok(groups) => groups,
        Err(error) => panic!("test group config should resolve: {error}"),
    };
    let builder = groups.effective_permissions(&["builder".to_owned()], &PermissionSet::new());
    assert!(builder.allows_key(&key("minecraft:command.give")));
    assert!(!builder.allows_key(&key("minecraft:other")));
}

#[test]
fn vanilla_management_authorities_are_scoped_to_each_level() {
    let groups = groups(PermissionGroupsConfig::default());

    let all = groups.effective_permissions(&[], &PermissionSet::new());
    let admins = groups.effective_permissions(&["admins".to_owned()], &PermissionSet::new());
    let owners = groups.effective_permissions(&["owners".to_owned()], &PermissionSet::new());

    let manage_give = Identifier::new("steel", "permission.manage.minecraft.command.give");
    let manage_stop = Identifier::new("steel", "permission.manage.minecraft.command.stop");
    let group_all = Identifier::new("steel", "permission.group.all");
    let group_admins = Identifier::new("steel", "permission.group.admins");
    let group_owners = Identifier::new("steel", "permission.group.owners");
    let manage_group_all = Identifier::new("steel", "permission.manage.steel.permission.group.all");
    let manage_group_admins =
        Identifier::new("steel", "permission.manage.steel.permission.group.admins");
    let manage_group_moderators = Identifier::new(
        "steel",
        "permission.manage.steel.permission.group.moderators",
    );
    let manage_group_owners =
        Identifier::new("steel", "permission.manage.steel.permission.group.owners");
    let metadata = Identifier::new("steel", "permission.metadata");
    let manage_wildcard = key("steel:permission.manage.*");
    let group_wildcard = key("steel:permission.group.*");

    // `all` is the only vanilla level without any administration surface.
    assert!(!all.allows_identifier(&manage_give));
    assert!(!all.allows_identifier(&group_all));
    assert!(!all.allows_identifier(&group_admins));
    assert!(!all.allows_identifier(&manage_group_all));
    assert!(!all.allows_identifier(&metadata));

    // `moderators` has no vanilla commands, but still carries its management authorities.
    let moderators =
        groups.effective_permissions(&["moderators".to_owned()], &PermissionSet::new());
    assert!(moderators.allows_identifier(&Identifier::new("steel", "permission.group.moderators")));
    assert!(moderators.allows_identifier(&manage_group_moderators));
    assert!(!moderators.allows_identifier(&group_all));
    assert!(!moderators.allows_identifier(&manage_group_all));

    // `admins` inherits its own level and everything below it, including `all`, but not owners.
    assert!(admins.allows_identifier(&manage_give));
    assert!(admins.allows_identifier(&group_all));
    assert!(admins.allows_identifier(&group_admins));
    assert!(admins.allows_identifier(&manage_group_all));
    assert!(admins.allows_identifier(&manage_group_admins));
    assert!(admins.allows_identifier(&metadata));
    assert!(!admins.allows_identifier(&manage_stop));
    assert!(!admins.allows_identifier(&group_owners));
    assert!(!admins.allows_identifier(&manage_group_owners));
    assert!(!admins.allows_key(&key("steel:permission.manage.*")));
    assert!(!admins.allows_key(&key("steel:permission.group.*")));

    // `owners` can administer its own level and hold both root wildcards.
    assert!(owners.allows_identifier(&manage_stop));
    assert!(owners.allows_identifier(&group_all));
    assert!(owners.allows_identifier(&group_owners));
    assert!(owners.allows_identifier(&manage_group_owners));
    assert!(owners.allows_key(&manage_wildcard));
    assert!(owners.allows_key(&group_wildcard));
}

#[test]
fn vanilla_moderators_collection_grants_perms_command_to_ops() {
    let mut declared = PermissionCollections::new();
    declared.push(
        MODERATORS_COLLECTION,
        PermissionGroupRule::permission(Identifier::from_steel("command.perms")),
    );
    let collections = PermissionCollections::effective(&declared, &PermissionCollections::new());
    let groups =
        match PermissionGroups::from_config(PermissionGroupsConfig::default(), &collections) {
            Ok(groups) => groups,
            Err(error) => panic!("default group config should resolve: {error}"),
        };

    let command = Identifier::from_steel("command.perms");
    let effective =
        |assigned: &[String]| groups.effective_permissions(assigned, &PermissionSet::new());

    assert!(!effective(&[]).allows_identifier(&command));
    assert!(effective(&["moderators".to_owned()]).allows_identifier(&command));
    assert!(effective(&["owners".to_owned()]).allows_identifier(&command));
}
