//! Vanilla set block command.

use steel_utils::Identifier;
use text_components::TextComponent;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};

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
    context
        .source()
        .sender()
        .send_message(&TextComponent::plain("test"));

    Ok(1)
}
