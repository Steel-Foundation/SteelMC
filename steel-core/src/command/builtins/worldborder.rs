//! World border command.

use steel_utils::{Identifier, translations};
use text_components::{TextComponent, translation::Translation};

use super::super::{
    brigadier::{ArgumentType, CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::world::{MAX_CENTER_COORDINATE, MAX_SIZE, WorldBorderError};

const MIN_SIZE: f64 = 1.0;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("worldborder"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("worldborder")
        .then(size_command("add", add_size))
        .then(size_command("set", set_requested_size))
        .then(
            literal("center")
                .then(argument("pos", SteelArgumentType::vec2(true)).executes(set_center)),
        )
        .then(
            literal("damage")
                .then(
                    literal("amount").then(
                        argument("damagePerBlock", ArgumentType::float(0.0, f32::MAX))
                            .executes(set_damage_amount),
                    ),
                )
                .then(
                    literal("buffer").then(
                        argument("distance", ArgumentType::float(0.0, f32::MAX))
                            .executes(set_damage_buffer),
                    ),
                ),
        )
        .then(literal("get").executes(get_size))
        .then(
            literal("warning")
                .then(
                    literal("distance").then(
                        argument("distance", ArgumentType::integer(0, i32::MAX))
                            .executes(set_warning_distance),
                    ),
                )
                .then(
                    literal("time").then(
                        argument("time", SteelArgumentType::time(0)).executes(set_warning_time),
                    ),
                ),
        )
}

fn size_command(
    name: &'static str,
    execute: fn(
        &SteelCommandContext<CommandSource>,
        Option<i64>,
    ) -> Result<i32, CommandSyntaxError>,
) -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal(name).then(
        argument("distance", ArgumentType::double(-MAX_SIZE, MAX_SIZE))
            .executes(move |context| execute(context, None))
            .then(
                argument("time", SteelArgumentType::time(0)).executes(move |context| {
                    let ticks = i64::from(context.time("time")?);
                    execute(context, Some(ticks))
                }),
            ),
    )
}

fn add_size(
    context: &SteelCommandContext<CommandSource>,
    requested_ticks: Option<i64>,
) -> Result<i32, CommandSyntaxError> {
    let border = context.source().world().world_border_snapshot();
    let distance = border.old_size + context.double("distance")?;
    let ticks = added_lerp_time(border.lerp_time, requested_ticks);
    set_size(context, distance, ticks)
}

const fn added_lerp_time(current_ticks: i64, requested_ticks: Option<i64>) -> i64 {
    match requested_ticks {
        Some(ticks) => current_ticks.wrapping_add(ticks),
        None => 0,
    }
}

fn set_requested_size(
    context: &SteelCommandContext<CommandSource>,
    ticks: Option<i64>,
) -> Result<i32, CommandSyntaxError> {
    set_size(context, context.double("distance")?, ticks.unwrap_or(0))
}

#[expect(
    clippy::float_cmp,
    reason = "Vanilla rejects only exactly unchanged border sizes."
)]
fn set_size(
    context: &SteelCommandContext<CommandSource>,
    distance: f64,
    ticks: i64,
) -> Result<i32, CommandSyntaxError> {
    let world = context.source().world();
    let current = world.world_border_snapshot().old_size;
    if current == distance {
        return Err(translated_error(
            &translations::COMMANDS_WORLDBORDER_SET_FAILED_NOCHANGE,
        ));
    }
    if distance < MIN_SIZE {
        return Err(translated_error(
            &translations::COMMANDS_WORLDBORDER_SET_FAILED_SMALL,
        ));
    }
    if distance > MAX_SIZE {
        let message = translations::COMMANDS_WORLDBORDER_SET_FAILED_BIG
            .message([format!("{MAX_SIZE:.6E}")])
            .component();
        return Err(CommandSyntaxError::dynamic(message));
    }

    let formatted_distance = format!("{distance:.1}");
    let message = if ticks > 0 {
        world
            .lerp_world_border_size_between(current, distance, ticks)
            .map_err(internal_border_error)?;
        let seconds = format_ticks_to_seconds(ticks);
        if distance > current {
            translations::COMMANDS_WORLDBORDER_SET_GROW
                .message([formatted_distance, seconds])
                .component()
        } else {
            translations::COMMANDS_WORLDBORDER_SET_SHRINK
                .message([formatted_distance, seconds])
                .component()
        }
    } else {
        world
            .set_world_border_size(distance)
            .map_err(internal_border_error)?;
        translations::COMMANDS_WORLDBORDER_SET_IMMEDIATE
            .message([formatted_distance])
            .component()
    };
    context.source().send_success(&message, true);
    Ok((distance - current) as i32)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::float_cmp,
    reason = "Vanilla resolves Vec2 components as f32 and compares exact center values."
)]
fn set_center(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let position = context.coordinates("pos")?.position(context.source());
    let center_x = f64::from(position.x as f32);
    let center_z = f64::from(position.z as f32);
    let world = context.source().world();
    let border = world.world_border_snapshot();
    if border.center_x == center_x && border.center_z == center_z {
        return Err(translated_error(
            &translations::COMMANDS_WORLDBORDER_CENTER_FAILED,
        ));
    }
    if center_x.abs() > MAX_CENTER_COORDINATE || center_z.abs() > MAX_CENTER_COORDINATE {
        let message = translations::COMMANDS_WORLDBORDER_SET_FAILED_FAR
            .message([format!("{MAX_CENTER_COORDINATE:.7E}")])
            .component();
        return Err(CommandSyntaxError::dynamic(message));
    }

    world
        .set_world_border_center(center_x, center_z)
        .map_err(internal_border_error)?;
    let message = translations::COMMANDS_WORLDBORDER_CENTER_SUCCESS
        .message([format!("{center_x:.2}"), format!("{center_z:.2}")])
        .component();
    context.source().send_success(&message, true);
    Ok(0)
}

