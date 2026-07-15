use steel_utils::Identifier;

use super::{
    PermissionContext, PermissionContextKey, PermissionEntry, PermissionExpr, PermissionKey,
    PermissionKeyError, PermissionMetadataEntry, PermissionMetadataExpression,
    PermissionMetadataKeyError, PermissionMetadataSet, PermissionMetadataValue,
    PermissionResolutionSource, PermissionRuleContext, PermissionRuleContextError,
    PermissionSegment, PermissionSet, PermissionState, parse_permission_metadata_key,
};

fn key(value: &str) -> PermissionKey {
    match PermissionKey::parse(value) {
        Ok(key) => key,
        Err(error) => panic!("test permission key '{value}' should parse: {error}"),
    }
}

fn context_key(value: &str) -> PermissionContextKey {
    match PermissionContextKey::parse(value) {
        Ok(key) => key,
        Err(error) => panic!("test context key '{value}' should parse: {error}"),
    }
}

fn world_context(domain: &str, world: &str) -> PermissionContext {
    PermissionContext::for_world(Identifier::new(domain.to_owned(), world.to_owned()))
}

fn domain_context(domain: &str) -> PermissionRuleContext {
    match PermissionRuleContext::domain(domain) {
        Ok(context) => context,
        Err(error) => panic!("test domain context should build: {error}"),
    }
}

fn custom_context(key: &str, value: &str) -> PermissionRuleContext {
    match PermissionRuleContext::custom(context_key(key), value) {
        Ok(context) => context,
        Err(error) => panic!("test custom context should build: {error}"),
    }
}

fn metadata_key(value: &str) -> Identifier {
    match parse_permission_metadata_key(value) {
        Ok(key) => key,
        Err(error) => panic!("test metadata key '{value}' should parse: {error}"),
    }
}

#[test]
fn metadata_keys_require_explicit_valid_namespaces() {
    assert_eq!(
        parse_permission_metadata_key("max_homes"),
        Err(PermissionMetadataKeyError::InvalidFormat)
    );
    assert_eq!(
        parse_permission_metadata_key(":max_homes"),
        Err(PermissionMetadataKeyError::EmptyNamespace)
    );
    assert_eq!(
        parse_permission_metadata_key("plugin:homes//max"),
        Err(PermissionMetadataKeyError::InvalidPath)
    );
    assert_eq!(
        parse_permission_metadata_key("plugin:max_homes"),
        Ok(Identifier::new("plugin", "max_homes"))
    );
}

#[test]
fn metadata_expressions_share_permission_context_syntax() {
    let expression = PermissionMetadataExpression::parse(
        "plugin:max_homes{plugin:region=spawn,world=lobby:spawn,domain=lobby}",
    );
    let Ok(expression) = expression else {
        panic!("metadata expression should parse");
    };
    assert_eq!(
        expression.to_string(),
        "plugin:max_homes{world=lobby:spawn,plugin:region=spawn}"
    );
}

#[test]
fn metadata_resolution_uses_context_source_priority_and_order() {
    let homes = metadata_key("plugin:max_homes");
    let mut metadata = PermissionMetadataSet::new();
    metadata.push_group(
        PermissionMetadataEntry::new(homes.clone(), PermissionMetadataValue::Integer(5)),
        "default",
        0,
    );
    metadata.push_group(
        PermissionMetadataEntry::new(homes.clone(), PermissionMetadataValue::Integer(10)),
        "vip",
        50,
    );
    metadata.push(PermissionMetadataEntry::new(
        homes.clone(),
        PermissionMetadataValue::Integer(20),
    ));
    metadata.push_group(
        PermissionMetadataEntry::new_with_context(
            homes.clone(),
            domain_context("survival"),
            PermissionMetadataValue::Integer(3),
        ),
        "default",
        0,
    );

    assert_eq!(
        metadata
            .resolve(&homes)
            .and_then(PermissionMetadataValue::as_i64),
        Some(20)
    );
    assert_eq!(
        metadata
            .resolve_in(&homes, &world_context("survival", "overworld"))
            .and_then(PermissionMetadataValue::as_i64),
        Some(3)
    );
    let resolution = metadata.resolve_detailed(&homes);
    let Some(resolution) = resolution else {
        panic!("metadata should resolve");
    };
    assert_eq!(resolution.source(), &PermissionResolutionSource::Subject);
}

