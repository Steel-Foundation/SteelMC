//! Vanilla set block command.

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::world::LevelAccessor;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_utils::Identifier;
use steel_utils::translations::{COMMANDS_SETBLOCK_FAILED, COMMANDS_SETBLOCK_SUCCESS};
use steel_utils::types::UpdateFlags;
use text_components::TextComponent;

/// How the block should be placed
enum SetBlockMode {
    /// Destroy the previous block and drop the loot according to the loot tabke
    Destroy,
    /// Can only place a block if the previous block was air
    Keep,
    /// Base case
    Replace,
    /// Replace the block without updating the world
    Strict,
}

pub(super) fn registration() -> CommandRegistration<CommandSource> {
    CommandRegistration::new(Identifier::vanilla_static("setblock"), |_| command())
}

fn command() -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("setblock").then(
        argument("pos", SteelArgumentType::block_pos()).then(
            argument("block", SteelArgumentType::block_state())
                .executes(set_block_replace)
                .then(literal("destroy").executes(set_block_destroy))
                .then(literal("keep").executes(set_block_keep))
                .then(literal("replace").executes(set_block_replace))
                .then(literal("strict").executes(set_block_strict)),
        ),
    )
}

fn set_block_destroy(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    set_block(context, SetBlockMode::Destroy)
}

fn set_block_keep(context: &SteelCommandContext<CommandSource>) -> Result<i32, CommandSyntaxError> {
    set_block(context, SetBlockMode::Keep)
}

fn set_block_replace(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    set_block(context, SetBlockMode::Replace)
}

fn set_block_strict(
    context: &SteelCommandContext<CommandSource>,
) -> Result<i32, CommandSyntaxError> {
    set_block(context, SetBlockMode::Strict)
}

/// Set a block in the desired position with a mode (destroy, keep, replace, strict), and return 1 if the block os placed, 0 else.
fn set_block(
    context: &SteelCommandContext<CommandSource>,
    mode: SetBlockMode,
) -> Result<i32, CommandSyntaxError> {
    // Block pos
    let Some(coordinates) = context.coordinates("pos") else {
        return Err(missing_argument("pos"));
    };
    let block_pos = coordinates.block_pos(context.source());

    // Block
    let Some(block) = context.block_state("block") else {
        return Err(missing_argument("block"));
    };

    // World the player is in
    let level = context.source().world();

    // Keep mode throw an error when you try to replace an air block
    if let SetBlockMode::Keep = mode
        && level.get_block_state(block_pos).is_air()
    {
        return Ok(set_block_failed(context.source()));
    }

    let place_needed = if let SetBlockMode::Destroy = mode {
        level.destroy_block(block_pos, true);

        !block.is_air() || level.get_block_state(block_pos).is_air()
    } else {
        true
    };

    let update_bits = if let SetBlockMode::Strict = mode {
        816
    } else {
        256
    };
    if place_needed
        && !level.set_block_state(
            block_pos,
            block,
            UpdateFlags::from_bits_truncate(2 | update_bits),
        )
    {
        return Ok(set_block_failed(context.source()));
    }

    if !matches!(mode, SetBlockMode::Strict) {
        level.update_neighbors_at(block_pos, block.get_block());
    }

    context.source().send_success(
        &COMMANDS_SETBLOCK_SUCCESS
            .message([
                TextComponent::plain(format!("{}", block_pos.x())),
                TextComponent::plain(format!("{}", block_pos.y())),
                TextComponent::plain(format!("{}", block_pos.z())),
            ])
            .component(),
        true,
    );

    Ok(1)
}

fn missing_argument(name: &str) -> CommandSyntaxError {
    CommandSyntaxError::dynamic(format!(
        "Parsed value for {name} is missing from the command context"
    ))
}

fn set_block_failed(source: &CommandSource) -> i32 {
    source.send_failure(COMMANDS_SETBLOCK_FAILED.msg().component());

    // No block placed or replaced
    0
}
