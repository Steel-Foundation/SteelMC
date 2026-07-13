use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::{
    ArgumentType, CommandDispatcher, CommandNodeBuilder, CommandRedirectTarget, CommandRequirement,
    NodeId, NodeKind, RegistrationError, RegistrationErrorKind, argument, literal,
    node::CommandNode,
};

#[derive(Debug)]
struct TestSource {
    allowed: bool,
}

fn register(
    dispatcher: &mut CommandDispatcher<TestSource>,
    builder: CommandNodeBuilder<TestSource>,
) -> NodeId {
    let Ok(node) = dispatcher.register(builder) else {
        panic!("command registration should succeed");
    };
    node
}

fn node_names(dispatcher: &CommandDispatcher<TestSource>, parent: NodeId) -> Vec<&str> {
    let Some(children) = dispatcher.children(parent) else {
        panic!("parent node should belong to the dispatcher");
    };
    children
        .iter()
        .map(|child| {
            let Some(node) = dispatcher.node(*child) else {
                panic!("child node should belong to the dispatcher");
            };
            node.name()
        })
        .collect()
}

fn assert_registration_error(
    result: Result<NodeId, RegistrationError>,
    expected: RegistrationErrorKind,
) {
    let Err(error) = result else {
        panic!("command registration should fail");
    };
    assert_eq!(error.kind(), &expected);
}

#[test]
fn registration_preserves_child_order_and_returns_stable_ids() {
    let mut dispatcher = CommandDispatcher::<TestSource>::new();
    let root = dispatcher.root();
    let first = register(&mut dispatcher, literal("first"));
    let second = register(&mut dispatcher, literal("second"));

    assert_ne!(first, second);
    assert_eq!(node_names(&dispatcher, root), ["first", "second"]);
    assert_eq!(dispatcher.node(first).map(CommandNode::name), Some("first"));
    assert_eq!(
        dispatcher.node(second).map(CommandNode::name),
        Some("second")
    );
}

#[test]
fn compatible_literal_registration_merges_grandchildren() {
    let mut dispatcher = CommandDispatcher::<TestSource>::new();
    let first_base = register(
        &mut dispatcher,
        literal("base").then(literal("first").executes(|_| Ok(1))),
    );
    let second_base = register(
        &mut dispatcher,
        literal("base").then(literal("second").executes(|_| Ok(2))),
    );

    assert_eq!(first_base, second_base);
    assert_eq!(node_names(&dispatcher, first_base), ["first", "second"]);
}

#[test]
fn compatible_registration_preserves_or_replaces_command() {
    let mut dispatcher = CommandDispatcher::<TestSource>::new();
    let node_id = register(
        &mut dispatcher,
        literal("base").then(literal("child").executes(|_| Ok(1))),
    );
    register(&mut dispatcher, literal("base").then(literal("child")));

    let Some(child_id) = dispatcher
        .children(node_id)
        .and_then(|children| children.first().copied())
    else {
        panic!("merged child should exist");
    };
    assert_eq!(
        dispatcher.execute_node_for_test(child_id, TestSource { allowed: true }),
        Some(Ok(1))
    );
    assert_eq!(
        dispatcher.node(child_id).map(CommandNode::is_executable),
        Some(true)
    );

    register(
        &mut dispatcher,
        literal("base").then(literal("child").executes(|_| Ok(2))),
    );
    assert_eq!(
        dispatcher.node(child_id).map(CommandNode::is_executable),
        Some(true)
    );
    assert_eq!(
        dispatcher.execute_node_for_test(child_id, TestSource { allowed: true }),
        Some(Ok(2))
    );
}

#[test]
fn duplicate_children_inside_one_builder_are_normalized() {
    let mut dispatcher = CommandDispatcher::<TestSource>::new();
    let base = register(
        &mut dispatcher,
        literal("base")
            .then(literal("child").then(literal("first")))
            .then(literal("child").then(literal("second"))),
    );

    assert_eq!(node_names(&dispatcher, base), ["child"]);
    let Some(child) = dispatcher
        .children(base)
        .and_then(|children| children.first())
    else {
        panic!("normalized child should exist");
    };
    assert_eq!(node_names(&dispatcher, *child), ["first", "second"]);
}

#[test]
fn shared_requirement_identity_can_merge() {
    let mut dispatcher = CommandDispatcher::<TestSource>::new();
    let requirement = CommandRequirement::contextual(|source: &TestSource| source.allowed);
    let first = register(
        &mut dispatcher,
        literal("secure")
            .requires(requirement.clone())
            .then(literal("first")),
    );
    let second = register(
        &mut dispatcher,
        literal("secure")
            .requires(requirement)
            .then(literal("second")),
    );

    assert_eq!(first, second);
    assert_eq!(node_names(&dispatcher, first), ["first", "second"]);
}

#[test]
fn incompatible_requirement_collision_is_atomic() {
    let mut dispatcher = CommandDispatcher::<TestSource>::new();
    let secure = register(
        &mut dispatcher,
        literal("secure")
            .requires(CommandRequirement::contextual(|source: &TestSource| {
                source.allowed
            }))
            .then(literal("existing")),
    );
    let node_count = dispatcher.node_count();

    assert_registration_error(
        dispatcher.register(
            literal("secure")
                .requires(CommandRequirement::contextual(|_: &TestSource| true))
                .then(literal("new")),
        ),
        RegistrationErrorKind::RequirementCollision {
            name: "secure".into(),
        },
    );

    assert_eq!(dispatcher.node_count(), node_count);
    assert_eq!(node_names(&dispatcher, secure), ["existing"]);
}

