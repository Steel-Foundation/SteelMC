use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{CommandSource, SteelCommandContext, SteelCommandRuntime, literal},
    registration::CommandRegistration,
};
use steel_utils::{Identifier, translations};
use text_components::TextComponent;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("perf"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("perf")
        .then(literal("start").executes(start_profiling))
        .then(literal("stop").executes(stop_profiling))
}

fn start_profiling(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let source = context.source();
    if !source.server().perf.start(source.sender().clone()) {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_PERF_ALREADY_RUNNING,
        )));
    }

    source.send_success(
        &TextComponent::from(&translations::COMMANDS_PERF_STARTED),
        false,
    );
    Ok(0)
}

fn stop_profiling(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let Some(recording) = context.source().server().perf.stop() else {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_PERF_NOT_RUNNING,
        )));
    };

    recording.finish(context.source().server());
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::super::create_dispatcher;
    use crate::command::{
        brigadier::{CommandDispatcher, NodeId},
        execution::{CommandSource, SteelCommandRuntime},
    };
    use steel_registry::init_vanilla_registry;

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
            panic!("child `{name}` should exist");
        };
        child
    }

    #[test]
    fn perf_graph_has_restricted_start_and_stop() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let perf = child(&dispatcher, dispatcher.root(), "perf");
        let Some(root) = dispatcher.node(perf) else {
            panic!("perf root should exist");
        };
        assert!(!root.is_executable());
        assert!(root.is_restricted());

        for name in ["start", "stop"] {
            let node = child(&dispatcher, perf, name);
            let Some(node) = dispatcher.node(node) else {
                panic!("perf subcommand should exist");
            };
            assert!(node.is_executable());
        }

        let Some(children) = dispatcher.children(perf) else {
            panic!("perf children should exist");
        };
        assert_eq!(children.len(), 2);
    }
}
