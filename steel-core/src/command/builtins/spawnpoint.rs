//! Vanilla per-player spawn-point command.

use std::{slice, sync::Arc};

use steel_utils::{BlockPos, Identifier, java::float_to_string, translations};
use text_components::TextComponent;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::{
    entity::Entity as _,
    level_data::RespawnData,
    player::{Player, PlayerRespawnConfig},
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("spawnpoint"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("spawnpoint").executes(set_source_spawn).then(
        argument("targets", SteelArgumentType::players())
            .executes(set_source_position)
            .then(
                argument("pos", SteelArgumentType::block_pos())
                    .executes(set_target_position)
                    .then(
                        argument("rotation", SteelArgumentType::rotation())
                            .executes(set_target_position_and_rotation),
                    ),
            ),
    )
}

fn set_source_spawn(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let Some(player) = context.source().player() else {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::PERMISSIONS_REQUIRES_PLAYER,
        )));
    };
    set_spawn(
        context,
        slice::from_ref(player),
        BlockPos::from(context.source().position()),
        (0.0, 0.0),
    )
}

fn set_source_position(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    set_spawn(
        context,
        &targets,
        BlockPos::from(context.source().position()),
        (0.0, 0.0),
    )
}

fn set_target_position(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    let position = super::setworldspawn::spawnable_position(context)?;
    set_spawn(context, &targets, position, (0.0, 0.0))
}

fn set_target_position_and_rotation(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    let position = super::setworldspawn::spawnable_position(context)?;
    let rotation = context.coordinates("rotation")?.rotation(context.source());
    set_spawn(context, &targets, position, rotation)
}

fn set_spawn(
    context: &SteelCommandContext<CommandSource>,
    targets: &[Arc<Player>],
    position: BlockPos,
    (yaw, pitch): (f32, f32),
) -> Result<i32, CommandSyntaxError> {
    let source = context.source();
    let respawn_data = RespawnData::of(source.world().key.clone(), position, yaw, pitch);
    let config = PlayerRespawnConfig::new(respawn_data.clone(), true);

    for target in targets {
        target.set_respawn_position(Some(config.clone()), false);
    }

    let message = if let [target] = targets {
        translations::COMMANDS_SPAWNPOINT_SUCCESS_SINGLE
            .message([
                TextComponent::from(position.x().to_string()),
                TextComponent::from(position.y().to_string()),
                TextComponent::from(position.z().to_string()),
                TextComponent::from(float_to_string(respawn_data.yaw)),
                TextComponent::from(float_to_string(respawn_data.pitch)),
                TextComponent::from(source.world().key.to_string()),
                target.display_name(),
            ])
            .component()
    } else {
        translations::COMMANDS_SPAWNPOINT_SUCCESS_MULTIPLE
            .message([
                TextComponent::from(position.x().to_string()),
                TextComponent::from(position.y().to_string()),
                TextComponent::from(position.z().to_string()),
                TextComponent::from(float_to_string(respawn_data.yaw)),
                TextComponent::from(float_to_string(respawn_data.pitch)),
                TextComponent::from(source.world().key.to_string()),
                TextComponent::from(targets.len().to_string()),
            ])
            .component()
    };
    source.send_success(&message, true);

    i32::try_from(targets.len()).map_err(|_| {
        CommandSyntaxError::dynamic("Target player count exceeds the command result range")
    })
}

#[cfg(test)]
mod tests {
    use super::super::create_dispatcher;
    use crate::command::{
        brigadier::{CommandDispatcher, NodeId},
        execution::{CommandSource, SteelArgumentType, SteelCommandRuntime},
    };
    use steel_registry::init_vanilla_registry;

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

    fn assert_executable(dispatcher: &Dispatcher, node: NodeId) {
        let Some(node) = dispatcher.node(node) else {
            panic!("command node should exist");
        };
        assert!(node.is_executable());
    }

    #[test]
    fn spawnpoint_graph_matches_vanilla_argument_paths() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let root = child(&dispatcher, dispatcher.root(), "spawnpoint");
        assert_executable(&dispatcher, root);

        let targets = child(&dispatcher, root, "targets");
        assert_eq!(
            dispatcher
                .node(targets)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::players())
        );
        assert_executable(&dispatcher, targets);

        let position = child(&dispatcher, targets, "pos");
        assert_eq!(
            dispatcher
                .node(position)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::block_pos())
        );
        assert_executable(&dispatcher, position);

        let rotation = child(&dispatcher, position, "rotation");
        assert_eq!(
            dispatcher
                .node(rotation)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::rotation())
        );
        assert_executable(&dispatcher, rotation);
    }
}
