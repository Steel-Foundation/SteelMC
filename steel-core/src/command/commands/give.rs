//! /// Handler for the "give" command.
use std::sync::Arc;

use steel_registry::{data_components::vanilla_components, item_stack::ItemStack, items::ItemRef};
use steel_utils::translations;
use text_components::{Modifier, TextComponent, interactivity::HoverEvent};

use crate::{
    command::{
        arguments::{integer::IntegerArgument, item::ItemStackArgument, player::PlayerArgument},
        commands::{CommandHandlerBuilder, CommandHandlerDyn, argument},
        context::CommandContext,
        sender::CommandSender,
    },
    inventory::container::Container,
    player::Player,
};

type GiveWithCountArgs = ((((), Vec<Arc<Player>>), ItemRef), i32);

/// Handler for the "give" command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["give"],
        "Give players the specified item with a specific amount.",
        "minecraft:command.give",
    )
    .then(
        argument("targets", PlayerArgument::multiple()).then(
            argument("item", ItemStackArgument) // FIXME: should be item predicate instead to also handle tags and components
                .executes(
                    |(((), targets), item): (((), Vec<Arc<Player>>), ItemRef),
                     ctx: &mut CommandContext| {
                        give(&targets, item, 1, &ctx.sender);

                        Ok(())
                    },
                )
                .then(
                    argument("count", IntegerArgument::bounded(Some(1), None)).executes(
                        |((((), targets), item), input_count): GiveWithCountArgs,
                         ctx: &mut CommandContext| {
                            give(&targets, item, input_count, &ctx.sender);

                            Ok(())
                        },
                    ),
                ),
        ),
    )
}

fn give(targets: &Vec<Arc<Player>>, item: ItemRef, count: i32, sender: &CommandSender) {
    let max_stack_size = item
        .components
        .get(vanilla_components::MAX_STACK_SIZE)
        .unwrap_or(1);

    if count > max_stack_size * 100 {
        sender.send_message(
            &translations::COMMANDS_GIVE_FAILED_TOOMANYITEMS
                .message([
                    TextComponent::from(format!("{}", max_stack_size * 100)),
                    TextComponent::from(format!("[{}]", item.key.path)).hover_event(
                        // FIXME: display name
                        HoverEvent::show_item(item.key.path.clone(), None, None::<&str>),
                    ),
                ])
                .into(),
        );
        return;
    }

    let stack = ItemStack::new(item);

    for target in targets {
        let mut remaining = count;

        while remaining > 0 {
            let stack_size = max_stack_size.min(remaining);
            remaining -= stack_size;
            let mut copy = stack.copy_with_count(stack_size);
            let added = target.inventory.lock().add(&mut copy);

            if !added || !copy.is_empty() {
                target.drop_item(copy, false, false);
            }
        }
    }

    if targets.len() == 1 {
        sender.send_message(
            &translations::COMMANDS_GIVE_SUCCESS_SINGLE
                .message([
                    TextComponent::from(format!("{count}")),
                    TextComponent::from(format!("[{}]", item.key.path)).hover_event(
                        // FIXME: display name
                        HoverEvent::show_item(item.key.path.clone(), None, None::<&str>),
                    ),
                    TextComponent::from(
                        targets
                            .first()
                            .expect("targets cannot be empty.")
                            .gameprofile
                            .name
                            .clone(),
                    ),
                ])
                .into(),
        );
    } else {
        sender.send_message(
            &translations::COMMANDS_GIVE_SUCCESS_MULTIPLE
                .message([
                    TextComponent::from(format!("{count}")),
                    TextComponent::from(format!("[{}]", item.key.path)).hover_event(
                        // FIXME: display name
                        HoverEvent::show_item(item.key.path.clone(), None, None::<&str>),
                    ),
                    TextComponent::from(targets.len().to_string()),
                ])
                .into(),
        );
    }
}
