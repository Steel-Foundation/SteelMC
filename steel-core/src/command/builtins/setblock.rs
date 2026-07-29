//! Vanilla set block command.

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use steel_utils::Identifier;
use steel_utils::types::UpdateFlags;
use text_components::TextComponent;

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("setblock"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("setblock").then(
        argument("pos", SteelArgumentType::block_pos()).then(
            argument("block", SteelArgumentType::block())
                .executes(set_block)
                .then(literal("destroy").executes(set_block))
                .then(literal("keep").executes(set_block))
                .then(literal("replace").executes(set_block))
                .then(literal("strict").executes(set_block)),
        ),
    )
}

/// Set a block in the desired position with a mode (destroy, keep, replace, strict), and return 1 if the block os placed, 0 else.
fn set_block(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    let Some(coordinates) = context.coordinates("pos") else {
        return Err(missing_argument("pos"));
    };


    let Some(block) = context.block("block") else {
        return Err(missing_argument("block"));
    };

    context.source().world().set_block(
        coordinates.block_pos(context.source()),
        block.default_state(),
        UpdateFlags::UPDATE_ALL,
    );

    Ok(1)
}

fn missing_argument(name: &str) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(format!(
        "Parsed value for {name} is missing from the command context"
    ))
}
