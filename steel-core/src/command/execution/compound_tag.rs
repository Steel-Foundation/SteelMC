//! Compound Tag command arguments.

use crate::command::brigadier::{CommandSyntaxError, CommandSyntaxErrorKind, StringReader};
use simdnbt::owned::NbtCompound;
use steel_utils::nbt::parse_snbt_compound_argument;
use text_components::TextComponent;

pub(super) fn parse_compound_tag(
    reader: &mut StringReader<'_>,
) -> Result<NbtCompound, CommandSyntaxError> {
    match parse_snbt_compound_argument(reader.remaining()) {
        Ok((compound, consumed)) => {
            if !reader.advance_bytes(consumed) {
                return Err(dynamic_error(reader, "Invalid SNBT compound cursor"));
            }
            Ok(compound)
        }
        Err(error) => {
            if !reader.advance_bytes(error.cursor()) {
                return Err(dynamic_error(reader, "Invalid SNBT compound cursor"));
            }
            Err(dynamic_error(reader, error.component()))
        }
    }
}

fn dynamic_error(
    reader: &StringReader<'_>,
    message: impl Into<TextComponent>,
) -> CommandSyntaxError {
    reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(message.into())))
}
