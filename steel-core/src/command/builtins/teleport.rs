//! Vanilla entity teleport command.

use std::{slice, sync::Arc};

use glam::DVec3;
use steel_protocol::packets::game::{AnimateAction, CAnimate, CSetCamera, RelativeMovement};
use steel_registry::entity_data::EntityPose;
use steel_utils::{BlockPos, Identifier, translations};
use text_components::TextComponent;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, Coordinates, SteelArgumentType, SteelCommandContext, SteelCommandRuntime,
        argument, literal,
    },
    registration::CommandRegistration,
};
use crate::{
    entity::{Entity, EntityAnchor, LivingEntity as _, SharedEntity, change_entity_world},
    portal::{TeleportPostTransition, TeleportTransition},
    world::World,
};

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("teleport"), |_| command()).alias("tp")
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("teleport")
        .then(
            argument("location", SteelArgumentType::vec3(true))
                .executes(teleport_source_to_position),
        )
        .then(
            argument("destination", SteelArgumentType::entity())
                .executes(teleport_source_to_entity),
        )
        .then(
            argument("targets", SteelArgumentType::entities())
                .then(target_position_branch())
                .then(
                    argument("destination", SteelArgumentType::entity())
                        .executes(teleport_targets_to_entity),
                ),
        )
}

fn target_position_branch() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    argument("location", SteelArgumentType::vec3(true))
        .executes(teleport_targets_to_position)
        .then(
            argument("rotation", SteelArgumentType::rotation())
                .executes(teleport_targets_to_position_with_rotation),
        )
        .then(
            literal("facing")
                .then(
                    literal("entity").then(
                        argument("facingEntity", SteelArgumentType::entity())
                            .executes(teleport_targets_facing_entity_feet)
                            .then(
                                argument("facingAnchor", SteelArgumentType::entity_anchor())
                                    .executes(teleport_targets_facing_entity_anchor),
                            ),
                    ),
                )
                .then(
                    argument("facingLocation", SteelArgumentType::vec3(true))
                        .executes(teleport_targets_facing_position),
                ),
        )
}

fn teleport_source_to_position(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let target = source_entity(context)?;
    let destination = required_coordinates(context, "location")?;
    teleport_to_position(context, slice::from_ref(target), destination, None, None)
}

fn teleport_source_to_entity(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let target = source_entity(context)?;
    let destination = context.entity("destination")?;
    teleport_to_entity(context, slice::from_ref(target), &destination)
}

fn teleport_targets_to_position(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.entities("targets")?;
    let destination = required_coordinates(context, "location")?;
    teleport_to_position(context, &targets, destination, None, None)
}

fn teleport_targets_to_position_with_rotation(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.entities("targets")?;
    let destination = required_coordinates(context, "location")?;
    let rotation = required_coordinates(context, "rotation")?;
    teleport_to_position(context, &targets, destination, Some(rotation), None)
}

fn teleport_targets_facing_entity_feet(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    teleport_targets_facing_entity(context, EntityAnchor::Feet)
}

fn teleport_targets_facing_entity_anchor(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let Some(anchor) = context.entity_anchor("facingAnchor") else {
        return Err(missing_argument("facingAnchor"));
    };
    teleport_targets_facing_entity(context, anchor)
}

fn teleport_targets_facing_entity(
    context: &SteelCommandContext<CommandSource>,
    anchor: EntityAnchor,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.entities("targets")?;
    let destination = required_coordinates(context, "location")?;
    let facing_entity = context.entity("facingEntity")?;
    teleport_to_position(
        context,
        &targets,
        destination,
        None,
        Some(TeleportFacing::Entity {
            target: facing_entity,
            anchor,
        }),
    )
}

fn teleport_targets_facing_position(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.entities("targets")?;
    let destination = required_coordinates(context, "location")?;
    let facing = required_coordinates(context, "facingLocation")?.position(context.source());
    teleport_to_position(
        context,
        &targets,
        destination,
        None,
        Some(TeleportFacing::Position(facing)),
    )
}

fn teleport_targets_to_entity(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.entities("targets")?;
    let destination = context.entity("destination")?;
    teleport_to_entity(context, &targets, &destination)
}

fn source_entity(
    context: &SteelCommandContext<CommandSource>,
) -> Result<&SharedEntity, CommandSyntaxError> {
    context.source().entity().ok_or_else(|| {
        CommandSyntaxError::dynamic(TextComponent::from(
            &translations::PERMISSIONS_REQUIRES_ENTITY,
        ))
    })
}