#[expect(
    clippy::float_cmp,
    reason = "Vanilla rejects only an exactly unchanged damage buffer."
)]
fn set_damage_buffer(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let distance = context.float("distance")?;
    let world = context.source().world();
    if world.world_border_snapshot().safe_zone == f64::from(distance) {
        return Err(translated_error(
            &translations::COMMANDS_WORLDBORDER_DAMAGE_BUFFER_FAILED,
        ));
    }
    world
        .set_world_border_safe_zone(f64::from(distance))
        .map_err(internal_border_error)?;
    let message = translations::COMMANDS_WORLDBORDER_DAMAGE_BUFFER_SUCCESS
        .message([format!("{distance:.2}")])
        .component();
    context.source().send_success(&message, true);
    Ok(distance as i32)
}

#[expect(
    clippy::float_cmp,
    reason = "Vanilla rejects only an exactly unchanged damage amount."
)]
fn set_damage_amount(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let damage = context.float("damagePerBlock")?;
    let world = context.source().world();
    if world.world_border_snapshot().damage_per_block == f64::from(damage) {
        return Err(translated_error(
            &translations::COMMANDS_WORLDBORDER_DAMAGE_AMOUNT_FAILED,
        ));
    }
    world
        .set_world_border_damage_per_block(f64::from(damage))
        .map_err(internal_border_error)?;
    let message = translations::COMMANDS_WORLDBORDER_DAMAGE_AMOUNT_SUCCESS
        .message([format!("{damage:.2}")])
        .component();
    context.source().send_success(&message, true);
    Ok(damage as i32)
}

fn set_warning_time(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let ticks = context.time("time")?;
    let world = context.source().world();
    if world.world_border_snapshot().warning_time == ticks {
        return Err(translated_error(
            &translations::COMMANDS_WORLDBORDER_WARNING_TIME_FAILED,
        ));
    }
    world.set_world_border_warning_time(ticks);
    let message = translations::COMMANDS_WORLDBORDER_WARNING_TIME_SUCCESS
        .message([format_ticks_to_seconds(i64::from(ticks))])
        .component();
    context.source().send_success(&message, true);
    Ok(ticks)
}

fn set_warning_distance(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let distance = context.integer("distance")?;
    let world = context.source().world();
    if world.world_border_snapshot().warning_blocks == distance {
        return Err(translated_error(
            &translations::COMMANDS_WORLDBORDER_WARNING_DISTANCE_FAILED,
        ));
    }
    world.set_world_border_warning_blocks(distance);
    let message = translations::COMMANDS_WORLDBORDER_WARNING_DISTANCE_SUCCESS
        .message([distance.to_string()])
        .component();
    context.source().send_success(&message, true);
    Ok(distance)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::unnecessary_wraps,
    reason = "Vanilla returns a signed result and command executors share a fallible signature."
)]
fn get_size(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let size = context.source().world().world_border_snapshot().old_size;
    let message = translations::COMMANDS_WORLDBORDER_GET
        .message([format!("{size:.0}")])
        .component();
    context.source().send_success(&message, false);
    Ok((size + 0.5).floor() as i32)
}

fn format_ticks_to_seconds(ticks: i64) -> String {
    format!("{:.2}", ticks as f64 / 20.0)
}

fn translated_error(translation: &Translation<0>) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(TextComponent::from(translation))
}

fn internal_border_error(error: WorldBorderError) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(TextComponent::from(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_CENTER_COORDINATE, MAX_SIZE, added_lerp_time, format_ticks_to_seconds, size_command,
    };
    use crate::command::{
        brigadier::{ArgumentType, CommandDispatcher},
        execution::{CommandSource, SteelArgumentType, SteelCommandRuntime},
    };

    #[test]
    fn formats_ticks_as_vanilla_seconds() {
        assert_eq!(format_ticks_to_seconds(0), "0.00");
        assert_eq!(format_ticks_to_seconds(31), "1.55");
    }

    #[test]
    fn formats_limits_like_vanilla_numeric_components() {
        assert_eq!(format!("{MAX_SIZE:.6E}"), "5.999997E7");
        assert_eq!(format!("{MAX_CENTER_COORDINATE:.7E}"), "2.9999984E7");
    }

    #[test]
    fn add_preserves_existing_lerp_only_when_time_is_explicit() {
        assert_eq!(added_lerp_time(40, None), 0);
        assert_eq!(added_lerp_time(40, Some(0)), 40);
        assert_eq!(added_lerp_time(40, Some(20)), 60);
    }

    #[test]
    fn size_subcommands_accept_optional_time() {
        let mut dispatcher = CommandDispatcher::<CommandSource, SteelCommandRuntime>::new();
        let Ok(root) = dispatcher.register(size_command("set", |_, _| Ok(1))) else {
            panic!("size command should register");
        };
        let Some(distance) = dispatcher
            .children(root)
            .and_then(|children| children.first())
        else {
            panic!("distance argument should exist");
        };
        assert_eq!(
            dispatcher
                .node(*distance)
                .and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::from(ArgumentType::double(
                -super::MAX_SIZE,
                super::MAX_SIZE,
            )))
        );
        let Some(time) = dispatcher
            .children(*distance)
            .and_then(|children| children.first())
        else {
            panic!("time argument should exist");
        };
        assert_eq!(
            dispatcher.node(*time).and_then(|node| node.argument_type()),
            Some(&SteelArgumentType::time(0))
        );
    }
}