#[test]
fn incompatible_argument_collision_is_atomic() {
    let mut dispatcher = CommandDispatcher::<TestSource>::new();
    let base = register(
        &mut dispatcher,
        literal("base").then(argument("value", ArgumentType::integer(0, 10))),
    );
    let node_count = dispatcher.node_count();

    assert_registration_error(
        dispatcher.register(literal("base").then(argument("value", ArgumentType::integer(0, 20)))),
        RegistrationErrorKind::ArgumentTypeCollision {
            name: "value".into(),
        },
    );

    assert_eq!(dispatcher.node_count(), node_count);
    assert_eq!(node_names(&dispatcher, base), ["value"]);
}

#[test]
fn literal_and_argument_with_the_same_name_collide() {
    let mut dispatcher = CommandDispatcher::<TestSource>::new();
    let base = register(&mut dispatcher, literal("base").then(literal("value")));

    assert_registration_error(
        dispatcher.register(literal("base").then(argument("value", ArgumentType::bool()))),
        RegistrationErrorKind::NodeKindCollision {
            name: "value".into(),
            existing: NodeKind::Literal,
            incoming: NodeKind::Argument,
        },
    );
    assert_eq!(node_names(&dispatcher, base), ["value"]);
}

#[test]
fn incompatible_redirect_collision_is_atomic() {
    let mut dispatcher = CommandDispatcher::<TestSource>::new();
    let first_target = register(&mut dispatcher, literal("first"));
    let second_target = register(&mut dispatcher, literal("second"));
    let alias = register(&mut dispatcher, literal("alias").redirects(first_target));
    let node_count = dispatcher.node_count();

    assert_registration_error(
        dispatcher.register(literal("alias").redirects(second_target)),
        RegistrationErrorKind::RedirectCollision {
            name: "alias".into(),
        },
    );

    assert_eq!(dispatcher.node_count(), node_count);
    assert_eq!(
        dispatcher.node(alias).and_then(CommandNode::redirect),
        Some(first_target)
    );
}

#[test]
fn redirect_targets_must_belong_to_the_dispatcher() {
    let first_dispatcher = CommandDispatcher::<TestSource>::new();
    let foreign_target = first_dispatcher.root();
    let mut second_dispatcher = CommandDispatcher::<TestSource>::new();

    assert_registration_error(
        second_dispatcher.register(literal("alias").redirects(foreign_target)),
        RegistrationErrorKind::InvalidRedirectTarget {
            target: foreign_target,
        },
    );
    assert!(node_names(&second_dispatcher, second_dispatcher.root()).is_empty());
}

#[test]
fn redirected_nodes_cannot_have_children() {
    let mut dispatcher = CommandDispatcher::<TestSource>::new();
    let root = dispatcher.root();

    assert_registration_error(
        dispatcher.register(literal("alias").redirects(root).then(literal("child"))),
        RegistrationErrorKind::RedirectWithChildren {
            name: "alias".into(),
        },
    );
    assert!(node_names(&dispatcher, root).is_empty());
}

#[test]
fn symbolic_command_root_redirect_resolves_to_the_registered_root() {
    let mut dispatcher = CommandDispatcher::<TestSource>::new();
    let command = register(
        &mut dispatcher,
        literal("execute").then(literal("again").redirects(CommandRedirectTarget::CommandRoot)),
    );
    let Some(again) = dispatcher
        .children(command)
        .and_then(|children| children.first().copied())
    else {
        panic!("redirect child should exist");
    };

    assert_eq!(
        dispatcher.node(again).and_then(CommandNode::redirect),
        Some(command)
    );
}

#[test]
fn command_roots_must_be_literals() {
    let mut dispatcher = CommandDispatcher::<TestSource>::new();

    assert_registration_error(
        dispatcher.register(argument("value", ArgumentType::bool())),
        RegistrationErrorKind::ArgumentRoot,
    );
}

#[test]
fn requirements_remain_generic_source_predicates() {
    let requirement = CommandRequirement::contextual(|source: &TestSource| source.allowed);

    assert!(requirement.allows(&TestSource { allowed: true }));
    assert!(!requirement.allows(&TestSource { allowed: false }));
    assert!(CommandRequirement::<TestSource>::allow_all().allows(&TestSource { allowed: false }));
}

#[test]
fn command_callbacks_receive_the_generic_source_context() {
    let observed = Arc::new(AtomicBool::new(false));
    let command_observed = Arc::clone(&observed);
    let builder = literal::<TestSource>("test").executes(move |context| {
        command_observed.store(context.source().allowed, Ordering::Relaxed);
        Ok(1)
    });
    let mut dispatcher = CommandDispatcher::new();
    let command = register(&mut dispatcher, builder);

    assert_eq!(
        dispatcher.execute_node_for_test(command, TestSource { allowed: true }),
        Some(Ok(1))
    );
    assert!(observed.load(Ordering::Relaxed));
}
