//! Handler for the "summon" command.

use std::borrow::Cow;
use std::sync::Arc;

use glam::DVec3;
use steel_registry::entity_type::EntityTypeRef;
use steel_utils::types::Difficulty;
use steel_utils::{BlockPos, translations};
use text_components::TextComponent;
use text_components::translation::TranslatedMessage;

use crate::command::arguments::entity_type::EntitySummonArgument;
use crate::command::arguments::vector3::Vector3Argument;
use crate::command::commands::{
    CommandExecutor, CommandHandlerBuilder, CommandHandlerDyn, argument,
};
use crate::command::context::CommandContext;
use crate::command::error::CommandError;
use crate::entity::{
    AddEntityError, ENTITIES, Entity, EntitySpawnReason, SharedEntity, next_entity_id,
};
use crate::world::World;

/// Handler for the "summon" command.
#[must_use]
pub fn command_handler() -> impl CommandHandlerDyn {
    CommandHandlerBuilder::new(
        &["summon"],
        "Summons an entity.",
        "minecraft:command.summon",
    )
    .then(
        argument("entity", EntitySummonArgument)
            .executes(SummonAtSourceExecutor)
            .then(argument("pos", Vector3Argument).executes(SummonAtPosExecutor)),
    )
}

struct SummonAtSourceExecutor;

impl CommandExecutor<((), EntityTypeRef)> for SummonAtSourceExecutor {
    fn execute(
        &self,
        ((), entity_type): ((), EntityTypeRef),
        context: &mut CommandContext,
    ) -> Result<(), CommandError> {
        summon_entity(context, entity_type, context.position)
    }
}

struct SummonAtPosExecutor;

impl CommandExecutor<(((), EntityTypeRef), DVec3)> for SummonAtPosExecutor {
    fn execute(
        &self,
        (((), entity_type), pos): (((), EntityTypeRef), DVec3),
        context: &mut CommandContext,
    ) -> Result<(), CommandError> {
        summon_entity(context, entity_type, pos)
    }
}

fn summon_entity(
    context: &mut CommandContext,
    entity_type: EntityTypeRef,
    pos: DVec3,
) -> Result<(), CommandError> {
    let entity = create_entity(context, entity_type, pos)?;
    context.sender.send_message(
        &translations::COMMANDS_SUMMON_SUCCESS
            .message([entity_display_name(entity.as_ref())])
            .into(),
    );
    Ok(())
}

fn create_entity(
    context: &CommandContext,
    entity_type: EntityTypeRef,
    pos: DVec3,
) -> Result<SharedEntity, CommandError> {
    let block_pos = BlockPos::containing(pos.x, pos.y, pos.z);
    if !World::is_in_spawnable_bounds(block_pos) {
        return Err(command_failed(
            translations::COMMANDS_SUMMON_INVALID_POSITION.msg(),
        ));
    }

    if context.world.difficulty() == Difficulty::Peaceful && !entity_type.allowed_in_peaceful {
        return Err(command_failed(
            translations::COMMANDS_SUMMON_FAILED_PEACEFUL.msg(),
        ));
    }

    let world = Arc::clone(&context.world);
    let Some(entity) = ENTITIES.create(entity_type, next_entity_id(), pos, Arc::downgrade(&world))
    else {
        return Err(command_failed(translations::COMMANDS_SUMMON_FAILED.msg()));
    };

    if let Some(mob) = entity.as_mob() {
        let _ = mob.finalize_spawn(&world, EntitySpawnReason::Command, None);
    }

    match world.try_add_entity(Arc::clone(&entity)) {
        Ok(()) => Ok(entity),
        Err(AddEntityError::DuplicateUuid { .. }) => Err(command_failed(
            translations::COMMANDS_SUMMON_FAILED_UUID.msg(),
        )),
        Err(_) => Err(command_failed(translations::COMMANDS_SUMMON_FAILED.msg())),
    }
}

fn command_failed(message: TranslatedMessage) -> CommandError {
    CommandError::CommandFailed(Box::new(message.into()))
}

fn entity_display_name(entity: &dyn Entity) -> TextComponent {
    entity
        .custom_name()
        .unwrap_or_else(|| entity_type_display_name(entity.entity_type()))
}

fn entity_type_display_name(entity_type: EntityTypeRef) -> TextComponent {
    TextComponent::translated(TranslatedMessage {
        key: Cow::Owned(format!(
            "entity.{}.{}",
            entity_type.key.namespace, entity_type.key.path
        )),
        fallback: None,
        args: None,
    })
}
