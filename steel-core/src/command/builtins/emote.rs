//! Vanilla emote command.

use super::super::{
    brigadier::CommandNodeBuilder,
    execution::{CommandSource, SteelArgumentType, SteelCommandRuntime, argument, literal},
    registration::CommandRegistration,
};
use crate::player::chat::OutgoingChatMessage;
use steel_registry::{RegistryEntry, vanilla_chat_types};
use steel_utils::Identifier;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("me"), |_| command()).default_access()
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("me").then(
        argument("action", SteelArgumentType::message()).executes(|ctx| {
            let message = ctx.message("action")?.resolve_component(ctx.source())?;
            let chat_type = ctx
                .source()
                .bind_chat_type(vanilla_chat_types::EMOTE_COMMAND.id() as i32);
            let outgoing = OutgoingChatMessage::from_command(ctx.source(), "action", message);

            ctx.source().server().broadcast_chat(&outgoing, &chat_type);
            Ok(1)
        }),
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
    fn clear_graph_matches_vanilla_argument_shape() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let me = child(&dispatcher, dispatcher.root(), "me");
        let Some(_me_node) = dispatcher.node(me) else {
            panic!("clear node should exist");
        };

        let action = child(&dispatcher, me, "action");
        assert_eq!(
            dispatcher
                .node(action)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::message())
        );
        assert!(matches!(
            dispatcher.node(action),
            Some(node) if node.is_executable()
        ));
    }
}
