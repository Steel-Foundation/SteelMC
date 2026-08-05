//! Vanilla item-giving command.

use steel_protocol::packets::game::SoundSource;
use steel_registry::{
    data_components::vanilla_components::{CUSTOM_NAME, ITEM_NAME},
    item_stack::ItemStack,
    sound_events,
};
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
use crate::{entity::Entity as _, inventory::container::Container as _, player::Player};

const MAX_ALLOWED_ITEM_STACKS: i32 = 100;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("give"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("give").then(
        argument("targets", SteelArgumentType::players()).then(
            argument("item", SteelArgumentType::item_stack())
                .executes(give_default_count)
                .then(
                    argument("count", ArgumentType::integer(1, i32::MAX)).executes(give_with_count),
                ),
        ),
    )
}

fn give_default_count(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    give(context, 1)
}

fn give_with_count(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let Some(count) = context.integer("count") else {
        return Err(missing_argument("count"));
    };
    give(context, count)
}

fn give(
    context: &SteelCommandContext<CommandSource>,
    count: i32,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    let Some(prototype) = context.item_stack("item") else {
        return Err(missing_argument("item"));
    };
    let max_allowed_count = prototype.max_stack_size() * MAX_ALLOWED_ITEM_STACKS;
    if count > max_allowed_count {
        let message = translations::COMMANDS_GIVE_FAILED_TOOMANYITEMS
            .message([
                TextComponent::from(max_allowed_count.to_string()),
                item_display_name(prototype),
            ])
            .component();
        context.source().send_failure(message);
        return Ok(0);
    }

    for target in &targets {
        give_to_player(target, prototype, count);
    }

    let message = if let [target] = targets.as_slice() {
        translations::COMMANDS_GIVE_SUCCESS_SINGLE
            .message([
                TextComponent::from(count.to_string()),
                item_display_name(prototype),
                TextComponent::plain(target.plain_text_name()),
            ])
            .component()
    } else {
        translations::COMMANDS_GIVE_SUCCESS_MULTIPLE
            .message([
                TextComponent::from(count.to_string()),
                item_display_name(prototype),
                TextComponent::from(targets.len().to_string()),
            ])
            .component()
    };
    context.source().send_success(&message, true);

    i32::try_from(targets.len()).map_err(|_| {
        CommandSyntaxError::dynamic("Target player count exceeds the command result range")
    })
}

fn give_to_player(player: &Player, prototype: &ItemStack, count: i32) {
    let max_stack_size = prototype.max_stack_size();
    let mut remaining = count;
    while remaining > 0 {
        let size = max_stack_size.min(remaining);
        remaining -= size;
        let mut stack = prototype.copy_with_count(size);
        let added = player.inventory.lock().add(&mut stack);

        if added && stack.is_empty() {
            if let Some(item) = player.drop_item(prototype.copy_with_count(1), false, false) {
                item.make_fake_item();
            }
            play_pickup_sound(player);
            player.broadcast_inventory_changes();
        } else if let Some(item) = player.drop_item(stack, false, false) {
            item.set_no_pickup_delay();
            item.set_owner(Some(player.gameprofile.id));
        }
    }
}

fn play_pickup_sound(player: &Player) {
    let pitch = ((rand::random::<f32>() - rand::random::<f32>()) * 0.7 + 1.0) * 2.0;
    player.get_world().play_sound_at(
        &sound_events::ENTITY_ITEM_PICKUP,
        SoundSource::Players,
        player.position(),
        0.2,
        pitch,
        None,
    );
}

fn item_display_name(stack: &ItemStack) -> TextComponent {
    stack
        .get(CUSTOM_NAME)
        .or_else(|| stack.get(ITEM_NAME))
        .cloned()
        .unwrap_or_else(|| TextComponent::plain(stack.item().key.to_string()))
}

fn missing_argument(name: &str) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(format!(
        "Parsed value for {name} is missing from the command context"
    ))
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;

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
    fn give_graph_uses_players_item_stack_and_positive_count() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let give = child(&dispatcher, dispatcher.root(), "give");
        let Some(give_node) = dispatcher.node(give) else {
            panic!("give node should exist");
        };
        assert!(give_node.is_restricted());

        let targets = child(&dispatcher, give, "targets");
        assert_eq!(
            dispatcher
                .node(targets)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::players())
        );

        let item = child(&dispatcher, targets, "item");
        let Some(item_node) = dispatcher.node(item) else {
            panic!("item node should exist");
        };
        assert_eq!(
            item_node.argument_type(),
            Some(&SteelArgumentType::item_stack())
        );
        assert!(item_node.is_executable());

        let count = child(&dispatcher, item, "count");
        let Some(count_node) = dispatcher.node(count) else {
            panic!("count node should exist");
        };
        assert_eq!(
            count_node.argument_type(),
            Some(&SteelArgumentType::from(ArgumentType::integer(1, i32::MAX)))
        );
        assert!(count_node.is_executable());
    }
}
