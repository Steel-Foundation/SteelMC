//! Vanilla set block command.

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandSource, SteelArgumentType, SteelCommandContext, SteelCommandRuntime, argument,
        literal,
    },
    registration::CommandRegistration,
};
use crate::command::execution::BlockPredicate;
use steel_registry::REGISTRY;
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
            argument("block", SteelArgumentType::block_predicate())
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

    // Block predicate into block state
    let Some(block_predicate) = context.block_predicate("block") else {
        return Err(missing_argument("block"));
    };

    let block_state = match block_predicate {
        BlockPredicate::Block {
            // TODO: use nbt with container for example
            block, properties, ..
        } => {
            // Adapt properties for the next function
            let properties_vec: Vec<(&str, &str)> = properties
                .iter()
                .map(|(name, value)| (name.as_ref(), value.as_ref()))
                .collect();

            // Get the block state id
            let Some(block_state_id) = REGISTRY
                .blocks
                .state_id_from_block_properties(block, &properties_vec)
            else {
                return Err(CommandSyntaxError::dynamic(
                    "This Block is not registered or a property name/value is invalid.",
                ));
            };

            block_state_id
        }
        BlockPredicate::Tag { .. } => unreachable!(),
    };

    // World the player is in
    let level = context.source().world();

    // Keep mode throw an error when you try to replace an air block
    if matches!(mode, SetBlockMode::Keep) && level.get_block_state(block_pos).is_air() {
        return Ok(set_block_failed(context.source()));
    }

    let place_needed = if matches!(mode, SetBlockMode::Destroy) {
        level.destroy_block(block_pos, true);

        !block_state.is_air() || level.get_block_state(block_pos).is_air()
    } else {
        true
    };

    let update_bits = if matches!(mode, SetBlockMode::Strict) {
        816
    } else {
        256
    };
    if place_needed
        && !level.set_block(
            block_pos,
            block_state,
            UpdateFlags::from_bits_truncate(2 | update_bits),
        )
    {
        return Ok(set_block_failed(context.source()));
    }

    if !matches!(mode, SetBlockMode::Strict) {
        /// TODO: use vanilla "updateNeighboursOnBlockSet"
        level.update_neighbors_at(block_pos, block_state.get_block());
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
