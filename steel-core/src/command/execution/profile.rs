//! Vanilla game-profile command arguments.

use steel_protocol::packets::game::{
    ArgumentType as ProtocolArgumentType, SuggestionType as ProtocolSuggestionType,
};
use steel_utils::{DowncastType, DowncastTypeKey, translations};
use text_components::TextComponent;
use uuid::Uuid;

use crate::command::brigadier::{
    CommandSyntaxError, CommandSyntaxErrorKind, StringReader, SuggestionsBuilder,
};

use super::{
    CommandArgumentSource, CommandSource,
    argument::{SteelArgumentParser, SteelArgumentSuggestionContext},
    selector::{EntitySelector, parse_entity_selector, suggest_entity_selector},
};

/// Resolved game profile used by permission administration commands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedGameProfile {
    pub(crate) uuid: Uuid,
    pub(crate) name: String,
}

/// Parsed vanilla game-profile argument.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum GameProfileArgument {
    /// Online players selected through an entity selector.
    Selector(Box<EntitySelector>),
    /// A direct profile name or UUID string.
    Direct(Box<str>),
}

impl GameProfileArgument {
    pub(crate) async fn resolve(
        self,
        source: &CommandSource,
    ) -> Result<Vec<ResolvedGameProfile>, CommandSyntaxError> {
        match self {
            Self::Selector(selector) => {
                let players = selector.find_players(source)?;
                if players.is_empty() {
                    return Err(CommandSyntaxError::dynamic(TextComponent::from(
                        &translations::ARGUMENT_ENTITY_NOTFOUND_PLAYER,
                    )));
                }
                Ok(players
                    .into_iter()
                    .map(|player| ResolvedGameProfile {
                        uuid: player.gameprofile.id,
                        name: player.gameprofile.name.clone(),
                    })
                    .collect())
            }
            Self::Direct(name) => {
                let profile = source
                    .server()
                    .resolve_player_profile(&name)
                    .await
                    .map_err(|error| CommandSyntaxError::dynamic(error.to_string()))?;
                Ok(vec![ResolvedGameProfile {
                    uuid: profile.uuid(),
                    name: profile.last_known_name().to_owned(),
                }])
            }
        }
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete parsed value.
unsafe impl DowncastType for GameProfileArgument {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:command/value/game_profile");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GameProfileSuggestionMode {
    All,
    NonOperators,
    Operators,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GameProfileParser {
    suggestion_mode: GameProfileSuggestionMode,
}

impl GameProfileParser {
    pub(super) const fn new(suggestion_mode: GameProfileSuggestionMode) -> Self {
        Self { suggestion_mode }
    }
}

// SAFETY: This Steel-owned key uniquely identifies the concrete parser.
unsafe impl DowncastType for GameProfileParser {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:command/parser/game_profile");
}

impl SteelArgumentParser for GameProfileParser {
    type Value = GameProfileArgument;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        source: &dyn CommandArgumentSource,
    ) -> Result<Self::Value, CommandSyntaxError> {
        if reader.peek() == Some('@') {
            return parse_entity_selector(reader, source, false, true)
                .map(Box::new)
                .map(GameProfileArgument::Selector);
        }

        let value = reader.read_unquoted_string();
        if value.is_empty() {
            return Err(reader.error(CommandSyntaxErrorKind::UnknownArgument));
        }
        Ok(GameProfileArgument::Direct(value.into()))
    }

    fn list_suggestions(
        &self,
        context: &dyn SteelArgumentSuggestionContext,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        suggest_entity_selector(builder, context.source(), false, true);
        let names = match self.suggestion_mode {
            GameProfileSuggestionMode::All => context.source().all_profile_names(),
            GameProfileSuggestionMode::NonOperators => {
                context.source().non_operator_profile_names()
            }
            GameProfileSuggestionMode::Operators => context.source().operator_profile_names(),
        };
        let prefix = builder.remaining_lowercase().to_owned();
        for name in names {
            if name.to_lowercase().starts_with(&prefix) {
                builder.suggest(name);
            }
        }
    }

    fn protocol_argument(&self) -> (ProtocolArgumentType, Option<ProtocolSuggestionType>) {
        (
            ProtocolArgumentType::GameProfile,
            Some(ProtocolSuggestionType::AskServer),
        )
    }
}
