use steel_utils::Identifier;

use crate::command::execution::CommandPermissionSource;
use crate::command::{
    brigadier::{CommandDispatcher, CommandRequirement, CommandSyntaxError, NodeId},
    execution::{CommandResultCallback, ExecutionCommandSource, SteelCommandRuntime, literal},
};
use crate::permission::{
    PermissionEntry, PermissionExpr, PermissionKey, PermissionSet, PermissionState,
};

use super::{CommandDispatcherBuilder, CommandRegistration, CommandRegistrationError};

struct TestSource {
    callback: CommandResultCallback,
    permission: Option<PermissionState>,
    permission_set: Option<PermissionSet>,
    expected_permission: Option<&'static str>,
    context_allowed: bool,
}

impl ExecutionCommandSource for TestSource {
    fn with_callback(&self, callback: CommandResultCallback) -> Self {
        Self {
            callback,
            permission: self.permission,
            permission_set: self.permission_set.clone(),
            expected_permission: self.expected_permission,
            context_allowed: self.context_allowed,
        }
    }

    fn callback(&self) -> CommandResultCallback {
        self.callback.clone()
    }

    fn handle_error(&self, _error: &CommandSyntaxError, _forked: bool) {}
}

impl CommandPermissionSource for TestSource {
    fn permission_state(&self, permission: &PermissionExpr) -> Option<PermissionState> {
        if let Some(permission_set) = &self.permission_set {
            return permission_set.resolve(permission);
        }
        if let Some(expected) = self.expected_permission {
            let PermissionExpr::Key(permission) = permission else {
                panic!("test command should use one derived permission key");
            };
            assert_eq!(permission.as_str(), expected);
        }
        self.permission
    }
}

type TestDispatcher = CommandDispatcher<TestSource, SteelCommandRuntime>;

fn command(id: Identifier, child: &'static str) -> CommandRegistration<TestSource> {
    let root = id.path.clone();
    CommandRegistration::new(id, move |_| {
        literal(root).then(literal(child).executes(|_| Ok(1)))
    })
}

fn build(
    registrations: impl IntoIterator<Item = CommandRegistration<TestSource>>,
) -> TestDispatcher {
    let mut builder = CommandDispatcherBuilder::new();
    for registration in registrations {
        assert!(builder.register(registration).is_ok());
    }
    let Ok(dispatcher) = builder.build() else {
        panic!("valid command declarations should build");
    };
    dispatcher
}

fn child(dispatcher: &TestDispatcher, parent: NodeId, name: &str) -> NodeId {
    let Some(children) = dispatcher.children(parent) else {
        panic!("parent should belong to dispatcher");
    };
    let Some(child) = children.iter().copied().find(|node| {
        dispatcher
            .node(*node)
            .is_some_and(|node| node.name() == name)
    }) else {
        panic!("child '{name}' should exist");
    };
    child
}

fn root_names(dispatcher: &TestDispatcher) -> Vec<&str> {
    let Some(children) = dispatcher.children(dispatcher.root()) else {
        panic!("dispatcher root should exist");
    };
    children
        .iter()
        .map(|child| {
            let Some(node) = dispatcher.node(*child) else {
                panic!("root child should exist");
            };
            node.name()
        })
        .collect()
}

fn child_names<'a>(dispatcher: &'a TestDispatcher, root: &str) -> Vec<&'a str> {
    let root = child(dispatcher, dispatcher.root(), root);
    let Some(children) = dispatcher.children(root) else {
        panic!("command root should exist");
    };
    children
        .iter()
        .map(|child| {
            let Some(node) = dispatcher.node(*child) else {
                panic!("command child should exist");
            };
            node.name()
        })
        .collect()
}

fn source(permission: Option<PermissionState>, expected_permission: &'static str) -> TestSource {
    TestSource {
        callback: CommandResultCallback::empty(),
        permission,
        permission_set: None,
        expected_permission: Some(expected_permission),
        context_allowed: true,
    }
}

fn permission_source(entries: impl IntoIterator<Item = PermissionEntry>) -> TestSource {
    TestSource {
        callback: CommandResultCallback::empty(),
        permission: None,
        permission_set: Some(PermissionSet::from_entries(entries)),
        expected_permission: None,
        context_allowed: true,
    }
}

