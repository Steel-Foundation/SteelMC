use crate::behavior::block::drop_from_block_interact_loot_table;
use crate::behavior::{
    BlockBehavior, BlockPlaceContext, BlockRef, InteractionResult, InventoryAccess,
};
use crate::entity::Entity;
use crate::player::Player;
use crate::world::World;
use crate::world::game_event::GameEventContext;
use glam::DVec3;
use rand::RngExt;
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, EnumProperty};
use steel_registry::items::item::BlockHitResult;
use steel_registry::stat::vanilla_stat_types;
use steel_registry::{
    sound_events, vanilla_blocks, vanilla_game_events, vanilla_items, vanilla_loot_tables,
};
use steel_utils::axis::Axis;
use steel_utils::types::{InteractionHand, UpdateFlags};
use steel_utils::{BlockPos, BlockStateId};

/// Behavior for pumpkins.
#[block_behavior]
pub struct PumpkinBlock {
    block: BlockRef,
}

const HORIZONTAL_FACING: &EnumProperty<Direction> = &BlockStateProperties::HORIZONTAL_FACING;

impl PumpkinBlock {
    /// Creates a pumpkin block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for PumpkinBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn use_item_on(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hand: InteractionHand,
        hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let mut rng = rand::rng();

        let Some(shears) = inv.with_item(|item_stack| {
            item_stack
                .is(&vanilla_items::SHEARS)
                .then(|| item_stack.clone())
        }) else {
            return InteractionResult::TryEmptyHandInteraction;
        };

        let drops = drop_from_block_interact_loot_table(
            &vanilla_loot_tables::CARVE_PUMPKIN,
            state,
            world.get_block_entity(pos),
            Some(&shears),
            Some(player),
            &mut rng,
        );

        let clicked_direction = hit_result.direction;
        let direction = if clicked_direction.axis() == Axis::Y {
            player.direction_yaw().opposite()
        } else {
            clicked_direction
        };

        let (x_offset, z_offset) = {
            let (x, _, z) = direction.offset();
            (f64::from(x), f64::from(z))
        };

        for drop in drops {
            world.spawn_item_with_velocity(
                DVec3::new(
                    f64::from(pos.x()) + 0.5 + x_offset * 0.65,
                    f64::from(pos.y()) + 0.1,
                    f64::from(pos.z()) + 0.5 + z_offset * 0.65,
                ),
                drop,
                DVec3::new(
                    rng.random_range(0.05 * x_offset..0.05 * x_offset + 0.02),
                    0.05,
                    rng.random_range(0.05 * z_offset..0.05 * z_offset + 0.02),
                ),
            );
        }

        world.play_block_sound(&sound_events::BLOCK_PUMPKIN_CARVE, pos, 1.0, 1.0, None);
        world.set_block(
            pos,
            vanilla_blocks::CARVED_PUMPKIN
                .default_state()
                .set_value(HORIZONTAL_FACING, direction),
            UpdateFlags::UPDATE_IMMEDIATE
                | UpdateFlags::UPDATE_CLIENTS
                | UpdateFlags::UPDATE_NEIGHBORS,
        );
        let has_infinite_materials = player.has_infinite_materials();
        inv.with_item(|item_stack| item_stack.hurt_and_break(1, has_infinite_materials));

        world.game_event(
            &vanilla_game_events::SHEAR,
            pos,
            &GameEventContext::new(Some(player), None),
        );

        player.award_stat(&vanilla_stat_types::ITEM_USED, &vanilla_items::SHEARS);

        InteractionResult::Success
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use steel_registry::item_stack::ItemStack;
    use steel_utils::{ChunkPos, Downcast as _};

    use super::*;
    use crate::bootstrap::init_globals;
    use crate::entity::entities::ItemEntity;
    use crate::test_support::{
        DROPPED_ITEM_SEARCH_SIZE, TestPlayerBuilder, dropped_items, fresh_test_world,
        insert_ready_full_chunk,
    };

    const CARVE_TIMEOUT: Duration = Duration::from_secs(5);

    #[test]
    fn carving_with_shears_drops_seeds_and_damages_shears() {
        init_globals();
        let world = fresh_test_world("pumpkin_carving");
        let pos = BlockPos::new(8, 64, 8);
        let _holder = insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let state = vanilla_blocks::PUMPKIN.default_state();
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
        let player =
            TestPlayerBuilder::new(Arc::clone(&world), "PumpkinCarver".to_owned(), 1).build();
        player.inventory.lock().set_item_in_hand(
            InteractionHand::MainHand,
            ItemStack::new(&vanilla_items::SHEARS),
        );

        let (sender, receiver) = mpsc::channel();
        {
            let world = Arc::clone(&world);
            let player = Arc::clone(&player);
            thread::spawn(move || {
                let mut inv =
                    InventoryAccess::new(Arc::clone(&player.inventory), InteractionHand::MainHand);
                let hit_result = BlockHitResult {
                    location: DVec3::new(8.5, 64.5, 8.0),
                    direction: Direction::North,
                    block_pos: pos,
                    miss: false,
                    inside: false,
                    world_border_hit: false,
                };
                let result = PumpkinBlock::new(&vanilla_blocks::PUMPKIN).use_item_on(
                    state,
                    &world,
                    pos,
                    &player,
                    InteractionHand::MainHand,
                    &hit_result,
                    &mut inv,
                );
                let _ = sender.send(result);
            });
        }
        let Ok(result) = receiver.recv_timeout(CARVE_TIMEOUT) else {
            panic!("carving a pumpkin should not deadlock on the player's inventory lock");
        };

        assert_eq!(result, InteractionResult::Success);
        let carved = world.get_block_state(pos);
        assert_eq!(carved.get_block(), &vanilla_blocks::CARVED_PUMPKIN);
        assert_eq!(carved.get_value(HORIZONTAL_FACING), Direction::North);
        assert_eq!(
            player
                .inventory
                .lock()
                .get_item_in_hand(InteractionHand::MainHand)
                .get_damage_value(),
            1
        );

        let dropped = dropped_items(
            &world,
            DVec3::new(8.5, 64.0 + DROPPED_ITEM_SEARCH_SIZE / 2.0, 8.5),
        );
        assert_eq!(dropped.len(), 1);
        let Some(seeds) = dropped[0].downcast_ref::<ItemEntity>() else {
            panic!("carved pumpkin drop should be an item entity");
        };
        let seeds = seeds.get_item();
        assert!(seeds.is(&vanilla_items::PUMPKIN_SEEDS));
        assert_eq!(seeds.count, 4);
    }
}
