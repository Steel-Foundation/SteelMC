//! NBT path command arguments.

use steel_utils::nbt::{NbtPath, parse_nbt_path_argument as parse_path};
use text_components::TextComponent;

use crate::command::brigadier::{CommandSyntaxError, CommandSyntaxErrorKind, StringReader};

pub(super) fn parse_nbt_path(reader: &mut StringReader<'_>) -> Result<NbtPath, CommandSyntaxError> {
    match parse_path(reader.remaining()) {
        Ok((path, consumed)) => {
            advance_reader_bytes(reader, consumed)?;
            Ok(path)
        }
        Err(error) => {
            advance_reader_bytes(reader, error.cursor())?;
            Err(dynamic_error(reader, error.message()))
        }
    }
}

fn advance_reader_bytes(
    reader: &mut StringReader<'_>,
    bytes: usize,
) -> Result<(), CommandSyntaxError> {
    let Some(consumed) = reader.remaining().get(..bytes) else {
        return Err(dynamic_error(reader, "Invalid NBT path cursor"));
    };
    for _ in consumed.chars() {
        reader.skip();
    }
    Ok(())
}

fn dynamic_error(reader: &StringReader<'_>, message: impl Into<String>) -> CommandSyntaxError {
    reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(
        TextComponent::from(message.into()),
    )))
}
