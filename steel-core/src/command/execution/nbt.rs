//! NBT path command arguments.

use steel_utils::nbt::{NbtPath, parse_nbt_path_argument as parse_path};
use text_components::TextComponent;

use crate::command::brigadier::{CommandSyntaxError, CommandSyntaxErrorKind, StringReader};

pub(super) fn parse_nbt_path(reader: &mut StringReader<'_>) -> Result<NbtPath, CommandSyntaxError> {
    match parse_path(reader.remaining()) {
        Ok((path, consumed)) => {
            if !reader.advance_bytes(consumed) {
                return Err(dynamic_error(reader, "Invalid NBT path cursor"));
            }
            Ok(path)
        }
        Err(error) => {
            if !reader.advance_bytes(error.cursor()) {
                return Err(dynamic_error(reader, "Invalid NBT path cursor"));
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