fn required_coordinates(
    context: &SteelCommandContext<CommandSource>,
    name: &str,
) -> Result<Coordinates, CommandSyntaxError> {
    context
        .coordinates(name)
        .ok_or_else(|| missing_argument(name))
}

fn teleport_to_entity(
    context: &SteelCommandContext<CommandSource>,
    targets: &[SharedEntity],
    destination: &SharedEntity,
) -> Result<i32, CommandSyntaxError> {
    let Some(target_world) = destination.level() else {
        return Err(CommandSyntaxError::dynamic(
            "Teleport destination is not in a live world",
        ));
    };
    let position = destination.position();
    ensure_spawnable_position(position)?;
    let rotation = destination.rotation();
    ensure_same_domain_targets(targets, &target_world)?;

    for target in targets {
        let transition = TeleportTransition {
            target_world: Arc::clone(&target_world),
            position,
            rotation: wrap_rotation(rotation),
            velocity: DVec3::ZERO,
            relatives: RelativeMovement::NONE,
            portal_cooldown: 0,
            as_passenger: false,
            post_transition: TeleportPostTransition::do_nothing(),
        };
        perform_teleport(context.source(), target, transition, None);
    }

    send_entity_success(context.source(), targets, destination.as_ref());
    target_count(targets)
}

fn teleport_to_position(
    context: &SteelCommandContext<CommandSource>,
    targets: &[SharedEntity],
    destination: Coordinates,
    rotation: Option<Coordinates>,
    facing: Option<TeleportFacing>,
) -> Result<i32, CommandSyntaxError> {
    let source = context.source();
    let position = destination.position(source);
    ensure_spawnable_position(position)?;
    let resolved_rotation = rotation.map(|rotation| rotation.rotation(source));
    ensure_same_domain_targets(targets, source.world())?;

    for target in targets {
        let same_world = target
            .level()
            .is_some_and(|world| Arc::ptr_eq(&world, source.world()));
        let relatives = teleport_relatives(
            (
                destination.is_x_relative(),
                destination.is_y_relative(),
                destination.is_z_relative(),
            ),
            rotation.map(|rotation| (rotation.is_y_relative(), rotation.is_x_relative())),
            same_world,
        );
        let target_rotation = target.rotation();
        let desired_rotation = resolved_rotation.unwrap_or(target_rotation);
        let transition = TeleportTransition {
            target_world: Arc::clone(source.world()),
            position: packet_position(position, target.position(), relatives),
            rotation: packet_rotation(desired_rotation, target_rotation, relatives),
            velocity: DVec3::ZERO,
            relatives,
            portal_cooldown: 0,
            as_passenger: false,
            post_transition: TeleportPostTransition::do_nothing(),
        };
        perform_teleport(source, target, transition, facing.as_ref());
    }

    send_position_success(source, targets, position);
    target_count(targets)
}

enum TeleportFacing {
    Position(DVec3),
    Entity {
        target: SharedEntity,
        anchor: EntityAnchor,
    },
}

fn perform_teleport(
    source: &CommandSource,
    target: &SharedEntity,
    transition: TeleportTransition,
    facing: Option<&TeleportFacing>,
) {
    if let Some(player) = target.as_player() {
        if player.is_sleeping() {
            let world = player.get_world();
            world.broadcast_to_entity_trackers(
                player.id(),
                CAnimate::new(player.id(), AnimateAction::WakeUp),
                None,
            );
            player.send_packet(CAnimate::new(player.id(), AnimateAction::WakeUp));
            player.stop_sleeping();
            player.set_pose(EntityPose::Standing);
            // TODO: Complete bed occupancy and sleep aggregation updates with the bed system.
        }
        player.send_packet(CSetCamera {
            camera_id: player.id(),
        });
    }
    if change_entity_world(Arc::clone(target), &transition).is_none() {
        return;
    }

    // Vanilla applies command post-effects through the original target reference.
    if let Some(facing) = facing {
        match facing {
            TeleportFacing::Position(position) => target.look_at(source.anchor(), *position),
            TeleportFacing::Entity {
                target: facing_target,
                anchor,
            } => target.look_at_entity(source.anchor(), facing_target.as_ref(), *anchor),
        }
    }

    if target
        .as_living_entity()
        .is_none_or(|living| !living.is_fall_flying())
    {
        let velocity = target.velocity();
        target.set_velocity(DVec3::new(velocity.x, 0.0, velocity.z));
        target.set_on_ground(true);
    }
    if let Some(pathfinder) = target.as_pathfinder_mob() {
        pathfinder.mob_base().navigation().lock().stop();
    }
}

