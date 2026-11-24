//! Vanilla command to give an effect to an entity.

use super::super::{
    brigadier::{ArgumentType, CommandNodeBuilder},
    execution::{CommandSource, SteelArgumentType, SteelCommandRuntime, literal},
    registration::CommandRegistration,
};
use crate::command::execution::argument;
use steel_utils::Identifier;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("effect"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("effect")
        .then(
            literal("clear").then(
                argument("targets", SteelArgumentType::entities())
                    .then(argument("effect", SteelArgumentType::mob_effect())),
            ),
        )
        .then(
            literal("give").then(
                argument("targets", SteelArgumentType::entities()).then(
                    argument("effect", SteelArgumentType::mob_effect())
                        .then(
                            argument("seconds", ArgumentType::integer(1, 1000000)).then(
                                argument("amplifier", ArgumentType::integer(0, 255))
                                    .then(argument("hideParticles", ArgumentType::bool())),
                            ),
                        )
                        .then(
                            literal("infinite").then(
                                argument("amplifier", ArgumentType::integer(0, 255))
                                    .then(argument("hideParticles", ArgumentType::bool())),
                            ),
                        ),
                ),
            ),
        )
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use super::super::create_dispatcher;
    use super::*;
    use crate::command::brigadier::{CommandDispatcher, NodeId};

    type Dispatcher = CommandDispatcher<CommandSource, SteelCommandRuntime>;

    fn child(dispatcher: &Dispatcher, parent: NodeId, name: &str) -> NodeId {
        let Some(children) = dispatcher.children(parent) else {
            panic!("parent node should exist");
        };
        let Some(child) = children.iter().copied().find(|child| {
            dispatcher
                .node(*child)
                .is_some_and(|node| node.name() == name)
        }) else {
            panic!("child {name} should exist");
        };
        child
    }

    #[test]
    fn give_graph_matches_vanilla_argument_shape() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let give = child(&dispatcher, dispatcher.root(), "effect");
        let Some(give_node) = dispatcher.node(give) else {
            panic!("give node should exist");
        };
        assert!(give_node.is_restricted());

        effect_clear_node(&dispatcher, give);

        let targets = child(&dispatcher, give, "targets");
        assert_eq!(
            dispatcher
                .node(targets)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::players())
        );
        assert!(matches!(
            dispatcher.node(targets),
            Some(node) if node.is_executable()
        ));

        let item = child(&dispatcher, targets, "item");
        assert_eq!(
            dispatcher.node(item).and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::item_predicate())
        );
        assert!(matches!(
            dispatcher.node(item),
            Some(node) if node.is_executable()
        ));

        let max_count = child(&dispatcher, item, "maxCount");
        assert_eq!(
            dispatcher
                .node(max_count)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(ArgumentType::integer(0, i32::MAX)))
        );
        assert!(matches!(
            dispatcher.node(max_count),
            Some(node) if node.is_executable()
        ));
    }

    fn effect_clear_node(
        dispatcher: &CommandDispatcher<CommandSource, SteelCommandRuntime>,
        give_node: NodeId,
    ) {
        let clear = child(&dispatcher, give_node, "clear");

        let targets = child(&dispatcher, clear, "targets");
        assert_eq!(
            dispatcher
                .node(targets)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(SteelArgumentType::entities()))
        );
        assert!(matches!(
            dispatcher.node(targets),
            Some(node) if node.is_executable()
        ));

        let effect = child(&dispatcher, targets, "effect");
        assert_eq!(
            dispatcher
                .node(effect)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(SteelArgumentType::mob_effect()))
        );
        assert!(matches!(
            dispatcher.node(effect),
            Some(node) if node.is_executable()
        ));
    }

    fn effect_give_node(
        dispatcher: &CommandDispatcher<CommandSource, SteelCommandRuntime>,
        give_node: NodeId,
    ) {
        let give = child(&dispatcher, give_node, "give");

        let targets = child(&dispatcher, give, "targets");
        assert_eq!(
            dispatcher
                .node(targets)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(SteelArgumentType::entities()))
        );
        assert!(matches!(
            dispatcher.node(targets),
            Some(node) if node.is_executable()
        ));

        let effect = child(&dispatcher, targets, "effect");
        assert_eq!(
            dispatcher
                .node(effect)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(SteelArgumentType::mob_effect()))
        );
        assert!(matches!(
            dispatcher.node(effect),
            Some(node) if node.is_executable()
        ));

        let infinite = child(&dispatcher, effect, "infinite");
        assert!(matches!(
            dispatcher.node(infinite),
            Some(node) if node.is_executable()
        ));
        effect_give_infinite_or_second_node(dispatcher, infinite);

        let seconds = child(&dispatcher, effect, "seconds");
        assert_eq!(
            dispatcher
                .node(seconds)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(ArgumentType::integer(1, 1000000)))
        );
        assert!(matches!(
            dispatcher.node(effect),
            Some(node) if node.is_executable()
        ));
        effect_give_infinite_or_second_node(dispatcher, seconds);
    }

    fn effect_give_infinite_or_second_node(
        dispatcher: &CommandDispatcher<CommandSource, SteelCommandRuntime>,
        parent_node: NodeId,
    ) {
        let amplifier = child(&dispatcher, parent_node, "amplifier");
        assert_eq!(
            dispatcher
                .node(amplifier)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(ArgumentType::integer(0, 255)))
        );
        assert!(matches!(
            dispatcher.node(amplifier),
            Some(node) if node.is_executable()
        ));

        let hide_particles = child(&dispatcher, amplifier, "hideParticles");
        assert_eq!(
            dispatcher
                .node(hide_particles)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(ArgumentType::bool()))
        );
        assert!(matches!(
            dispatcher.node(hide_particles),
            Some(node) if node.is_executable()
        ));
    }
}