fn permission_entry(value: &str, state: PermissionState) -> PermissionEntry {
    let Ok(key) = PermissionKey::parse(value) else {
        panic!("test permission key should parse");
    };
    PermissionEntry::new(key, state)
}

#[test]
fn unique_commands_do_not_pollute_the_root_with_namespaced_variants() {
    let dispatcher = build([
        command(Identifier::new_static("minecraft", "seed"), "first"),
        command(Identifier::new_static("steel", "fly"), "second"),
    ]);

    assert_eq!(root_names(&dispatcher), ["seed", "fly"]);
}

#[test]
fn collisions_keep_the_first_root_and_expose_both_namespaced_commands() {
    let dispatcher = build([
        command(Identifier::new_static("minecraft", "home"), "vanilla"),
        command(Identifier::new_static("example", "home"), "plugin"),
    ]);

    assert_eq!(
        root_names(&dispatcher),
        ["home", "minecraft:home", "example:home"]
    );
    assert_eq!(child_names(&dispatcher, "home"), ["vanilla"]);
    assert_eq!(child_names(&dispatcher, "minecraft:home"), ["vanilla"]);
    assert_eq!(child_names(&dispatcher, "example:home"), ["plugin"]);
}

#[test]
fn alias_collisions_use_the_same_owner_policy() {
    let dispatcher = build([
        command(Identifier::new_static("first", "warp"), "first").alias("home"),
        command(Identifier::new_static("second", "home"), "second"),
    ]);

    assert_eq!(
        root_names(&dispatcher),
        ["warp", "home", "first:warp", "second:home"]
    );
    assert_eq!(child_names(&dispatcher, "home"), ["first"]);
    assert_eq!(child_names(&dispatcher, "second:home"), ["second"]);
}

#[test]
fn duplicate_command_ids_are_rejected_without_replacing_the_first() {
    let mut builder = CommandDispatcherBuilder::new();
    assert!(
        builder
            .register(command(Identifier::new_static("example", "home"), "first"))
            .is_ok()
    );
    let error = builder.register(command(Identifier::new_static("example", "home"), "second"));

    assert!(matches!(
        error,
        Err(CommandRegistrationError::DuplicateCommandId(id))
            if id == Identifier::new_static("example", "home")
    ));
    let Ok(dispatcher) = builder.build() else {
        panic!("the first declaration should remain buildable");
    };
    assert_eq!(child_names(&dispatcher, "home"), ["first"]);
}

#[test]
fn aliases_cannot_duplicate_another_root_owned_by_the_same_command() {
    let mut builder = CommandDispatcherBuilder::new();
    let error =
        builder.register(command(Identifier::new_static("example", "home"), "child").alias("home"));

    assert!(matches!(
        error,
        Err(CommandRegistrationError::DuplicateOwnedRoot { id, root })
            if id == Identifier::new_static("example", "home") && root.as_ref() == "home"
    ));
}

#[test]
fn namespaced_aliases_are_reserved_for_collision_fallbacks() {
    let mut builder = CommandDispatcherBuilder::new();
    let error = builder
        .register(command(Identifier::new_static("example", "home"), "child").alias("other:home"));

    assert!(matches!(
        error,
        Err(CommandRegistrationError::NamespacedAlias(alias))
            if alias.as_ref() == "other:home"
    ));
}

#[test]
fn command_root_must_match_its_stable_id_path() {
    let mut builder = CommandDispatcherBuilder::<TestSource>::new();
    assert!(
        builder
            .register(CommandRegistration::new(
                Identifier::new_static("example", "home"),
                |_| literal("warp")
            ))
            .is_ok()
    );

    assert!(matches!(
        builder.build(),
        Err(CommandRegistrationError::RootDoesNotMatchId { id, root })
            if id == Identifier::new_static("example", "home") && root.as_ref() == "warp"
    ));
}