fn ensure_spawnable_position(position: DVec3) -> Result<(), CommandSyntaxError> {
    if World::is_in_spawnable_bounds(BlockPos::from(position)) {
        return Ok(());
    }
    Err(CommandSyntaxError::dynamic(TextComponent::from(
        &translations::COMMANDS_TELEPORT_INVALID_POSITION,
    )))
}

fn ensure_same_domain_targets(
    targets: &[SharedEntity],
    target_world: &World,
) -> Result<(), CommandSyntaxError> {
    for target in targets {
        let Some(source_world) = target.level() else {
            continue;
        };
        ensure_same_domain(source_world.domain(), target_world.domain())?;
    }
    Ok(())
}

fn ensure_same_domain(source_domain: &str, target_domain: &str) -> Result<(), CommandSyntaxError> {
    if source_domain == target_domain {
        return Ok(());
    }
    Err(CommandSyntaxError::dynamic(
        "Entities cannot be teleported across Steel domains",
    ))
}

fn teleport_relatives(
    position_relative: (bool, bool, bool),
    rotation_relative: Option<(bool, bool)>,
    same_world: bool,
) -> RelativeMovement {
    let mut flags = 0;
    let (relative_x, relative_y, relative_z) = position_relative;
    if relative_x {
        flags |= RelativeMovement::DELTA_X;
        if same_world {
            flags |= RelativeMovement::X;
        }
    }
    if relative_y {
        flags |= RelativeMovement::DELTA_Y;
        if same_world {
            flags |= RelativeMovement::Y;
        }
    }
    if relative_z {
        flags |= RelativeMovement::DELTA_Z;
        if same_world {
            flags |= RelativeMovement::Z;
        }
    }

    let (relative_yaw, relative_pitch) = rotation_relative.unwrap_or((true, true));
    if relative_yaw {
        flags |= RelativeMovement::Y_ROT;
    }
    if relative_pitch {
        flags |= RelativeMovement::X_ROT;
    }
    RelativeMovement::new(flags)
}

fn packet_position(destination: DVec3, current: DVec3, relatives: RelativeMovement) -> DVec3 {
    DVec3::new(
        if relatives.is_x_relative() {
            destination.x - current.x
        } else {
            destination.x
        },
        if relatives.is_y_relative() {
            destination.y - current.y
        } else {
            destination.y
        },
        if relatives.is_z_relative() {
            destination.z - current.z
        } else {
            destination.z
        },
    )
}

fn packet_rotation(
    desired: (f32, f32),
    current: (f32, f32),
    relatives: RelativeMovement,
) -> (f32, f32) {
    wrap_rotation((
        if relatives.is_y_rot_relative() {
            desired.0 - current.0
        } else {
            desired.0
        },
        if relatives.is_x_rot_relative() {
            desired.1 - current.1
        } else {
            desired.1
        },
    ))
}

fn wrap_rotation((yaw, pitch): (f32, f32)) -> (f32, f32) {
    (wrap_degrees(yaw), wrap_degrees(pitch))
}

fn wrap_degrees(value: f32) -> f32 {
    let wrapped = value.rem_euclid(360.0);
    if wrapped >= 180.0 {
        wrapped - 360.0
    } else {
        wrapped
    }
}

fn send_entity_success(source: &CommandSource, targets: &[SharedEntity], destination: &dyn Entity) {
    let message = if let [target] = targets {
        translations::COMMANDS_TELEPORT_SUCCESS_ENTITY_SINGLE
            .message([
                TextComponent::plain(target.plain_text_name()),
                TextComponent::plain(destination.plain_text_name()),
            ])
            .component()
    } else {
        translations::COMMANDS_TELEPORT_SUCCESS_ENTITY_MULTIPLE
            .message([
                TextComponent::plain(targets.len().to_string()),
                TextComponent::plain(destination.plain_text_name()),
            ])
            .component()
    };
    source.send_success(&message, true);
}

