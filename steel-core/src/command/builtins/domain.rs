//! Steel domain switch command.

use std::sync::Arc;

use steel_utils::Identifier;
use text_components::TextComponent;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::from_steel("domain"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("domain").then(argument("domain", SteelArgumentType::domain()).executes(switch_domain))
}

fn switch_domain(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let source = context.source();
    let Some(player) = source.player() else {
        return Err(CommandSyntaxError::dynamic(
            "This command can only be used by a player",
        ));
    };
    let Some(domain) = context.domain("domain") else {
        return Err(CommandSyntaxError::dynamic(
            "Parsed domain is missing from the command context",
        ));
    };
    source
        .server()
        .queue_domain_switch(Arc::clone(player), domain.to_owned())
        .map_err(CommandSyntaxError::dynamic)?;

    source.send_success(
        &TextComponent::plain(format!("Switching to domain {domain}")),
        true,
    );
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::super::create_dispatcher;
    use crate::command::{
        brigadier::{CommandDispatcher, NodeId},
        execution::{CommandSource, SteelArgumentType, SteelCommandRuntime},
    };
    use steel_registry::test_support::init_test_registry;

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
    fn domain_graph_uses_the_configured_domain_argument() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let root = child(&dispatcher, dispatcher.root(), "domain");
        let domain = child(&dispatcher, root, "domain");
        assert_eq!(
            dispatcher
                .node(domain)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::domain())
        );
        let Some(domain) = dispatcher.node(domain) else {
            panic!("domain argument should exist");
        };
        assert!(domain.is_executable());
    }
}
