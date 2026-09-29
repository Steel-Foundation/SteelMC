//! Vanilla private-message command (`/msg`, `/tell`, `/w`).

use steel_utils::{Identifier, translations};
use text_components::{Modifier, TextComponent, format::Color};

use crate::entity::Entity;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("msg"), |_| command())
        .alias("tell")
        .alias("w")
        .default_access()
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("msg").then(
        argument("targets", SteelArgumentType::players())
            .then(argument("message", SteelArgumentType::message()).executes(send_message)),
    )
}

fn send_message(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    let body = context.message("message")?;
    let sender_name = context.source().entity().map_or_else(
        || TextComponent::plain(context.source().sender().to_string()),
        |entity| entity.display_name(),
    );

    for target in &targets {
        let incoming = translations::COMMANDS_MESSAGE_DISPLAY_INCOMING
            .message([sender_name.clone(), body.clone()])
            .component()
            .color(Color::Gray)
            .italic(true);
        target.send_message(&incoming);

        let outgoing = translations::COMMANDS_MESSAGE_DISPLAY_OUTGOING
            .message([target.display_name(), body.clone()])
            .component()
            .color(Color::Gray)
            .italic(true);
        context.source().send_system_message(&outgoing);
    }

    i32::try_from(targets.len())
        .map_err(|_| CommandSyntaxError::dynamic("Target count exceeds the command result range"))
}

#[cfg(test)]
mod tests {
    use steel_registry::init_vanilla_registry;

    use super::super::create_dispatcher;
    use crate::command::execution::SteelArgumentType;

    #[test]
    fn msg_graph_requires_a_player_target_and_a_message_and_registers_its_aliases() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let roots = ["msg", "tell", "w"].map(|name| {
            dispatcher
                .children(dispatcher.root())
                .and_then(|children| {
                    children.iter().copied().find(|child| {
                        dispatcher
                            .node(*child)
                            .is_some_and(|node| node.name() == name)
                    })
                })
                .unwrap_or_else(|| panic!("{name} root should exist"))
        });
        let [msg, ..] = roots;

        let Some(targets) = dispatcher
            .children(msg)
            .and_then(|children| children.first())
            .copied()
        else {
            panic!("msg targets should exist");
        };
        assert!(matches!(
            dispatcher.node(targets),
            Some(node)
                if !node.is_executable()
                    && node.argument_type() == Some(&SteelArgumentType::players())
        ));

        let Some(message) = dispatcher
            .children(targets)
            .and_then(|children| children.first())
        else {
            panic!("msg message should exist");
        };
        assert!(matches!(
            dispatcher.node(*message),
            Some(node)
                if node.is_executable()
                    && node.argument_type() == Some(&SteelArgumentType::message())
        ));
    }
}
