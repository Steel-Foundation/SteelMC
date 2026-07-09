use steel_utils::Identifier;

use crate::command::{
    brigadier::{CommandDispatcher, CommandSyntaxError, NodeId},
    execution::{CommandResultCallback, ExecutionCommandSource, SteelCommandRuntime, literal},
};

use super::{CommandDispatcherBuilder, CommandRegistration, CommandRegistrationError};

struct TestSource {
    callback: CommandResultCallback,
}

impl ExecutionCommandSource for TestSource {
    fn with_callback(&self, callback: CommandResultCallback) -> Self {
        Self { callback }
    }

    fn callback(&self) -> CommandResultCallback {
        self.callback.clone()
    }

    fn handle_error(&self, _error: &CommandSyntaxError, _forked: bool) {}
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
