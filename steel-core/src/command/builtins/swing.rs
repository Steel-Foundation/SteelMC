//! swing animation command

use std::slice;

use steel_registry::data_components::vanilla_components::SwingAnimation;
use steel_utils::{Identifier, translations, types::InteractionHand};
use text_components::TextComponent;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::entity::SharedEntity;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("swing"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("swing").executes(swing_self).then(
        argument("targets", SteelArgumentType::entities())
            .executes(|context| {
                swing_targets(context, InteractionHand::MainHand, SwingAnimation::DEFAULT)
            })
            .then(hand_swing("mainhand", InteractionHand::MainHand))
            .then(hand_swing("offhand", InteractionHand::OffHand)),
    )
}

fn hand_swing(
    name: &'static str,
    hand: InteractionHand,
) -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal(name)
        .executes(move |context| swing_targets(context, hand, SwingAnimation::DEFAULT))
        .then(
            argument("animation", SteelArgumentType::swing_animation())
                .executes(move |context| {
                    let animation = SwingAnimation::new(
                        context.swing_animation("animation")?,
                        SwingAnimation::DEFAULT.duration,
                    );
                    swing_targets(context, hand, animation)
                })
                .then(
                    argument("duration", SteelArgumentType::time(1)).executes(move |context| {
                        let animation = SwingAnimation::new(
                            context.swing_animation("animation")?,
                            context.time("duration")?,
                        );
                        swing_targets(context, hand, animation)
                    }),
                ),
        )
}

fn swing_self(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let Some(entity) = context.source().entity() else {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::PERMISSIONS_REQUIRES_ENTITY,
        )));
    };
    let (result, message) = swing_entities(
        slice::from_ref(entity),
        InteractionHand::MainHand,
        SwingAnimation::DEFAULT,
    )?;
    context.source().send_success(&message, true);
    Ok(result)
}

fn swing_targets(
    context: &SteelCommandContext<CommandSource>,
    hand: InteractionHand,
    animation: SwingAnimation,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.entities("targets")?;
    let (result, message) = swing_entities(&targets, hand, animation)?;
    context.source().send_success(&message, true);
    Ok(result)
}

fn swing_entities(
    targets: &[SharedEntity],
    hand: InteractionHand,
    animation: SwingAnimation,
) -> Result<(i32, TextComponent), CommandSyntaxError> {
    let living_targets = targets
        .iter()
        .filter_map(|target| target.as_living_entity())
        .collect::<Vec<_>>();

    if living_targets.is_empty() {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_SWING_FAILED_NOTLIVING,
        )));
    }

    let result = i32::try_from(living_targets.len()).map_err(|_| {
        CommandSyntaxError::dynamic("Target count exceeds the command result range")
    })?;

    for living in &living_targets {
        living.swing(hand, animation, true);
    }

    let message = if let [target] = living_targets.as_slice() {
        translations::COMMANDS_SWING_SUCCESS_SINGLE
            .message([target.display_name()])
            .component()
    } else {
        translations::COMMANDS_SWING_SUCCESS_MULTIPLE
            .message([result.to_string()])
            .component()
    };

    Ok((result, message))
}