#[test]
fn keys_validate_segments_and_wildcard_position() {
    assert_eq!(
        PermissionKey::parse("minecraft.*.give"),
        Err(PermissionKeyError::WildcardNotFinal)
    );
    assert_eq!(
        PermissionKey::parse("minecraft.command.g*"),
        Err(PermissionKeyError::InvalidWildcardSegment)
    );
    assert_eq!(
        PermissionKey::parse("Minecraft.command.give"),
        Err(PermissionKeyError::InvalidSegment)
    );
    assert_eq!(
        PermissionSegment::parse("give.item"),
        Err(PermissionKeyError::InvalidSegment)
    );
}

#[test]
fn typed_segments_build_child_keys() {
    let segments =
        ["minecraft", "command", "give"].map(|value| match PermissionSegment::parse(value) {
            Ok(segment) => segment,
            Err(error) => panic!("test segment should parse: {error}"),
        });
    let built = PermissionKey::from_segments(segments);
    let Ok(built) = built else {
        panic!("typed segments should build a key");
    };
    assert_eq!(built.as_str(), "minecraft.command.give");

    let segment = match PermissionSegment::parse("freeze") {
        Ok(segment) => segment,
        Err(error) => panic!("test segment should parse: {error}"),
    };
    let child = key("minecraft.command.tick").child(&segment);
    assert_eq!(
        child.as_ref().map(PermissionKey::as_str),
        Ok("minecraft.command.tick.freeze")
    );
    assert_eq!(
        key("minecraft.command.*").child(&segment),
        Err(PermissionKeyError::WildcardNotFinal)
    );
}

#[test]
fn wildcards_match_only_their_descendants() {
    assert!(key("*").matches(&key("some.plugin.dangerous")));
    assert!(key("minecraft.command.*").matches(&key("minecraft.command.give")));
    assert!(!key("minecraft.command.*").matches(&key("minecraft.other.give")));
    assert!(!key("minecraft.command.*").matches(&key("minecraft.command")));
}

#[test]
fn chained_contexts_are_canonical_and_idempotent() {
    let domain = domain_context("lobby");
    let world = PermissionRuleContext::world(Identifier::new("lobby", "spawn"));
    let region = custom_context("plugin:region", "spawn");
    let first = PermissionRuleContext::all([region.clone(), world.clone(), domain.clone()]);
    let second = PermissionRuleContext::all([domain, world, region]);

    assert_eq!(first, second);
    let Ok(first) = first else {
        panic!("non-conflicting contexts should chain");
    };
    assert_eq!(first.to_string(), "world lobby:spawn + plugin:region spawn");

    let same = domain_context("lobby");
    assert_eq!(
        PermissionRuleContext::all([same.clone(), same.clone()]),
        Ok(same)
    );
}

#[test]
fn chained_contexts_reject_conflicting_values() {
    assert_eq!(
        PermissionRuleContext::all([domain_context("lobby"), domain_context("survival"),]),
        Err(PermissionRuleContextError::DuplicateDomain)
    );
    assert_eq!(
        PermissionRuleContext::all([
            custom_context("region", "spawn"),
            custom_context("region", "market"),
        ]),
        Err(PermissionRuleContextError::DuplicateCustomKey(context_key(
            "region"
        )))
    );
}

