use std::sync::Arc;

use steel_protocol::packets::game::{CStopSound, SoundSource};
use steel_utils::{Identifier, translations};
use text_components::TextComponent;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};

use crate::player::Player;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("stopsound"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    let mut targets =
        argument("targets", SteelArgumentType::players()).executes(stop_all_for_targets);

    for source in SoundSource::VALUES {
        targets = targets.then(source_command(source));
    }

    targets = targets.then(
        literal("*")
            .then(argument("sound", SteelArgumentType::sound()).executes(stop_sound_any_source)),
    );

    literal("stopsound").then(targets)
}

fn source_command(source: SoundSource) -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal(source.name())
        .executes(move |context| stop_all_for_targets_with_source(context, source))
        .then(
            argument("sound", SteelArgumentType::sound())
                .executes(move |context| stop_sound(context, source)),
        )
}

fn stop_all_for_targets(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;

    execute(None, None, &targets)?;

    context.source().send_success(
        &TextComponent::from(&translations::COMMANDS_STOPSOUND_SUCCESS_SOURCELESS_ANY),
        true,
    );

    target_count(&targets)
}

fn stop_all_for_targets_with_source(
    context: &SteelCommandContext<CommandSource>,
    source: SoundSource,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;

    execute(Some(source), None, &targets)?;

    let message = translations::COMMANDS_STOPSOUND_SUCCESS_SOURCE_ANY
        .message([TextComponent::plain(source.name())])
        .component();

    context.source().send_success(&message, true);

    target_count(&targets)
}

fn stop_sound_any_source(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    let sound = context.identifier("sound")?.clone();

    execute(None, Some(&sound), &targets)?;

    let message = translations::COMMANDS_STOPSOUND_SUCCESS_SOURCELESS_SOUND
        .message([TextComponent::plain(sound.to_string())])
        .component();

    context.source().send_success(&message, true);

    target_count(&targets)
}

fn stop_sound(
    context: &SteelCommandContext<CommandSource>,
    source: SoundSource,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    let sound = context.identifier("sound")?.clone();

    execute(Some(source), Some(&sound), &targets)?;

    let message = translations::COMMANDS_STOPSOUND_SUCCESS_SOURCE_SOUND
        .message([
            TextComponent::plain(sound.to_string()),
            TextComponent::plain(source.name()),
        ])
        .component();

    context.source().send_success(&message, true);

    target_count(&targets)
}

fn execute(
    source: Option<SoundSource>,
    sound: Option<&Identifier>,
    targets: &[Arc<Player>],
) -> Result<(), CommandSyntaxError> {
    for target in targets {
        target.send_packet(CStopSound::new(source, sound.cloned()));
    }

    Ok(())
}

fn target_count(targets: &[Arc<Player>]) -> Result<i32, CommandSyntaxError> {
    i32::try_from(targets.len()).map_err(|_| {
        CommandSyntaxError::dynamic("Target player count exceeds the command result range")
    })
}
