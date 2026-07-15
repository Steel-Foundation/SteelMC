//! Entity summoning command.

use std::sync::Arc;

use glam::DVec3;
use steel_registry::entity_type::EntityTypeRef;
use steel_utils::{BlockPos, Identifier, translations, types::Difficulty};
use text_components::{TextComponent, translation::Translation};

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::{
    entity::{AddEntityError, ENTITIES, EntitySpawnReason, SharedEntity, next_entity_id},
    world::World,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("summon"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("summon").then(
        argument("entity", SteelArgumentType::summonable_entity())
            .executes(|context| summon_entity(context, context.source().position()))
            .then(
                argument("pos", SteelArgumentType::vec3(true)).executes(|context| {
                    let Some(position) = context.coordinates("pos") else {
                        return Err(missing_argument("pos"));
                    };
                    summon_entity(context, position.position(context.source()))
                }),
            ),
    )
    // TODO: Add the vanilla compound-NBT branch once Steel has an SNBT compound
    // argument and recursive command entity loading.
}

fn summon_entity(
    context: &SteelCommandContext<CommandSource>,
    position: DVec3,
) -> Result<i32, CommandSyntaxError> {
    let Some(entity_type) = context.entity_type("entity") else {
        return Err(missing_argument("entity"));
    };
    let entity = create_entity(context, entity_type, position)?;
    let message = translations::COMMANDS_SUMMON_SUCCESS
        .message([entity.display_name()])
        .component();
    context.source().send_success(&message, true);
    Ok(1)
}

pub(super) fn create_entity(
    context: &SteelCommandContext<CommandSource>,
    entity_type: EntityTypeRef,
    position: DVec3,
) -> Result<SharedEntity, CommandSyntaxError> {
    if !World::is_in_spawnable_bounds(BlockPos::from(position)) {
        return Err(command_failed(
            &translations::COMMANDS_SUMMON_INVALID_POSITION,
        ));
    }

    let world = context.source().world();
    if world.difficulty() == Difficulty::Peaceful && !entity_type.allowed_in_peaceful {
        return Err(command_failed(
            &translations::COMMANDS_SUMMON_FAILED_PEACEFUL,
        ));
    }

    let Some(entity) = ENTITIES.create(
        entity_type,
        next_entity_id(),
        position,
        Arc::downgrade(world),
    ) else {
        return Err(command_failed(&translations::COMMANDS_SUMMON_FAILED));
    };

    if let Some(mob) = entity.as_mob() {
        let _ = mob.finalize_spawn(world, EntitySpawnReason::Command, None);
    }

    match world.try_add_entity(Arc::clone(&entity)) {
        Ok(()) => Ok(entity),
        Err(AddEntityError::DuplicateUuid { .. }) => {
            Err(command_failed(&translations::COMMANDS_SUMMON_FAILED_UUID))
        }
        Err(_) => Err(command_failed(&translations::COMMANDS_SUMMON_FAILED)),
    }
}

fn command_failed(translation: &'static Translation<0>) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(TextComponent::from(translation))
}

fn missing_argument(name: &str) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(format!(
        "Parsed value for {name} is missing from the command context"
    ))
}

#[cfg(test)]
mod tests {
    use super::super::create_dispatcher;
    use crate::{
        command::{
            brigadier::{CommandDispatcher, NodeId},
            execution::{CommandSource, SteelArgumentType, SteelCommandRuntime},
        },
        entity::init_test_entities,
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
    fn summon_graph_uses_typed_entity_and_deferred_position_arguments() {
        init_test_entities();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let root = child(&dispatcher, dispatcher.root(), "summon");
        let entity = child(&dispatcher, root, "entity");
        assert_eq!(
            dispatcher
                .node(entity)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::summonable_entity())
        );
        assert!(matches!(
            dispatcher.node(entity),
            Some(node) if node.is_executable()
        ));

        let position = child(&dispatcher, entity, "pos");
        assert_eq!(
            dispatcher
                .node(position)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::vec3(true))
        );
        assert!(matches!(
            dispatcher.node(position),
            Some(node) if node.is_executable()
        ));
        assert!(dispatcher.children(position).is_some_and(<[_]>::is_empty));
    }
}
