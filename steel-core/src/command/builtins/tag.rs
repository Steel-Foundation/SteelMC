//! Vanilla entity scoreboard tag command.

use std::collections::BTreeSet;

use steel_utils::{Identifier, translations};
use text_components::{Modifier, TextComponent};

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
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
            .then(literal("add").then(
                argument("name", SteelArgumentType::word()).executes(add_tag),
            ))
            .then(literal("remove").then(
                argument("name", SteelArgumentType::entity_tag()).executes(remove_tag),
            ))
            .then(literal("list").executes(list_tags)),
    )
}

fn add_tag(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let targets = context.entities("targets")?;
    let tag = context.string("name")?;

    let changed = targets
        .iter()
        .any(|target| target.add_tag(tag.to_owned()));

    if !changed {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_TAG_ADD_FAILED,
        )));
    }

    let message = if let [target] = targets.as_slice() {
        translations::COMMANDS_TAG_ADD_SUCCESS_SINGLE
            .message([
                TextComponent::plain(tag.to_owned()),
                target.display_name(),
            ])
            .component()
    } else {
        translations::COMMANDS_TAG_ADD_SUCCESS_MULTIPLE
            .message([
                TextComponent::plain(tag.to_owned()),
                TextComponent::plain(targets.len().to_string()),
            ])
            .component()
    };

    context.source().send_success(&message, true);

    Ok(i32::from(changed))
}

fn remove_tag(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let targets = context.entities("targets")?;
    let tag = context.string("name")?;

    let changed = targets
        .iter()
        .any(|target| target.remove_tag(tag));

    if !changed {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_TAG_REMOVE_FAILED,
        )));
    }

    let message = if let [target] = targets.as_slice() {
        translations::COMMANDS_TAG_REMOVE_SUCCESS_SINGLE
            .message([
                TextComponent::plain(tag.to_owned()),
                target.display_name(),
            ])
            .component()
    } else {
        translations::COMMANDS_TAG_REMOVE_SUCCESS_MULTIPLE
            .message([
                TextComponent::plain(tag.to_owned()),
                TextComponent::plain(targets.len().to_string()),
            ])
            .component()
    };

    context.source().send_success(&message, true);

    Ok(i32::from(changed))
}

fn list_tags(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let targets = context.entities("targets")?;

    if let [target] = targets.as_slice() {
        let tags = target.tags();

        if tags.is_empty() {
            let message = translations::COMMANDS_TAG_LIST_SINGLE_EMPTY
                .message([target.display_name()])
                .component();

            context.source().send_success(&message, false);

            return Ok(0);
        }

        let message = translations::COMMANDS_TAG_LIST_SINGLE_SUCCESS
            .message([
                target.display_name(),
                TextComponent::plain(tags.len().to_string()),
                format_tag_list(&tags),
            ])
            .component();

        context.source().send_success(&message, false);

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

        context.source().send_success(&message, false);

        return Ok(0);
    }

    let tag_count = all_tags.len();

    let message = translations::COMMANDS_TAG_LIST_MULTIPLE_SUCCESS
        .message([
            TextComponent::plain(targets.len().to_string()),
            TextComponent::plain(tag_count.to_string()),
            format_tag_list(&all_tags.iter().cloned().collect::<Vec<_>>()),
        ])
        .component();

    context.source().send_success(&message, false);

    Ok(tag_count.min(i32::MAX as usize) as i32)
}

fn format_tag_list(tags: &[String]) -> TextComponent {
    let mut list = TextComponent::plain("");

    for (index, tag) in tags.iter().enumerate() {
        if index > 0 {
            list = list.add_child(TextComponent::plain(", "));
        }

        list = list.add_child(TextComponent::plain(tag.to_owned()));
    }

    list
}