fn send_position_success(source: &CommandSource, targets: &[SharedEntity], position: DVec3) {
    let [x, y, z] = [
        format!("{:.6}", position.x),
        format!("{:.6}", position.y),
        format!("{:.6}", position.z),
    ];
    let message = if let [target] = targets {
        translations::COMMANDS_TELEPORT_SUCCESS_LOCATION_SINGLE
            .message([
                TextComponent::plain(target.plain_text_name()),
                TextComponent::plain(x),
                TextComponent::plain(y),
                TextComponent::plain(z),
            ])
            .component()
    } else {
        translations::COMMANDS_TELEPORT_SUCCESS_LOCATION_MULTIPLE
            .message([
                TextComponent::plain(targets.len().to_string()),
                TextComponent::plain(x),
                TextComponent::plain(y),
                TextComponent::plain(z),
            ])
            .component()
    };
    source.send_success(&message, true);
}

fn target_count(targets: &[SharedEntity]) -> Result<i32, CommandSyntaxError> {
    i32::try_from(targets.len())
        .map_err(|_| CommandSyntaxError::dynamic("Target count exceeds the command result range"))
}

fn missing_argument(name: &str) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(format!(
        "Parsed value for {name} is missing from the command context"
    ))
}

#[cfg(test)]
mod tests {
    use super::super::create_dispatcher;
    use super::{ensure_same_domain, packet_position, packet_rotation, teleport_relatives};
    use crate::command::{
        brigadier::{CommandDispatcher, NodeId},
        execution::{CommandSource, SteelArgumentType, SteelCommandRuntime},
    };
    use glam::DVec3;
    use steel_protocol::packets::game::RelativeMovement;
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
    fn teleport_graph_matches_vanilla_target_and_facing_shapes() {
        init_test_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        for root_name in ["teleport", "tp"] {
            let root = child(&dispatcher, dispatcher.root(), root_name);
            let location = child(&dispatcher, root, "location");
            assert!(matches!(
                dispatcher.node(location),
                Some(node)
                    if node.is_executable()
                        && node.argument_type() == Some(&SteelArgumentType::vec3(true))
            ));

            let destination = child(&dispatcher, root, "destination");
            assert!(matches!(
                dispatcher.node(destination),
                Some(node)
                    if node.is_executable()
                        && node.argument_type() == Some(&SteelArgumentType::entity())
            ));

            let targets = child(&dispatcher, root, "targets");
            let target_location = child(&dispatcher, targets, "location");
            let rotation = child(&dispatcher, target_location, "rotation");
            assert!(matches!(
                dispatcher.node(rotation),
                Some(node)
                    if node.is_executable()
                        && node.argument_type() == Some(&SteelArgumentType::rotation())
            ));

            let facing = child(&dispatcher, target_location, "facing");
            let entity = child(&dispatcher, facing, "entity");
            let facing_entity = child(&dispatcher, entity, "facingEntity");
            let anchor = child(&dispatcher, facing_entity, "facingAnchor");
            assert!(matches!(
                dispatcher.node(anchor),
                Some(node)
                    if node.is_executable()
                        && node.argument_type() == Some(&SteelArgumentType::entity_anchor())
            ));
            let facing_location = child(&dispatcher, facing, "facingLocation");
            assert!(matches!(
                dispatcher.node(facing_location),
                Some(node) if node.is_executable()
            ));
        }
    }

    #[test]
    fn different_worlds_strip_relative_position_but_preserve_direction_flags() {
        let relatives = teleport_relatives((true, false, true), None, false);

        assert!(!relatives.is_x_relative());
        assert!(!relatives.is_z_relative());
        assert_eq!(
            relatives.0
                & (RelativeMovement::DELTA_X
                    | RelativeMovement::DELTA_Z
                    | RelativeMovement::Y_ROT
                    | RelativeMovement::X_ROT),
            RelativeMovement::DELTA_X
                | RelativeMovement::DELTA_Z
                | RelativeMovement::Y_ROT
                | RelativeMovement::X_ROT
        );
    }

    #[test]
    fn direct_teleport_rejects_cross_domain_transitions() {
        assert!(ensure_same_domain("survival", "survival").is_ok());
        assert!(ensure_same_domain("survival", "creative").is_err());
    }

    #[test]
    fn packet_values_rebase_source_relative_results_for_each_target() {
        let relatives = teleport_relatives((true, false, true), Some((true, false)), true);
        assert_eq!(
            packet_position(
                DVec3::new(20.0, 64.0, 40.0),
                DVec3::new(5.0, 10.0, 12.0),
                relatives,
            ),
            DVec3::new(15.0, 64.0, 28.0)
        );
        assert_eq!(
            packet_rotation((90.0, 30.0), (45.0, -10.0), relatives),
            (45.0, 30.0)
        );
    }
}
