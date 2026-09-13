//! Vanilla sound playback command.

use std::sync::Arc;

use steel_protocol::packets::game::SoundSource;
use steel_registry::sound_event::SoundEventHolder;
use steel_utils::{Identifier, translations};
use text_components::TextComponent;

use super::super::{
    brigadier::{ArgumentType, CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::{entity::Entity as _, player::Player};

const SOUND_SOURCES: [SoundSource; 11] = [
    SoundSource::Master,
    SoundSource::Music,
    SoundSource::Records,
    SoundSource::Weather,
    SoundSource::Blocks,
    SoundSource::Hostile,
    SoundSource::Neutral,
    SoundSource::Players,
    SoundSource::Ambient,
    SoundSource::Voice,
    SoundSource::Ui,
];

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("playsound"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    let mut sound = argument("sound", SteelArgumentType::sound())
        .executes(|context| execute_as_source(context, SoundSource::Master));
    for source in SOUND_SOURCES {
        sound = sound.then(source_command(source));
    }
    literal("playsound").then(sound)
}

fn source_command(source: SoundSource) -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    let min_volume = argument("minVolume", ArgumentType::float(0.0, 1.0))
        .executes(move |context| execute_for_targets(context, source));
    let pitch = argument("pitch", ArgumentType::float(0.0, 2.0))
        .executes(move |context| execute_for_targets(context, source))
        .then(min_volume);
    let volume = argument("volume", ArgumentType::float(0.0, f32::MAX))
        .executes(move |context| execute_for_targets(context, source))
        .then(pitch);
    let pos = argument("pos", SteelArgumentType::vec3(true))
        .executes(move |context| execute_for_targets(context, source))
        .then(volume);
    let targets = argument("targets", SteelArgumentType::players())
        .executes(move |context| execute_for_targets(context, source))
        .then(pos);

    literal(source.name())
        .executes(move |context| execute_as_source(context, source))
        .then(targets)
}

fn execute_as_source(
    context: &SteelCommandContext<CommandSource>,
    source: SoundSource,
) -> Result<i32, CommandSyntaxError> {
    let targets = context
        .source()
        .player()
        .map_or_else(Vec::new, |player| vec![Arc::clone(player)]);
    execute(context, source, &targets)
}

fn execute_for_targets(
    context: &SteelCommandContext<CommandSource>,
    source: SoundSource,
) -> Result<i32, CommandSyntaxError> {
    let targets = context.players("targets")?;
    execute(context, source, &targets)
}

fn execute(
    context: &SteelCommandContext<CommandSource>,
    source: SoundSource,
    targets: &[Arc<Player>],
) -> Result<i32, CommandSyntaxError> {
    let sound_id = context.identifier("sound")?.clone();
    let sound = SoundEventHolder::Direct {
        sound_id: sound_id.clone(),
        fixed_range: None,
    };
    let position = match context.coordinates("pos") {
        Ok(coordinates) => coordinates.position(context.source()),
        Err(_) => context.source().position(),
    };
    let volume = context.float("volume").unwrap_or(1.0);
    let pitch = context.float("pitch").unwrap_or(1.0);
    let min_volume = context.float("minVolume").unwrap_or(0.0);

    let played_for = context
        .source()
        .world()
        .play_sound_to_players(&sound, source, position, volume, pitch, min_volume, targets);
    if played_for.is_empty() {
        return Err(CommandSyntaxError::dynamic(TextComponent::from(
            &translations::COMMANDS_PLAYSOUND_FAILED,
        )));
    }

    let message = if let [target] = played_for.as_slice() {
        translations::COMMANDS_PLAYSOUND_SUCCESS_SINGLE
            .message([
                TextComponent::plain(sound_id.to_string()),
                target.display_name(),
            ])
            .component()
    } else {
        translations::COMMANDS_PLAYSOUND_SUCCESS_MULTIPLE
            .message([
                TextComponent::plain(sound_id.to_string()),
                TextComponent::plain(played_for.len().to_string()),
            ])
            .component()
    };
    context.source().send_success(&message, true);

    i32::try_from(played_for.len()).map_err(|_| {
        CommandSyntaxError::dynamic("Played-for player count exceeds the command result range")
    })
}

#[cfg(test)]
mod tests {
    use steel_protocol::packets::game::{ArgumentType as ProtocolArgumentType, SuggestionType};
    use steel_registry::init_vanilla_registry;

    use super::super::create_dispatcher;
    use crate::command::brigadier::{ArgumentType, CommandDispatcher, NodeId};
    use crate::command::execution::CommandSource;
    use crate::command::execution::{SteelArgumentType, SteelCommandRuntime};

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

    #[expect(
        clippy::redundant_closure_for_method_calls,
        reason = "the command node type is intentionally private to the Brigadier module"
    )]
    #[test]
    fn playsound_graph_matches_vanilla_shape() {
        init_vanilla_registry();
        let Ok(dispatcher) = create_dispatcher() else {
            panic!("built-in commands should register");
        };
        let playsound = child(&dispatcher, dispatcher.root(), "playsound");
        let Some(playsound_node) = dispatcher.node(playsound) else {
            panic!("playsound root should exist");
        };
        assert!(playsound_node.is_restricted());
        assert!(!playsound_node.is_executable());

        let sound = child(&dispatcher, playsound, "sound");
        let Some(sound_node) = dispatcher.node(sound) else {
            panic!("sound argument should exist");
        };
        assert!(sound_node.is_executable());
        assert_eq!(
            sound_node.argument_type(),
            Some(&SteelArgumentType::sound())
        );
        let (protocol_argument, suggestions) = SteelArgumentType::sound().protocol_argument();
        assert!(matches!(
            protocol_argument,
            ProtocolArgumentType::ResourceLocation
        ));
        assert!(matches!(suggestions, Some(SuggestionType::AvailableSounds)));

        let source_names = [
            "master", "music", "record", "weather", "block", "hostile", "neutral", "player",
            "ambient", "voice", "ui",
        ];
        for source_name in source_names {
            let source = child(&dispatcher, sound, source_name);
            let Some(source_node) = dispatcher.node(source) else {
                panic!("sound source should exist");
            };
            assert!(source_node.is_executable());

            let targets = child(&dispatcher, source, "targets");
            assert_eq!(
                dispatcher
                    .node(targets)
                    .and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::players())
            );
            assert!(
                dispatcher
                    .node(targets)
                    .is_some_and(|node| node.is_executable())
            );

            let pos = child(&dispatcher, targets, "pos");
            assert_eq!(
                dispatcher.node(pos).and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::vec3(true))
            );
            assert!(
                dispatcher
                    .node(pos)
                    .is_some_and(|node| node.is_executable())
            );

            let volume = child(&dispatcher, pos, "volume");
            assert_eq!(
                dispatcher
                    .node(volume)
                    .and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::from(
                    ArgumentType::float(0.0, f32::MAX,)
                ))
            );
            assert!(
                dispatcher
                    .node(volume)
                    .is_some_and(|node| node.is_executable())
            );

            let pitch = child(&dispatcher, volume, "pitch");
            assert_eq!(
                dispatcher.node(pitch).and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::from(ArgumentType::float(0.0, 2.0,)))
            );
            assert!(
                dispatcher
                    .node(pitch)
                    .is_some_and(|node| node.is_executable())
            );

            let min_volume = child(&dispatcher, pitch, "minVolume");
            assert_eq!(
                dispatcher
                    .node(min_volume)
                    .and_then(|node| node.argument_type()),
                Some(&SteelArgumentType::from(ArgumentType::float(0.0, 1.0,)))
            );
            assert!(
                dispatcher
                    .node(min_volume)
                    .is_some_and(|node| node.is_executable())
            );
        }
    }
}
