//! Conversion utilities between Flint types and `SteelMC` types.

use flint_core::Block;
use flint_core::test_spec::GameruleValue;
use rustc_hash::FxHashMap;
use steel_core::behavior::BlockHitResult;
use steel_core::world::ClipHitResult;
use steel_registry::game_rules::{ErasedGameRuleRef, GameRuleType, GameRuleValue};
use steel_registry::{REGISTRY, RegistryExt};
use steel_utils::{BlockPos as SteelBlockPos, BlockStateId, Identifier};

/// Convert a Flint block specification to a `SteelMC` `BlockStateId`.
///
/// Returns `None` if the block ID is unknown or if any property is invalid.
pub fn flint_block_to_state_id(block: &Block) -> Option<BlockStateId> {
    let block_id = block.id.strip_prefix("minecraft:").unwrap_or(&block.id);
    let identifier = Identifier::vanilla(block_id.to_string());

    let block_ref = REGISTRY.blocks.by_key(&identifier)?;

    REGISTRY.blocks.state_id_from_block_defaulted_properties(
        block_ref,
        block
            .properties
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str())),
    )
}

/// Convert a `SteelMC` `BlockStateId` to Flint `Block`.
pub fn state_id_to_block(state_id: BlockStateId) -> Block {
    let Some(block) = REGISTRY.blocks.by_state_id(state_id) else {
        return Block::new("minecraft:air");
    };

    let id = if block.key.path == "void_air" || block.key.path == "cave_air" {
        "minecraft:air".to_owned()
    } else {
        format!("minecraft:{}", block.key.path)
    };

    #[allow(clippy::disallowed_types)]
    let properties: FxHashMap<String, String> = REGISTRY
        .blocks
        .properties(state_id)
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    Block::with_properties(id, properties)
}

/// Convert Flint `BlockPos` to `SteelMC` `BlockPos`.
pub const fn flint_pos_to_steel(pos: flint_core::BlockPos) -> SteelBlockPos {
    SteelBlockPos::new(pos[0], pos[1], pos[2])
}

/// Convert a Flint gamerule value into the type the Steel rule expects.
pub fn flint_gamerule_value(
    rule: ErasedGameRuleRef,
    value: GameruleValue,
) -> anyhow::Result<GameRuleValue> {
    match (rule.value_type(), value) {
        (GameRuleType::Bool, GameruleValue::Bool(value)) => Ok(GameRuleValue::new(value)),
        (GameRuleType::Int, GameruleValue::Integer(value)) => {
            i32::try_from(value).map(GameRuleValue::new).map_err(|_| {
                anyhow::anyhow!("Gamerule '{}' value {value} out of i32 range", rule.key())
            })
        }
        (expected, value) => Err(anyhow::anyhow!(
            "Gamerule '{}' expects {expected:?}, got '{value}'",
            rule.key()
        )),
    }
}

/// Convert a `SteelMC` [`ClipHitResult`] to a [`BlockHitResult`].
///
/// The two types carry identical data; `World::clip` returns the former while
/// the interaction API (`use_item_on`) takes the latter.
pub const fn clip_to_block_hit(clip: ClipHitResult) -> BlockHitResult {
    BlockHitResult {
        location: clip.location,
        direction: clip.direction,
        block_pos: clip.block_pos,
        miss: clip.miss,
        inside: clip.inside,
        world_border_hit: clip.world_border_hit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_round_trip_with_and_without_prefix() {
        crate::init();
        for id in ["minecraft:stone", "stone"] {
            let state_id = flint_block_to_state_id(&Block::new(id)).expect("known block");
            assert_eq!(state_id_to_block(state_id).id, "minecraft:stone");
        }
    }
}
