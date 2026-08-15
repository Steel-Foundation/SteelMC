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
use crate::world::tick_scheduler::TickPriority;
use simdnbt::borrow::read_compound;
use std::io::Cursor;
use steel_registry::REGISTRY;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_utils::Identifier;
use steel_utils::translations::{COMMANDS_SETBLOCK_FAILED, COMMANDS_SETBLOCK_SUCCESS};
use steel_utils::types::UpdateFlags;
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
    // Block pos
    let coordinates = context.coordinates("pos")?;
    let block_pos = coordinates.block_pos(context.source());

    // Block predicate into block state
    let block_predicate = context.block_predicate("block")?;

    let (block_state, nbt) = match block_predicate {
        BlockPredicate::Block {
            block,
            properties,
            nbt,
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

            (block_state_id, nbt)
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

    // Save the old state to update the neighbors later
    let old_state = level.get_block_state(block_pos);

    // Place the block and update with the right flag
    if place_needed
        && !level.set_block(
            block_pos,
            block_state,
            UpdateFlags::from_bits_truncate(2 | update_bits),
        )
    {
        return Ok(set_block_failed(context.source()));
    }

    // Load the NBT data into the block entity if it exists
    if let Some(block_entity) = level.get_block_entity(block_pos)
        && let Some(nbt) = nbt
    {
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_compound(&mut Cursor::new(bytes.as_slice())).map_err(|_| {
            CommandSyntaxError::dynamic("Failed to transpose owned compound into borrowed ")
        })?;
        block_entity.load_additional(&borrowed);
        block_entity.set_changed();
    }

    if !matches!(mode, SetBlockMode::Strict) {
        level.update_neighbour_on_block_set(block_pos, old_state);
        if block_state.has_fluid() {
            level.schedule_fluid_tick(
                block_pos,
                block_state.get_fluid_state().fluid_id,
                0,
                TickPriority::ExtremelyHigh,
            );
        }
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

    // No block placed or replaced
    0
}
