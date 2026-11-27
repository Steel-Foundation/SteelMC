use steel_protocol::packets::game::{ArgumentType, SuggestionType};
use steel_registry::data_components::vanilla_components::SwingAnimationType;
use steel_utils::{DowncastType, DowncastTypeKey, translations};

use super::{
    CommandArgumentSource, SteelArgumentParser, SteelArgumentSuggestionContext, suggest_list,
};
use crate::command::brigadier::{
    CommandSyntaxError, CommandSyntaxErrorKind, StringReader, SuggestionsBuilder,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SwingAnimationParser;

// SAFETY: this key is steel owned and uniqely identifies this concrete parser
unsafe impl DowncastType for SwingAnimationParser {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:command/parser/swing_animation");
}

#[derive(Debug)]
pub(super) struct SwingAnimationValue(pub(super) SwingAnimationType);

// SAFETY: this key is steel owned and uniqely identifies this concrete parser
unsafe impl DowncastType for SwingAnimationValue {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:command/value/swing_animation");
}

impl SteelArgumentParser for SwingAnimationParser {
    type Value = SwingAnimationValue;

    fn parse(
        &self,
        reader: &mut StringReader<'_>,
        _source: &dyn CommandArgumentSource,
    ) -> Result<Self::Value, CommandSyntaxError> {
        let name = reader.read_unquoted_string();
        SwingAnimationType::from_serialized_name(name)
            .map(SwingAnimationValue)
            .ok_or_else(|| {
                let message = translations::ARGUMENT_SWING_ANIMATION_INVALID
                    .message([name.to_owned()])
                    .component();
                reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message)))
            })
    }

    fn list_suggestions(
        &self,
        _context: &dyn SteelArgumentSuggestionContext,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        suggest_list(
            builder,
            [
                SwingAnimationType::None,
                SwingAnimationType::Whack,
                SwingAnimationType::Stab,
            ]
            .map(SwingAnimationType::serialized_name),
        );
    }

    fn protocol_argument(&self) -> (ArgumentType, Option<SuggestionType>) {
        (ArgumentType::SwingAnimation, None)
    }
}