#[test]
fn factories_receive_the_built_dispatcher_root_for_redirects() {
    let mut builder = CommandDispatcherBuilder::new();
    assert!(
        builder
            .register(CommandRegistration::new(
                Identifier::new_static("example", "forward"),
                |root| literal("forward").redirects(root)
            ))
            .is_ok()
    );
    let Ok(dispatcher) = builder.build() else {
        panic!("redirect to the built dispatcher's root should be valid");
    };
    let forward = child(&dispatcher, dispatcher.root(), "forward");

    assert_eq!(
        dispatcher.node(forward).and_then(|node| node.redirect()),
        Some(dispatcher.root())
    );
}

#[test]
fn required_root_permissions_are_derived_from_stable_command_ids() {
    let dispatcher = build([command(
        Identifier::new_static("minecraft", "seed"),
        "child",
    )]);
    let seed = child(&dispatcher, dispatcher.root(), "seed");
    let Some(seed) = dispatcher.node(seed) else {
        panic!("seed root should exist");
    };

    assert!(!seed.allows(&source(None, "minecraft.command.seed")));
    assert!(seed.allows(&source(
        Some(PermissionState::Allow),
        "minecraft.command.seed"
    )));
    assert!(!seed.allows(&source(
        Some(PermissionState::Deny),
        "minecraft.command.seed"
    )));
    assert!(seed.is_restricted());
}

#[test]
fn default_access_allows_unset_but_not_explicitly_denied_permissions() {
    let dispatcher =
        build([command(Identifier::new_static("minecraft", "list"), "child").default_access()]);
    let list = child(&dispatcher, dispatcher.root(), "list");
    let Some(list) = dispatcher.node(list) else {
        panic!("list root should exist");
    };

    assert!(list.allows(&source(None, "minecraft.command.list")));
    assert!(!list.allows(&source(
        Some(PermissionState::Deny),
        "minecraft.command.list"
    )));
}

#[test]
fn explicit_permission_expressions_replace_id_derivation() {
    let permission = crate::permission::PermissionKey::parse("steel.command.inspect");
    let Ok(permission) = permission else {
        panic!("test permission should be valid");
    };
    let dispatcher = build([
        command(Identifier::new_static("example", "inspect"), "child")
            .permission(PermissionExpr::key(permission)),
    ]);
    let inspect = child(&dispatcher, dispatcher.root(), "inspect");
    let Some(inspect) = dispatcher.node(inspect) else {
        panic!("inspect root should exist");
    };

    assert!(inspect.allows(&source(
        Some(PermissionState::Allow),
        "steel.command.inspect"
    )));
}

#[test]
fn registration_composes_authorization_with_existing_context_requirements() {
    let mut builder = CommandDispatcherBuilder::new();
    let registration =
        CommandRegistration::new(Identifier::new_static("example", "inspect"), |_| {
            literal("inspect").requires(CommandRequirement::contextual(|source: &TestSource| {
                source.context_allowed
            }))
        });
    assert!(builder.register(registration).is_ok());
    let Ok(dispatcher) = builder.build() else {
        panic!("composed requirements should build");
    };
    let inspect = child(&dispatcher, dispatcher.root(), "inspect");
    let Some(inspect) = dispatcher.node(inspect) else {
        panic!("inspect root should exist");
    };
    let mut allowed = source(Some(PermissionState::Allow), "example.command.inspect");

    allowed.context_allowed = false;
    assert!(!inspect.allows(&allowed));
    allowed.context_allowed = true;
    assert!(inspect.allows(&allowed));
    assert!(inspect.is_restricted());
}

