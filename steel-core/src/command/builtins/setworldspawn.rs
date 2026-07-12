//! Default world spawn command.

use steel_utils::{BlockPos, Identifier, translations};
use text_components::TextComponent;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::{level_data::RespawnData, world::World};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("setworldspawn"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("setworldspawn")
        .executes(|context| {
            set_spawn(
                context,
                BlockPos::from(context.source().position()),
                (0.0, 0.0),
            )
        })
        .then(
            argument("pos", SteelArgumentType::block_pos())
                .executes(|context| {
                    let position = spawnable_position(context)?;
                    set_spawn(context, position, (0.0, 0.0))
                })
                .then(
                    argument("rotation", SteelArgumentType::rotation()).executes(|context| {
                        let position = spawnable_position(context)?;
                        let Some(rotation) = context.coordinates("rotation") else {
                            return Err(missing_argument("rotation"));
                        };
                        set_spawn(context, position, rotation.rotation(context.source()))
                    }),
                ),
        )
}

fn spawnable_position(
    context: &SteelCommandContext<CommandSource>,
) -> Result<BlockPos, CommandSyntaxError> {
    let Some(coordinates) = context.coordinates("pos") else {
        return Err(missing_argument("pos"));
    };
    let position = coordinates.block_pos(context.source());
    if !World::is_in_spawnable_bounds(position) {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::ARGUMENT_POS_OUTOFBOUNDS,
        )));
    }
    Ok(position)
}

fn set_spawn(
    context: &SteelCommandContext<CommandSource>,
    position: BlockPos,
    (yaw, pitch): (f32, f32),
) -> Result<i32, CommandSyntaxError> {
    let source = context.source();
    let respawn_data = RespawnData::of(source.world().key.clone(), position, yaw, pitch);
    let yaw = respawn_data.yaw;
    let pitch = respawn_data.pitch;
    source
        .server()
        .set_respawn_data(respawn_data)
        .map_err(CommandSyntaxError::dynamic)?;

    let message = translations::COMMANDS_SETWORLDSPAWN_SUCCESS_NEW
        .message([
            position.x().to_string(),
            position.y().to_string(),
            position.z().to_string(),
            yaw.to_string(),
            pitch.to_string(),
            source.world().key.to_string(),
        ])
        .component();
    source.send_success(&message, true);
    Ok(1)
}

fn missing_argument(name: &str) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(format!(
        "Parsed value for {name} is missing from the command context"
    ))
}

#[cfg(test)]
mod tests {
    use super::super::create_dispatcher;
    use crate::command::{
        brigadier::{CommandDispatcher, NodeId},
        execution::{CommandSource, SteelArgumentType, SteelCommandRuntime},
    };
    use steel_registry::test_support::init_test_registry;

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
    fn setworldspawn_graph_uses_deferred_coordinate_arguments() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let root = child(&dispatcher, dispatcher.root(), "setworldspawn");
        let Some(root_node) = dispatcher.node(root) else {
            panic!("setworldspawn root should exist");
        };
        assert!(root_node.is_executable());

        let position = child(&dispatcher, root, "pos");
        assert_eq!(
            dispatcher
                .node(position)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::block_pos())
        );
        let Some(position_node) = dispatcher.node(position) else {
            panic!("setworldspawn position should exist");
        };
        assert!(position_node.is_executable());

        let rotation = child(&dispatcher, position, "rotation");
        assert_eq!(
            dispatcher
                .node(rotation)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::rotation())
        );
        let Some(rotation_node) = dispatcher.node(rotation) else {
            panic!("setworldspawn rotation should exist");
        };
        assert!(rotation_node.is_executable());
    }
}
