//! Vanilla entity scoreboard tag command.

use std::collections::BTreeSet;

use steel_utils::{Identifier, translations};
use text_components::TextComponent;

use super::super::{
    brigadier::{ArgumentType, CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("tag"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("tag").then(
        argument("targets", SteelArgumentType::entities())
            .then(literal("add").then(argument("name", ArgumentType::word()).executes(add_tag)))
            .then(
                literal("remove").then(argument("name", ArgumentType::word()).executes(remove_tag)),
            )
            .then(literal("list").executes(list_tags)),
    )
}

fn add_tag(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let targets = context.entities("targets")?;
    let tag = context.string("name")?;

    let changed = targets
        .iter()
        .filter(|target| target.add_tag(tag.to_owned()))
        .count();

    if changed == 0 {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_TAG_ADD_FAILED,
        )));
    }

    let message = if let [target] = targets.as_slice() {
        translations::COMMANDS_TAG_ADD_SUCCESS_SINGLE
            .message([
                TextComponent::plain(tag.to_owned()),
                TextComponent::plain(target.plain_text_name()),
            ])
            .component()
    } else {
        translations::COMMANDS_TAG_ADD_SUCCESS_MULTIPLE
            .message([
                TextComponent::plain(tag.to_owned()),
                TextComponent::plain(changed.to_string()),
            ])
            .component()
    };

    context.source().send_success(&message, true);

    Ok(changed.min(i32::MAX as usize) as i32)
}

fn remove_tag(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let targets = context.entities("targets")?;
    let tag = context.string("name")?;

    let changed = targets
        .iter()
        .filter(|target| target.remove_tag(tag))
        .count();

    if changed == 0 {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_TAG_REMOVE_FAILED,
        )));
    }

    let message = if let [target] = targets.as_slice() {
        translations::COMMANDS_TAG_REMOVE_SUCCESS_SINGLE
            .message([
                TextComponent::plain(tag.to_owned()),
                TextComponent::plain(target.plain_text_name()),
            ])
            .component()
    } else {
        translations::COMMANDS_TAG_REMOVE_SUCCESS_MULTIPLE
            .message([
                TextComponent::plain(tag.to_owned()),
                TextComponent::plain(changed.to_string()),
            ])
            .component()
    };

    context.source().send_success(&message, true);

    Ok(changed.min(i32::MAX as usize) as i32)
}

fn list_tags(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let targets = context.entities("targets")?;

    if let [target] = targets.as_slice() {
        let tags = target.tags();

        if tags.is_empty() {
            return Err(CommandSyntaxError::dynamic(
                translations::COMMANDS_TAG_LIST_SINGLE_EMPTY
                    .message([TextComponent::plain(target.plain_text_name())])
                    .component(),
            ));
        }

        let message = translations::COMMANDS_TAG_LIST_SINGLE_SUCCESS
            .message([
                TextComponent::plain(target.plain_text_name()),
                TextComponent::plain(tags.len().to_string()),
                TextComponent::plain(tags.join(", ")),
            ])
            .component();

        context.source().send_success(&message, true);

        return Ok(tags.len().min(i32::MAX as usize) as i32);
    }

    let all_tags = targets
        .iter()
        .flat_map(|target| target.tags())
        .collect::<BTreeSet<_>>();

    if all_tags.is_empty() {
        let message = translations::COMMANDS_TAG_LIST_MULTIPLE_EMPTY
            .message([TextComponent::plain(targets.len().to_string())])
            .component();

        return Err(CommandSyntaxError::dynamic(message));
    }

    let tag_count = all_tags.len();

    let message = translations::COMMANDS_TAG_LIST_MULTIPLE_SUCCESS
        .message([
            TextComponent::plain(targets.len().to_string()),
            TextComponent::plain(tag_count.to_string()),
            TextComponent::plain(all_tags.iter().cloned().collect::<Vec<_>>().join(", ")),
        ])
        .component();

    context.source().send_success(&message, true);

    Ok(tag_count.min(i32::MAX as usize) as i32)
}

#[cfg(test)]
mod tests {
    use steel_protocol::packets::game::ArgumentType as ProtocolArgumentType;
    use steel_registry::init_vanilla_registry;

    use super::super::create_dispatcher;
    use super::*;

    #[test]
    fn tag_graph_matches_vanilla_argument_shape() {
        init_vanilla_registry();

        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };

        let Some(tag) = dispatcher.children(dispatcher.root()).and_then(|children| {
            children.iter().copied().find(|child| {
                dispatcher
                    .node(*child)
                    .is_some_and(|node| node.name() == "tag")
            })
        }) else {
            panic!("tag root should exist");
        };

        let target_id = dispatcher
            .children(tag)
            .and_then(|children| children.first())
            .copied()
            .expect("tag targets should exist");

        let Some(targets) = dispatcher.node(target_id) else {
            panic!("tag targets should exist");
        };

        assert_eq!(
            targets.argument_type(),
            Some(&SteelArgumentType::entities())
        );
        assert!(!targets.is_executable());

        let children = dispatcher
            .children(target_id)
            .expect("tag targets children");

        assert_eq!(children.len(), 3);

        for child in children {
            let node = dispatcher
                .node(*child)
                .expect("tag subcommand should exist");

            assert!(matches!(node.name(), "add" | "remove" | "list"));
        }

        for operation in ["add", "remove"] {
            let operation = children
                .iter()
                .copied()
                .find(|child| {
                    dispatcher
                        .node(*child)
                        .is_some_and(|node| node.name() == operation)
                })
                .expect("tag operation should exist");

            let name = dispatcher
                .children(operation)
                .and_then(|children| children.first())
                .and_then(|child| dispatcher.node(*child))
                .expect("tag name should exist");

            assert!(name.is_executable());

            assert!(matches!(
                name.argument_type()
                    .map(|argument| argument.protocol_argument().0),
                Some(ProtocolArgumentType::String { .. })
            ));
        }
    }
}
