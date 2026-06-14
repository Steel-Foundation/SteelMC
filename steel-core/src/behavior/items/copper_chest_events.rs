use std::sync::Arc;

use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, ChestType};
use steel_registry::vanilla_game_events;
use steel_utils::{BlockPos, BlockStateId};

use crate::{
    entity::Entity,
    player::Player,
    world::{World, game_event_context::GameEventContext},
};

/// Emits the extra vanilla block-change notification for the other half of a double copper chest.
pub(super) fn emit_connected_chest_block_change(
    world: &Arc<World>,
    pos: BlockPos,
    old_state: BlockStateId,
    player: &Player,
    level_event: Option<i32>,
) {
    let Some(neighbor_pos) = connected_chest_pos(pos, old_state) else {
        return;
    };

    let neighbor_state = world.get_block_state(neighbor_pos);
    world.game_event(
        &vanilla_game_events::BLOCK_CHANGE,
        neighbor_pos,
        &GameEventContext::new(Some(player), Some(neighbor_state)),
    );

    if let Some(event) = level_event {
        world.level_event(event, neighbor_pos, 0, Some(player.id()));
    }
}

fn connected_chest_pos(pos: BlockPos, state: BlockStateId) -> Option<BlockPos> {
    let chest_type = state.try_get_value(&BlockStateProperties::CHEST_TYPE)?;
    if chest_type == ChestType::Single {
        return None;
    }

    let facing = state.try_get_value(&BlockStateProperties::FACING)?;
    let connected_direction = if chest_type == ChestType::Left {
        facing.rotate_y_clockwise()
    } else {
        facing.rotate_y_counter_clockwise()
    };

    Some(pos.relative(connected_direction))
}

#[cfg(test)]
mod tests {
    use steel_registry::blocks::block_state_ext::BlockStateExt;
    use steel_registry::blocks::properties::{BlockStateProperties, ChestType, Direction};
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_blocks;
    use steel_utils::BlockPos;

    use crate::behavior::items::copper_chest_events::connected_chest_pos;

    #[test]
    fn connected_chest_pos_matches_vanilla_left_and_right_offsets() {
        init_test_registry();

        let pos = BlockPos::new(10, 64, 10);
        let north_left = vanilla_blocks::COPPER_CHEST
            .default_state()
            .set_value(&BlockStateProperties::FACING, Direction::North)
            .set_value(&BlockStateProperties::CHEST_TYPE, ChestType::Left);
        let north_right = vanilla_blocks::COPPER_CHEST
            .default_state()
            .set_value(&BlockStateProperties::FACING, Direction::North)
            .set_value(&BlockStateProperties::CHEST_TYPE, ChestType::Right);

        assert_eq!(connected_chest_pos(pos, north_left), Some(pos.east()));
        assert_eq!(connected_chest_pos(pos, north_right), Some(pos.west()));
    }

    #[test]
    fn connected_chest_pos_ignores_single_chests() {
        init_test_registry();

        let pos = BlockPos::new(10, 64, 10);
        let single = vanilla_blocks::COPPER_CHEST
            .default_state()
            .set_value(&BlockStateProperties::CHEST_TYPE, ChestType::Single);

        assert_eq!(connected_chest_pos(pos, single), None);
    }
}