#[test]
fn contexts_reject_non_roundtrippable_values_and_mismatched_world_domains() {
    assert!(matches!(
        PermissionRuleContext::custom(context_key("plugin:region"), "spawn,market"),
        Err(PermissionRuleContextError::InvalidValue(_))
    ));
    assert!(matches!(
        PermissionRuleContext::domain("Uppercase"),
        Err(PermissionRuleContextError::InvalidDomain(_))
    ));
    assert_eq!(
        PermissionRuleContext::all([
            domain_context("lobby"),
            PermissionRuleContext::world(Identifier::new("survival", "overworld")),
        ]),
        Err(PermissionRuleContextError::WorldDomainMismatch {
            domain: "lobby".to_owned(),
            world_domain: "survival".to_owned(),
        })
    );
}

#[test]
fn active_context_rejects_conflicting_custom_values() {
    let context = PermissionContext::global()
        .with_custom_context(context_key("region"), "spawn")
        .and_then(|context| context.with_custom_context(context_key("region"), "spawn"));
    let Ok(context) = context else {
        panic!("same custom context should be idempotent");
    };

    assert_eq!(
        context.with_custom_context(context_key("region"), "market"),
        Err(PermissionRuleContextError::DuplicateCustomKey(context_key(
            "region"
        )))
    );
}

#[test]
fn unset_permissions_default_to_deny() {
    let permissions = PermissionSet::new();
    let give = key("minecraft.command.give");

    assert_eq!(permissions.resolve_key(&give), None);
    assert!(!permissions.allows_key(&give));
}

#[test]
fn key_specificity_precedes_context_specificity() {
    let creative = key("minecraft.command.gamemode.creative");
    let permissions = PermissionSet::from_entries([
        PermissionEntry::allow_with_context(key("minecraft.command.*"), domain_context("lobby")),
        PermissionEntry::deny(creative.clone()),
    ]);

    assert!(!permissions.allows_key_in(&creative, &world_context("lobby", "spawn")));
}

#[test]
fn context_specificity_breaks_equal_key_ties() {
    let fly = key("steel.fly");
    let permissions = PermissionSet::from_entries([
        PermissionEntry::deny(fly.clone()),
        PermissionEntry::allow_with_context(fly.clone(), domain_context("lobby")),
    ]);

    assert!(permissions.allows_key_in(&fly, &world_context("lobby", "spawn")));
    assert!(!permissions.allows_key_in(&fly, &world_context("survival", "overworld")));
}

#[test]
fn chained_context_is_more_specific_than_one_constraint() {
    let fly = key("steel.fly");
    let world = PermissionRuleContext::world(Identifier::new("lobby", "spawn"));
    let chained =
        PermissionRuleContext::all([world.clone(), custom_context("plugin:region", "spawn")]);
    let Ok(chained) = chained else {
        panic!("test context chain should build");
    };
    let permissions = PermissionSet::from_entries([
        PermissionEntry::allow_with_context(fly.clone(), world),
        PermissionEntry::deny_with_context(fly.clone(), chained),
    ]);
    let matching = PermissionContext::for_world(Identifier::new("lobby", "spawn"))
        .with_custom_context(context_key("plugin:region"), "spawn");
    let Ok(matching) = matching else {
        panic!("active test context should build");
    };

    assert!(!permissions.allows_key_in(&fly, &matching));
    assert!(permissions.allows_key_in(&fly, &world_context("lobby", "spawn")));
}

#[test]
fn rule_context_builds_an_equivalent_active_context_for_admin_checks() {
    let rule = PermissionRuleContext::all([
        domain_context("lobby"),
        PermissionRuleContext::world(Identifier::new("lobby", "spawn")),
        custom_context("plugin:region", "market"),
    ]);
    let Ok(rule) = rule else {
        panic!("test rule context should compose");
    };
    let active = PermissionContext::from_rule_context(&rule);
    let Ok(active) = active else {
        panic!("rule context should become an active context");
    };

    assert!(rule.matches(&active));
}

