//! Handler for the "summon" command.
//!
//! A basic summon command that spawns block display entities.

use std::sync::Arc;

use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::math::Vector3;
use text_components::TextComponent;

use crate::command::arguments::vector3::Vector3Argument;
use crate::command::commands::{
    CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn, argument,
};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use crate::command::sender::CommandSender;
use crate::entity::entities::BlockDisplayEntity;
use crate::entity::{Entity, next_entity_id};

/// Handler for the "summon" command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["summon"],
        "Summons an entity.",
        "minecraft:command.summon",
    )
    // /summon - summons at player position
    .executes(SummonAtSelfExecutor)
    // /summon <x> <y> <z> - summons at specified position
    .then(argument("pos", Vector3Argument).executes(SummonAtPosExecutor))
}

struct SummonAtSelfExecutor;

impl CommandExecutor<()> for SummonAtSelfExecutor {
    fn execute(&self, _args: (), context: &mut CommandContext) -> Result<(), CommandError> {
        let CommandSender::Player(player) = &context.sender else {
            return Err(CommandError::CommandFailed(Box::new(TextComponent::plain(
                "This command can only be used by players",
            ))));
        };

        let pos = player.position();
        let world = &player.world;

        // Create the block display entity
        let entity = Arc::new(BlockDisplayEntity::new(
            next_entity_id(),
            pos,
            Arc::downgrade(world),
        ));

        entity.set_block_state_id(REGISTRY.blocks.get_base_state_id(vanilla_blocks::STONE));

        // Add it to the world
        world.add_entity(entity);

        context.sender.send_message(&TextComponent::plain(format!(
            "Summoned block_display at {:.2}, {:.2}, {:.2}",
            pos.x, pos.y, pos.z
        )));

        Ok(())
    }
}

struct SummonAtPosExecutor;

impl CommandExecutor<((), Vector3<f64>)> for SummonAtPosExecutor {
    fn execute(
        &self,
        args: ((), Vector3<f64>),
        context: &mut CommandContext,
    ) -> Result<(), CommandError> {
        let ((), pos) = args;

        let CommandSender::Player(player) = &context.sender else {
            return Err(CommandError::CommandFailed(Box::new(TextComponent::plain(
                "This command can only be used by players",
            ))));
        };

        let world = &player.world;

        // Create the block display entity
        let entity = Arc::new(BlockDisplayEntity::new(
            next_entity_id(),
            pos,
            Arc::downgrade(world),
        ));

        // Add it to the world
        world.add_entity(entity);

        context.sender.send_message(&TextComponent::plain(format!(
            "Summoned block_display at {:.2}, {:.2}, {:.2}",
            pos.x, pos.y, pos.z
        )));

        Ok(())
    }
}
