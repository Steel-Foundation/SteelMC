use std::ops::Range;

use steel_utils::translations::ARGUMENT_MESSAGE_TOO_LONG;
use text_components::{Modifier, TextComponent, format::Color};

use super::{
    CommandArgumentSource, CommandSource,
    selector::{EntitySelector, parse_message_selector},
    text::join_components,
};
use crate::command::brigadier::{CommandSyntaxError, StringReader};

const MAX_MESSAGE_LENGTH: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Message {
    text: Box<str>,
    parts: Vec<(Range<usize>, EntitySelector)>,
}

impl Message {
    pub(super) fn parse(
        reader: &mut StringReader<'_>,
        source: &dyn CommandArgumentSource,
    ) -> Result<Self, CommandSyntaxError> {
        let length = reader.remaining_length();
        if length > MAX_MESSAGE_LENGTH {
            return Err(CommandSyntaxError::dynamic(
                ARGUMENT_MESSAGE_TOO_LONG
                    .message([length.to_string(), MAX_MESSAGE_LENGTH.to_string()])
                    .component(),
            ));
        }

        let text = reader.remaining();
        let start = reader.byte_cursor();
        let mut parts = Vec::new();

        if source.allows_entity_selectors() {
            let allow_advanced = source.allows_advanced_entity_selectors();
            while reader.can_read() {
                let offset = reader.byte_cursor() - start;

                if let Some(selector) = parse_message_selector(reader, allow_advanced)? {
                    parts.push((offset..reader.byte_cursor() - start, selector));
                } else {
                    reader.skip();
                }
            }
        } else {
            reader.read_remaining();
        }

        Ok(Self {
            text: text.into(),
            parts,
        })
    }

    pub(crate) fn resolve_component(
        &self,
        source: &CommandSource,
    ) -> Result<TextComponent, CommandSyntaxError> {
        let Some((first, _)) = self
            .parts
            .first()
            .filter(|_| source.allows_entity_selectors())
        else {
            return Ok(TextComponent::plain(self.text.to_string()));
        };

        let separator = TextComponent::const_plain(", ").color(Color::Gray);
        let mut component = TextComponent::plain(self.text[..first.start].to_owned());
        let mut offset = first.start;

        for (range, selector) in &self.parts {
            if offset < range.start {
                component.children.push(TextComponent::plain(
                    self.text[offset..range.start].to_owned(),
                ));
            }

            let entities = selector.find_entities(source)?;
            component.children.push(join_components(
                entities.iter().map(|entity| entity.display_name()),
                &separator,
            ));

            offset = range.end;
        }

        if offset < self.text.len() {
            component
                .children
                .push(TextComponent::plain(self.text[offset..].to_owned()));
        }

        Ok(component)
    }
}
