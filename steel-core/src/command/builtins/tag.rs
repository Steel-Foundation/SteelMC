//! Vanilla entity scoreboard tag command.

use std::collections::BTreeSet;

use steel_utils::{Identifier, translations};
use text_components::{Modifier, TextComponent, format::Color};

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
                literal("remove")
                    .then(argument("name", SteelArgumentType::entity_tag()).executes(remove_tag)),
            )
            .then(literal("list").executes(list_tags)),
    )
}

#[derive(Clone, Copy)]
enum TagChange {
    Add,
    Remove,
}

fn add_tag(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    change_tags(context, TagChange::Add)
}

fn remove_tag(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    change_tags(context, TagChange::Remove)
}

fn change_tags(
    context: &SteelCommandContext<CommandSource>,
    change: TagChange,
) -> Result<i32, CommandSyntaxError> {
    let name = context.string("name")?;
    let (failed, single, multiple) = match change {
        TagChange::Add => (
            &translations::COMMANDS_TAG_ADD_FAILED,
            &translations::COMMANDS_TAG_ADD_SUCCESS_SINGLE,
            &translations::COMMANDS_TAG_ADD_SUCCESS_MULTIPLE,
        ),
        TagChange::Remove => (
            &translations::COMMANDS_TAG_REMOVE_FAILED,
            &translations::COMMANDS_TAG_REMOVE_SUCCESS_SINGLE,
            &translations::COMMANDS_TAG_REMOVE_SUCCESS_MULTIPLE,
        ),
    };
    let targets = context.entities("targets")?;
    let changed: Vec<_> = targets
        .iter()
        .filter(|target| match change {
            TagChange::Add => target.add_tag(name.to_owned()),
            TagChange::Remove => target.remove_tag(name),
        })
        .collect();

    let message = match changed.as_slice() {
        [] => return Err(CommandSyntaxError::dynamic(failed)),
        [target] => single
            .message([TextComponent::from(name.to_owned()), target.display_name()])
            .component(),
        _ => multiple
            .message([name.to_owned(), changed.len().to_string()])
            .component(),
    };
    context.source().send_success(&message, true);

    // max is 1024
    Ok(changed.len() as i32)
}

fn list_tags(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let targets = context.entities("targets")?;
    let tagged: Vec<_> = targets
        .iter()
        .filter(|target| !target.tags().is_empty())
        .collect();
    let tags: BTreeSet<String> = tagged.iter().flat_map(|target| target.tags()).collect();

    // Vanilla's empty feedback counts tagged targets, so it is always the
    // multiple form, even for a single target.
    let message = match tagged.as_slice() {
        [] => translations::COMMANDS_TAG_LIST_MULTIPLE_EMPTY
            .message(["0"])
            .component(),
        [target] => translations::COMMANDS_TAG_LIST_SINGLE_SUCCESS
            .message([
                target.display_name(),
                TextComponent::from(tags.len().to_string()),
                format_list(&tags),
            ])
            .component(),
        _ => translations::COMMANDS_TAG_LIST_MULTIPLE_SUCCESS
            .message([
                TextComponent::from(tagged.len().to_string()),
                TextComponent::from(tags.len().to_string()),
                format_list(&tags),
            ])
            .component(),
    };
    context.source().send_success(&message, false);

    // max is 1024
    Ok(tags.len() as i32)
}

/// Sorted, green values joined by a gray separator.
fn format_list(values: &BTreeSet<String>) -> TextComponent {
    let mut list = TextComponent::plain("");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            list = list.add_child(TextComponent::plain(", ").color(Color::Gray));
        }
        list = list.add_child(TextComponent::plain(value.clone()).color(Color::Green));
    }
    list
}
