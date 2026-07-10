//! `/execute` operations that transform or fork the command source.

use steel_utils::translations;
use text_components::TextComponent;

use super::super::super::{
    brigadier::{CommandNodeBuilder, CommandRedirectTarget, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
};
use super::super::summon;
use crate::entity::{EntityAnchor, SharedEntity};

type Builder = CommandNodeBuilder<CommandSource, SteelCommandRuntime>;

const EXECUTE_ROOT: CommandRedirectTarget = CommandRedirectTarget::CommandRoot;

pub(super) fn as_operation() -> Builder {
    literal("as").then(argument("targets", SteelArgumentType::entities()).forks(
        EXECUTE_ROOT,
        |context| {
            Ok(context
                .optional_entities("targets")?
                .into_iter()
                .map(|entity| context.source().with_entity(entity))
                .collect())
        },
    ))
}

pub(super) fn at_operation() -> Builder {
    literal("at").then(argument("targets", SteelArgumentType::entities()).forks(
        EXECUTE_ROOT,
        |context| {
            context
                .optional_entities("targets")?
                .into_iter()
                .map(|entity| {
                    let world = entity.level().ok_or_else(|| {
                        CommandSyntaxError::dynamic("Selected entity is not in a loaded world")
                    })?;
                    Ok(context
                        .source()
                        .with_world(world)
                        .with_position(entity.position())
                        .with_rotation(entity.rotation()))
                })
                .collect::<Result<Vec<_>, CommandSyntaxError>>()
        },
    ))
}

pub(super) fn positioned_operation() -> Builder {
    literal("positioned")
        .then(
            argument("pos", SteelArgumentType::vec3(true)).redirects_with(
                EXECUTE_ROOT,
                |context: &SteelCommandContext<CommandSource>| {
                    let position = required_coordinates(context, "pos")?.position(context.source());
                    Ok(context
                        .source()
                        .with_position(position)
                        .with_anchor(EntityAnchor::Feet))
                },
            ),
        )
        .then(
            literal("as").then(argument("targets", SteelArgumentType::entities()).forks(
                EXECUTE_ROOT,
                |context| {
                    Ok(context
                        .optional_entities("targets")?
                        .into_iter()
                        .map(|entity| context.source().with_position(entity.position()))
                        .collect())
                },
            )),
        )
        .then(literal("over").then(
            argument("heightmap", SteelArgumentType::heightmap()).redirects_with(
                EXECUTE_ROOT,
                |context: &SteelCommandContext<CommandSource>| {
                    let source = context.source();
                    let position = source.position();
                    let heightmap = context
                        .heightmap("heightmap")
                        .ok_or_else(|| missing_argument("heightmap"))?;
                    let Some(height) = source.world().height_at(
                        heightmap,
                        position.x.floor() as i32,
                        position.z.floor() as i32,
                    ) else {
                        return Err(CommandSyntaxError::dynamic(TextComponent::from(
                            &translations::ARGUMENT_POS_UNLOADED,
                        )));
                    };
                    Ok(source.with_position(position.with_y(f64::from(height))))
                },
            ),
        ))
}

pub(super) fn rotated_operation() -> Builder {
    literal("rotated")
        .then(
            argument("rot", SteelArgumentType::rotation()).redirects_with(
                EXECUTE_ROOT,
                |context| {
                    let rotation = required_coordinates(context, "rot")?.rotation(context.source());
                    Ok(context.source().with_rotation(rotation))
                },
            ),
        )
        .then(
            literal("as").then(argument("targets", SteelArgumentType::entities()).forks(
                EXECUTE_ROOT,
                |context| {
                    Ok(context
                        .optional_entities("targets")?
                        .into_iter()
                        .map(|entity| context.source().with_rotation(entity.rotation()))
                        .collect())
                },
            )),
        )
}

pub(super) fn facing_operation() -> Builder {
    literal("facing")
        .then(
            literal("entity").then(argument("targets", SteelArgumentType::entities()).then(
                argument("anchor", SteelArgumentType::entity_anchor()).forks(
                    EXECUTE_ROOT,
                    |context| {
                        let anchor = context
                            .entity_anchor("anchor")
                            .ok_or_else(|| missing_argument("anchor"))?;
                        Ok(context
                            .optional_entities("targets")?
                            .into_iter()
                            .map(|entity| {
                                context
                                    .source()
                                    .facing_position(anchor.position(entity.as_ref()))
                            })
                            .collect())
                    },
                ),
            )),
        )
        .then(
            argument("pos", SteelArgumentType::vec3(true)).redirects_with(
                EXECUTE_ROOT,
                |context| {
                    let position = required_coordinates(context, "pos")?.position(context.source());
                    Ok(context.source().facing_position(position))
                },
            ),
        )
}

pub(super) fn align_operation() -> Builder {
    literal("align").then(
        argument("axes", SteelArgumentType::swizzle()).redirects_with(
            EXECUTE_ROOT,
            |context: &SteelCommandContext<CommandSource>| {
                let axes = context
                    .swizzle("axes")
                    .ok_or_else(|| missing_argument("axes"))?;
                Ok(context
                    .source()
                    .with_position(axes.align(context.source().position())))
            },
        ),
    )
}

pub(super) fn anchored_operation() -> Builder {
    literal("anchored").then(
        argument("anchor", SteelArgumentType::entity_anchor()).redirects_with(
            EXECUTE_ROOT,
            |context: &SteelCommandContext<CommandSource>| {
                let anchor = context
                    .entity_anchor("anchor")
                    .ok_or_else(|| missing_argument("anchor"))?;
                Ok(context.source().with_anchor(anchor))
            },
        ),
    )
}

pub(super) fn in_operation() -> Builder {
    literal("in").then(
        argument("dimension", SteelArgumentType::world()).redirects_with(
            EXECUTE_ROOT,
            |context: &SteelCommandContext<CommandSource>| {
                let world = context
                    .world_argument("dimension")
                    .ok_or_else(|| missing_argument("dimension"))?
                    .resolve(context.source())?;
                Ok(context.source().with_world(world))
            },
        ),
    )
}

pub(super) fn summon_operation() -> Builder {
    literal("summon").then(
        argument("entity", SteelArgumentType::summonable_entity()).redirects_with(
            EXECUTE_ROOT,
            |context| {
                let entity_type = context
                    .entity_type("entity")
                    .ok_or_else(|| missing_argument("entity"))?;
                let entity =
                    summon::create_entity(context, entity_type, context.source().position())?;
                Ok(context.source().with_entity(entity))
            },
        ),
    )
}

pub(super) fn on_relations() -> Builder {
    literal("on")
        .then(literal("vehicle").forks(EXECUTE_ROOT, |context| {
            Ok(one_relation_sources(context.source(), |entity| {
                entity.vehicle()
            }))
        }))
        .then(literal("controller").forks(EXECUTE_ROOT, |context| {
            Ok(one_relation_sources(context.source(), |entity| {
                entity.controlling_passenger()
            }))
        }))
        .then(literal("passengers").forks(EXECUTE_ROOT, |context| {
            Ok(passenger_sources(context.source()))
        }))
}

fn one_relation_sources(
    source: &CommandSource,
    relation: impl FnOnce(&SharedEntity) -> Option<SharedEntity>,
) -> Vec<CommandSource> {
    let entity = source.entity().and_then(relation);
    entity
        .filter(|entity| !entity.is_removed())
        .map_or_else(Vec::new, |entity| vec![source.with_entity(entity)])
}

fn passenger_sources(source: &CommandSource) -> Vec<CommandSource> {
    source.entity().map_or_else(Vec::new, |entity| {
        entity
            .passengers()
            .into_iter()
            .filter(|passenger| !passenger.is_removed())
            .map(|passenger| source.with_entity(passenger))
            .collect()
    })
}

fn required_coordinates(
    context: &SteelCommandContext<CommandSource>,
    name: &str,
) -> Result<super::super::super::execution::Coordinates, CommandSyntaxError> {
    context
        .coordinates(name)
        .ok_or_else(|| missing_argument(name))
}

fn missing_argument(name: &str) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(format!(
        "Parsed value for {name} is missing from the command context"
    ))
}
