//! Entity summoning command.

use std::sync::Arc;

use glam::DVec3;
use simdnbt::owned::NbtCompound;
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
use crate::entity::load_entity_recursive_owned;
use crate::{
    entity::{AddEntityError, ENTITIES, EntitySpawnReason, SharedEntity},
    world::World,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("summon"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("summon").then(
        argument("entity", SteelArgumentType::summonable_entity())
            .executes(|context| summon_entity(context, context.source().position(), None))
            .then(
                argument("pos", SteelArgumentType::vec3(true))
                    .executes(|context| {
                        let position = context.coordinates("pos")?;
                        summon_entity(context, position.position(context.source()), None)
                    })
                    .then(
                        argument("nbt", SteelArgumentType::compound_tag()).executes(|context| {
                            let position = context.coordinates("pos")?;
                            let nbt = context.compound_tag("nbt")?;
                            summon_entity(context, position.position(context.source()), Some(nbt))
                        }),
                    ),
            ),
    )
}

fn summon_entity(
    context: &SteelCommandContext<CommandSource>,
    position: DVec3,
    nbt: Option<&NbtCompound>,
) -> Result<i32, CommandSyntaxError> {
    let entity_type = context.entity_type("entity")?;
    let entity = create_entity(context, entity_type, position, nbt)?;
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
    nbt: Option<&NbtCompound>,
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

    let mut _loaded_tree = None;
    let entity = if let Some(nbt) = nbt {
        let mut compound = nbt.clone();
        compound.remove("id");
        compound.insert("id", entity_type.key.to_string());

        let Some(loaded) =
            load_entity_recursive_owned(world, &compound, EntitySpawnReason::Command, |entity| {
                entity.set_position_local(position);
                entity.set_old_position_to_current();
            })
        else {
            return Err(command_failed(&translations::COMMANDS_SUMMON_FAILED));
        };
        let root = Arc::clone(&loaded.root);
        _loaded_tree = Some(loaded);
        root
    } else {
        let Some(entity) = ENTITIES.create(entity_type, position, Arc::downgrade(world)) else {
            return Err(command_failed(&translations::COMMANDS_SUMMON_FAILED));
        };

        if let Some(mob) = entity.as_mob() {
            let difficulty = world.get_current_difficulty_at(entity.block_position());
            let _ = mob.finalize_spawn(world, difficulty, EntitySpawnReason::Command, None);
        }

        entity
    };

    match world.try_add_fresh_entity_with_passengers(Arc::clone(&entity)) {
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

#[cfg(test)]
mod tests {
    use super::super::create_dispatcher;
    use crate::bootstrap::init_globals;
    use crate::command::{
        brigadier::{CommandDispatcher, NodeId},
        execution::{CommandSource, SteelArgumentType, SteelCommandRuntime},
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
    fn summon_graph_uses_typed_entity_deferred_position_and_compound_tag_arguments() {
        init_globals();
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

        let nbt = child(&dispatcher, position, "nbt");
        assert_eq!(
            dispatcher.node(nbt).and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::compound_tag())
        );
        assert!(matches!(
            dispatcher.node(nbt),
            Some(node) if node.is_executable()
        ));
        assert!(dispatcher.children(nbt).is_some_and(<[_]>::is_empty));
    }
}
