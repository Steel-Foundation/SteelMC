//! Handler for the "clear" command.
use std::sync::Arc;

use steel_registry::{item_stack::ItemStack, items::ItemRef};
use steel_utils::translations;
use text_components::TextComponent;

use crate::{
    command::{
        arguments::{integer::IntegerArgument, item::ItemStackArgument, player::PlayerArgument},
        commands::{CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn, argument},
        context::CommandContext,
        error::CommandError,
        sender::CommandSender,
    },
    inventory::container::Container,
    player::Player,
};

/// Handler for the "clear" command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["clear"],
        "Clears the Player's inventory.",
        "minecraft:command.clear",
    )
    .executes(ClearNoArgumentExecutor)
    .then(
        argument("targets", PlayerArgument::multiple())
            .executes(ClearMultipleArgumentExecutor)
            .then(
                argument("item", ItemStackArgument)
                    .executes(ClearWithItemExecutor) // FIXME: item predicate instead
                    .then(
                        argument("maxCount", IntegerArgument::bounded(Some(0), None))
                            .executes(ClearWithMaxAmountExecutor),
                    ),
            ),
    )
}

struct ClearNoArgumentExecutor;

impl CommandExecutor<()> for ClearNoArgumentExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let player = context
            .sender
            .get_player()
            .ok_or(CommandError::InvalidRequirement)?;

        let count = { player.inventory.lock().clear_content() };

        clear_messages(
            &context.sender,
            count,
            1,
            Some(player.gameprofile.name.clone()),
        );

        Ok(())
    }
}

struct ClearMultipleArgumentExecutor;

impl CommandExecutor<((), Vec<Arc<Player>>)> for ClearMultipleArgumentExecutor {
    fn execute(
        &self,
        args: ((), Vec<Arc<Player>>),
        context: &mut CommandContext,
    ) -> Result<(), CommandError> {
        let ((), targets) = args;

        let count = targets
            .iter()
            .map(|player| player.inventory.lock().clear_content())
            .sum();

        clear_messages(
            &context.sender,
            count,
            targets.len(),
            targets.first().map(|it| it.gameprofile.name.clone()),
        );

        Ok(())
    }
}

struct ClearWithItemExecutor;

impl CommandExecutor<(((), Vec<Arc<Player>>), ItemRef)> for ClearWithItemExecutor {
    fn execute(
        &self,
        args: (((), Vec<Arc<Player>>), ItemRef),
        context: &mut CommandContext,
    ) -> Result<(), CommandError> {
        let (((), targets), item) = args;

        let mut filter = |item_stack: &mut ItemStack| item_stack.is(item);

        let count: i32 = targets
            .iter()
            .map(|it| it.inventory.lock().clear_content_matching(&mut filter))
            .sum();

        clear_messages(
            &context.sender,
            count,
            targets.len(),
            targets.first().map(|it| it.gameprofile.name.clone()),
        );

        Ok(())
    }
}

struct ClearWithMaxAmountExecutor;

impl CommandExecutor<((((), Vec<Arc<Player>>), ItemRef), i32)> for ClearWithMaxAmountExecutor {
    fn execute(
        &self,
        args: ((((), Vec<Arc<Player>>), ItemRef), i32),
        context: &mut CommandContext,
    ) -> Result<(), CommandError> {
        let ((((), targets), item), max_amount) = args;

        let count: i32 = targets
            .iter()
            .map(|it| {
                let mut current_amount = max_amount;
                let mut inventory = it.inventory.lock();
                for i in 0..inventory.get_container_size() {
                    let current_item = inventory.get_item_mut(i);
                    if current_item.is_empty() || !current_item.is(item) {
                        continue;
                    }
                    let amount_to_remove = current_amount.min(current_item.count);
                    current_amount -= amount_to_remove;
                    current_item.shrink(amount_to_remove);
                }
                max_amount - current_amount
            })
            .sum();

        clear_messages(
            &context.sender,
            count,
            targets.len(),
            targets.first().map(|it| it.gameprofile.name.clone()),
        );

        Ok(())
    }
}

fn clear_messages(
    sender: &CommandSender,
    count: i32,
    player_amount: usize,
    target_name: Option<String>,
) {
    if count == 0
        && player_amount > 1
        && let Some(name) = target_name
    {
        sender.send_message(
            &translations::CLEAR_FAILED_SINGLE
                .message([TextComponent::from(name)])
                .into(),
        );
    } else if count == 0 {
        sender.send_message(
            &translations::CLEAR_FAILED_MULTIPLE
                .message([TextComponent::from(format!("{player_amount}"))])
                .into(),
        );
    } else if player_amount == 1
        && let Some(name) = target_name
    {
        sender.send_message(
            &translations::COMMANDS_CLEAR_SUCCESS_SINGLE
                .message([
                    TextComponent::from(format!("{count}")),
                    TextComponent::from(name),
                ])
                .into(),
        );
    } else {
        sender.send_message(
            &translations::COMMANDS_CLEAR_SUCCESS_MULTIPLE
                .message([
                    TextComponent::from(format!("{count}")),
                    TextComponent::from(format!("{player_amount}")),
                ])
                .into(),
        );
    }
}