#[test]
fn derived_subcommand_permissions_are_alternatives_to_the_root() {
    let registration =
        CommandRegistration::new(Identifier::new_static("minecraft", "tick"), |_| {
            literal("tick")
                .then(literal("query").executes(|_| Ok(1)))
                .then(literal("rate").executes(|_| Ok(1)))
                .then(literal("freeze").executes(|_| Ok(1)))
        })
        .subcommand_permission(["rate"])
        .subcommand_permission(["freeze"]);
    let dispatcher = build([registration]);
    let tick = child(&dispatcher, dispatcher.root(), "tick");
    let query = child(&dispatcher, tick, "query");
    let rate = child(&dispatcher, tick, "rate");
    let freeze = child(&dispatcher, tick, "freeze");

    let rate_only = permission_source([permission_entry(
        "minecraft.command.tick.rate",
        PermissionState::Allow,
    )]);
    assert!(
        dispatcher
            .node(tick)
            .is_some_and(|node| node.allows(&rate_only))
    );
    assert!(
        dispatcher
            .node(query)
            .is_some_and(|node| node.allows(&rate_only))
    );
    assert!(
        dispatcher
            .node(rate)
            .is_some_and(|node| node.allows(&rate_only) && node.is_restricted())
    );
    assert!(
        dispatcher
            .node(freeze)
            .is_some_and(|node| !node.allows(&rate_only))
    );

    let root = permission_source([permission_entry(
        "minecraft.command.tick",
        PermissionState::Allow,
    )]);
    assert!(dispatcher.node(rate).is_some_and(|node| node.allows(&root)));
    assert!(
        dispatcher
            .node(freeze)
            .is_some_and(|node| node.allows(&root))
    );

    let root_with_freeze_deny = permission_source([
        permission_entry("minecraft.command.tick", PermissionState::Allow),
        permission_entry("minecraft.command.tick.freeze", PermissionState::Deny),
    ]);
    assert!(
        dispatcher
            .node(rate)
            .is_some_and(|node| node.allows(&root_with_freeze_deny))
    );
    assert!(
        dispatcher
            .node(freeze)
            .is_some_and(|node| !node.allows(&root_with_freeze_deny))
    );
}

#[test]
fn default_access_still_honors_specific_subcommand_denies() {
    let registration =
        CommandRegistration::new(Identifier::new_static("minecraft", "inspect"), |_| {
            literal("inspect")
                .then(literal("query").executes(|_| Ok(1)))
                .then(literal("write").executes(|_| Ok(1)))
        })
        .default_access()
        .subcommand_permission(["write"]);
    let dispatcher = build([registration]);
    let inspect = child(&dispatcher, dispatcher.root(), "inspect");
    let write = child(&dispatcher, inspect, "write");

    let unset = permission_source([]);
    assert!(
        dispatcher
            .node(inspect)
            .is_some_and(|node| node.allows(&unset))
    );
    assert!(
        dispatcher
            .node(write)
            .is_some_and(|node| !node.allows(&unset))
    );

    let denied_root_with_write = permission_source([
        permission_entry("minecraft.command.inspect", PermissionState::Deny),
        permission_entry("minecraft.command.inspect.write", PermissionState::Allow),
    ]);
    assert!(
        dispatcher
            .node(inspect)
            .is_some_and(|node| node.allows(&denied_root_with_write))
    );
    assert!(
        dispatcher
            .node(write)
            .is_some_and(|node| node.allows(&denied_root_with_write))
    );

    let denied_write = permission_source([permission_entry(
        "minecraft.command.inspect.write",
        PermissionState::Deny,
    )]);
    assert!(
        dispatcher
            .node(inspect)
            .is_some_and(|node| node.allows(&denied_write))
    );
    assert!(
        dispatcher
            .node(write)
            .is_some_and(|node| !node.allows(&denied_write))
    );
}

#[test]
fn missing_subcommand_permission_paths_fail_the_build() {
    let mut builder = CommandDispatcherBuilder::new();
    let registration = command(Identifier::new_static("minecraft", "tick"), "query")
        .subcommand_permission(["freeze"]);
    assert!(builder.register(registration).is_ok());

    assert!(matches!(
        builder.build(),
        Err(CommandRegistrationError::MissingSubcommandPermissionPath { id, path })
            if id == Identifier::new_static("minecraft", "tick") && path == "freeze"
    ));
}

#[test]
fn explicit_root_permissions_cannot_mix_with_derived_subcommands() {
    let Ok(permission) = PermissionKey::parse("steel.command.tick") else {
        panic!("test permission should parse");
    };
    let registration = command(Identifier::new_static("minecraft", "tick"), "freeze")
        .permission(PermissionExpr::key(permission))
        .subcommand_permission(["freeze"]);
    let mut builder = CommandDispatcherBuilder::new();

    assert!(matches!(
        builder.register(registration),
        Err(CommandRegistrationError::SubcommandPermissionsRequireDerivedRoot { id })
            if id == Identifier::new_static("minecraft", "tick")
    ));
}