#[test]
fn scoped_child_rules_override_parent_fallbacks() {
    let parent = key("minecraft.command.gamemode");
    let creative = key("minecraft.command.gamemode.creative");
    let survival = key("minecraft.command.gamemode.survival");
    let permissions = PermissionSet::from_entries([
        PermissionEntry::allow(parent.clone()),
        PermissionEntry::deny(creative.clone()),
    ]);

    assert_eq!(
        permissions.resolve_scoped_key(&parent, &creative),
        Some(PermissionState::Deny)
    );
    assert!(permissions.allows_scoped_key(&parent, &survival));
    assert!(!permissions.allows_scoped_key(&parent, &key("steel.command.fly")));
}

#[test]
fn expression_all_and_any_preserve_unset_state() {
    let permissions = PermissionSet::from_entries([
        PermissionEntry::allow(key("steel.allowed")),
        PermissionEntry::deny(key("steel.denied")),
    ]);
    let allowed = PermissionExpr::key(key("steel.allowed"));
    let denied = PermissionExpr::key(key("steel.denied"));
    let unset = PermissionExpr::key(key("steel.unset"));

    assert_eq!(
        permissions.resolve(&(allowed.clone() & unset.clone())),
        None
    );
    assert_eq!(permissions.resolve(&(denied.clone() | unset.clone())), None);
    assert_eq!(
        permissions.resolve(&(allowed.clone() | unset)),
        Some(PermissionState::Allow)
    );
    assert_eq!(
        permissions.resolve(&(allowed & denied)),
        Some(PermissionState::Deny)
    );
}

#[test]
fn deny_wins_a_complete_tie() {
    let give = key("minecraft.command.give");
    let permissions = PermissionSet::from_entries([
        PermissionEntry::allow(give.clone()),
        PermissionEntry::deny(give.clone()),
    ]);

    assert_eq!(permissions.resolve_key(&give), Some(PermissionState::Deny));
}

#[test]
fn source_and_group_priority_break_only_equal_specificity_ties() {
    let admin = key("steel.admin");
    let mut permissions = PermissionSet::new();
    permissions.push_group(PermissionEntry::allow(admin.clone()), "low", 0);
    permissions.push_group(PermissionEntry::deny(admin.clone()), "high", 50);

    let resolution = permissions.resolve_key_detailed(&admin);
    let Some(resolution) = resolution else {
        panic!("group permission should resolve");
    };
    assert_eq!(resolution.state(), PermissionState::Deny);
    assert_eq!(resolution.source().group_name(), Some("high"));
    assert_eq!(resolution.source().group_priority(), Some(50));

    permissions.push(PermissionEntry::allow(admin.clone()));
    let resolution = permissions.resolve_key_detailed(&admin);
    let Some(resolution) = resolution else {
        panic!("subject permission should resolve");
    };
    assert_eq!(resolution.state(), PermissionState::Allow);
    assert_eq!(resolution.source(), &PermissionResolutionSource::Subject);

    let contextual = key("steel.contextual");
    permissions.push(PermissionEntry::allow(contextual.clone()));
    permissions.push_group(
        PermissionEntry::deny_with_context(contextual.clone(), domain_context("lobby")),
        "context",
        -100,
    );
    assert!(!permissions.allows_key_in(&contextual, &world_context("lobby", "spawn")));
}

#[test]
fn set_and_unset_replace_only_exact_rules() {
    let fly = key("steel.fly");
    let lobby = domain_context("lobby");
    let mut permissions = PermissionSet::from_entries([
        PermissionEntry::allow(fly.clone()),
        PermissionEntry::deny_with_context(fly.clone(), lobby.clone()),
    ]);

    permissions.set_in(fly.clone(), lobby.clone(), PermissionState::Allow);
    assert_eq!(permissions.entries().len(), 2);
    assert!(permissions.allows_key_in(&fly, &world_context("lobby", "spawn")));
    assert!(permissions.unset_in(&fly, &lobby));
    assert!(!permissions.unset_in(&fly, &lobby));
    assert_eq!(permissions.entries().len(), 1);
}
