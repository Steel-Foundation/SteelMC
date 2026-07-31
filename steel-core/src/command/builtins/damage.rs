//! Vanilla damage entity command.

use super::super::{
    brigadier::{ArgumentType, CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::entity::damage::DamageSource;
use steel_registry::vanilla_damage_types;
use steel_utils::Identifier;
use steel_utils::translations::{COMMANDS_DAMAGE_INVULNERABLE, COMMANDS_DAMAGE_SUCCESS};
use text_components::TextComponent;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("damage"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("damage").then(
        argument("target", SteelArgumentType::entity()).then(
            argument("amount", ArgumentType::float(0.0, f32::MAX))
                .executes(damage)
                .then(
                    argument("damageType", SteelArgumentType::damage_type())
                        .executes(damage)
                        .then(literal("at").then(
                            argument("location", SteelArgumentType::vec3(true)).executes(damage),
                        ))
                        .then(
                            literal("by").then(
                                argument("entity", SteelArgumentType::entity())
                                    .executes(damage)
                                    .then(
                                        literal("from").then(
                                            argument("cause", SteelArgumentType::entity())
                                                .executes(damage),
                                        ),
                                    ),
                            ),
                        ),
                ),
        ),
    )
}

fn damage(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let target = context.entity("target")?;
    let Some(amount) = context.float("amount") else {
        return Err(CommandSyntaxError::dynamic(
            "Parsed value for amount is missing from the command context",
        ));
    };

    // The base damage type for this command is generic
    let damage_type = context
        .damage_type("amount")
        .unwrap_or(&vanilla_damage_types::GENERIC);

    // Create the DamageSource to apply modifiers after
    let mut damage_source = DamageSource::environment(damage_type);

    // If we can get "location" from the context, it's from "at"
    if let Some(coordinates) = context.coordinates("location") {
        damage_source.source_position = Some(coordinates.position(context.source()));
    }

    // Else, it's from the "by", or maybe it's nothing
    if let Ok(entity) = context.entity("entity") {
        damage_source.direct_entity_id = Some(entity.id());

        // Maybe even the causing entity is known
        if let Ok(cause) = context.entity("cause") {
            damage_source.causing_entity_id = Some(cause.id());
        }
    }

    let Some(target_world) = target.level() else {
        return Err(CommandSyntaxError::dynamic(
            "The entity is not in a world or the world was dropped.",
        ));
    };

    if target.hurt(&target_world, &damage_source, amount) {
        context.source().send_success(
            &COMMANDS_DAMAGE_SUCCESS
                .message([
                    TextComponent::plain(format!("{:?}", amount)),
                    target.display_name(),
                ])
                .component(),
            true,
        );
        Ok(1)
    } else {
        context
            .source()
            .send_failure(COMMANDS_DAMAGE_INVULNERABLE.msg().component());
        Ok(0)
    }
}
