//! Vanilla set block command.

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal, placement_flags,
    },
    registration::CommandRegistration,
};
use super::execute::loaded_block_position;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_utils::Identifier;
use steel_utils::translations::{COMMANDS_SETBLOCK_FAILED, COMMANDS_SETBLOCK_SUCCESS};
use text_components::TextComponent;

/// How the block should be placed
enum SetBlockMode {
    /// Destroy the previous block and drop the loot according to the loot table
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
                .executes(|c| set_block(c, SetBlockMode::Replace))
                .then(literal("destroy").executes(|c| set_block(c, SetBlockMode::Destroy)))
                .then(literal("keep").executes(|c| set_block(c, SetBlockMode::Keep)))
                .then(literal("replace").executes(|c| set_block(c, SetBlockMode::Replace)))
                .then(literal("strict").executes(|c| set_block(c, SetBlockMode::Strict))),
        ),
    )
}

/// Set a block in the desired position with a mode (destroy, keep, replace, strict), and return 1 if the block is placed, 0 else.
fn set_block(
    context: &SteelCommandContext<CommandSource>,
    mode: SetBlockMode,
) -> Result<i32, CommandSyntaxError> {
    let block_pos = loaded_block_position(context, "pos")?;
    let block = context.block_input("block")?;
    let level = context.source().world();

    if matches!(mode, SetBlockMode::Keep) && !level.get_block_state(block_pos).is_air() {
        return Ok(set_block_failed(context.source()));
    }

    let place_needed = if matches!(mode, SetBlockMode::Destroy) {
        level.destroy_block(block_pos, true);

        !block.state().is_air() || !level.get_block_state(block_pos).is_air()
    } else {
        true
    };

    let strict = matches!(mode, SetBlockMode::Strict);
    let old_state = level.get_block_state(block_pos);

    if place_needed && !block.place(level, block_pos, placement_flags(strict))? {
        return Ok(set_block_failed(context.source()));
    }

    if !strict {
        level.update_neighbors_on_block_set(block_pos, old_state);
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

fn set_block_failed(source: &CommandSource) -> i32 {
    source.send_failure(COMMANDS_SETBLOCK_FAILED.msg().component());

    0
}
