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
    let count = context.integer("count")?;
    give(context, count)
}

fn give(
    context: &SteelCommandContext<CommandSource>,
    count: i32,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    let prototype = context.item_stack("item")?;
    validate_give_count(prototype, count)?;

    for target in &targets {
        give_to_player(target, prototype, count);
    }

    let message = if let [target] = targets.as_slice() {
        translations::COMMANDS_GIVE_SUCCESS_SINGLE
            .message([
                TextComponent::from(count.to_string()),
                item_display_name(prototype),
                target.display_name(),
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

fn validate_give_count(prototype: &ItemStack, count: i32) -> Result<i32, CommandSyntaxError> {
    let max_allowed_count = prototype.max_stack_size() * MAX_ALLOWED_ITEM_STACKS;
    if count > max_allowed_count {
        let message = translations::COMMANDS_GIVE_FAILED_TOOMANYITEMS
            .message([
                TextComponent::from(max_allowed_count.to_string()),
                item_display_name(prototype),
            ])
            .component();
        return Err(CommandSyntaxError::dynamic(message));
    }
    Ok(max_allowed_count)
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
        } else {
            let partial_added = stack.count() < size;
            if let Some(item) = player.drop_item(stack, false, false) {
                item.set_no_pickup_delay();
                item.set_owner(Some(player.gameprofile.id));
            }
            if partial_added {
                player.broadcast_inventory_changes();
            }
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use steel_registry::{init_vanilla_registry, vanilla_items};

    use super::super::create_dispatcher;
    use super::*;
    use crate::{
        command::brigadier::{CommandDispatcher, NodeId},
        test_support::{TestPlayerBuilder, test_world},
    };

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
        init_vanilla_registry();
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

    #[test]
    fn validate_give_count_rejects_exceeding_max_allowed_stacks() {
        init_vanilla_registry();

        let diamond = ItemStack::new(&vanilla_items::DIAMOND);
        assert_eq!(diamond.max_stack_size(), 64);
        assert!(validate_give_count(&diamond, 6400).is_ok());
        let err = validate_give_count(&diamond, 6401)
            .expect_err("count 6401 should exceed 100 stacks of 64");
        let debug = format!("{:?}", err.message_component());
        assert!(debug.contains("6400"));
        assert!(debug.contains("commands.give.failed.toomanyitems"));

        let sword = ItemStack::new(&vanilla_items::DIAMOND_SWORD);
        assert_eq!(sword.max_stack_size(), 1);
        assert!(validate_give_count(&sword, 100).is_ok());
        let err =
            validate_give_count(&sword, 101).expect_err("count 101 should exceed 100 stacks of 1");
        assert!(format!("{:?}", err.message_component()).contains("100"));

        let pearl = ItemStack::new(&vanilla_items::ENDER_PEARL);
        assert_eq!(pearl.max_stack_size(), 16);
        assert!(validate_give_count(&pearl, 1600).is_ok());
        let err = validate_give_count(&pearl, 1601)
            .expect_err("count 1601 should exceed 100 stacks of 16");
        assert!(format!("{:?}", err.message_component()).contains("1600"));
    }

    #[test]
    fn player_display_name_contains_interactivity() {
        init_vanilla_registry();
        let world = test_world();
        let player = TestPlayerBuilder::new(Arc::clone(world), "PlayerOne", 3).build();
        let display = player.display_name();
        assert_eq!(player.plain_text_name(), "PlayerOne");
        assert!(display.interactions.click.is_some());
        assert!(display.interactions.hover.is_some());
        assert_eq!(display.interactions.insertion.as_deref(), Some("PlayerOne"));
    }

    #[test]
    fn give_to_player_adds_items_to_inventory() {
        init_vanilla_registry();
        let world = test_world();
        let player = TestPlayerBuilder::new(Arc::clone(world), "TestReceiver", 1).build();
        let stack = ItemStack::new(&vanilla_items::DIAMOND);
        give_to_player(&player, &stack, 5);

        let inv = player.inventory.lock();
        let item = inv.get_item(0);
        assert_eq!(item.item(), &*vanilla_items::DIAMOND);
        assert_eq!(item.count(), 5);
    }

    #[test]
    fn give_to_player_with_full_inventory_drops_remaining() {
        init_vanilla_registry();
        let world = test_world();
        let player = TestPlayerBuilder::new(Arc::clone(world), "TestReceiver", 2).build();

        {
            let mut inv = player.inventory.lock();
            for slot in 0..inv.get_container_size() {
                inv.set_item(
                    slot,
                    ItemStack::new(&vanilla_items::DIRT).copy_with_count(64),
                );
            }
        }

        let stack = ItemStack::new(&vanilla_items::DIAMOND);
        give_to_player(&player, &stack, 5);

        let inv = player.inventory.lock();
        assert_eq!(inv.get_item(0).item(), &*vanilla_items::DIRT);
    }
}
