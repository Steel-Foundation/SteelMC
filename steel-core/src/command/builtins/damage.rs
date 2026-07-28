//! Vanilla damage entity command.

use super::super::{
    brigadier::{ArgumentType, CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use steel_utils::Identifier;
use text_components::TextComponent;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("damage"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("damage").then(
        argument("targets", SteelArgumentType::players()).then(
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
    let targets = context.players("targets")?;

    for target in &targets {
        target.send_message(&TextComponent::plain("test"))
    }

    i32::try_from(targets.len()).map_err(|_| {
        CommandSyntaxError::dynamic("Target player count exceeds the command result range")
    })
}
