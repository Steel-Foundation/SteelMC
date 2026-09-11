use std::borrow::Cow;
use std::sync::Arc;

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::{
    blocks::block_state_ext::BlockStateExt,
    data_components::{
        components::{GlobalPos, LodestoneTracker},
        vanilla_components::LODESTONE_TRACKER,
    },
    item_stack::ItemStack,
    sound_events, vanilla_blocks,
};
use text_components::TextComponent;

use crate::behavior::{InteractionResult, ItemBehavior, UseOnContext};
use crate::inventory::container::Container;
use crate::player::Player;
use crate::world::World;

use super::dynamic_name::{default_name, translated};

/// Compass item behavior implementing lodestone binding and validation.
#[item_behavior]
pub struct CompassItem;

impl ItemBehavior for CompassItem {
    fn get_name<'a>(&self, stack: &'a ItemStack) -> Cow<'a, TextComponent> {
        if stack.has(LODESTONE_TRACKER) {
            translated("item.minecraft.lodestone_compass".to_owned(), None)
        } else {
            default_name(stack)
        }
    }

    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let pos = context.hit_result.block_pos;
        let block_state = context.world.get_block_state(pos);

        if block_state.get_block() != &vanilla_blocks::LODESTONE {
            return InteractionResult::Pass;
        }

        context.world.play_sound(
            &sound_events::ITEM_LODESTONE_COMPASS_LOCK,
            SoundSource::Players,
            pos,
            1.0,
            1.0,
            None,
        );

        let dimension = context.world.key.clone();
        let tracker = LodestoneTracker::new(Some(GlobalPos::new(dimension, pos)), true);

        let has_infinite_materials = context.player.has_infinite_materials();

        let leftover = context.inv.with_inventory(|inv| {
            let held_count = inv.get_item_in_hand(context.hand).count();

            if !has_infinite_materials && held_count == 1 {
                inv.mutate_item_in_hand(context.hand, |item| item.set(LODESTONE_TRACKER, tracker));
                return ItemStack::empty();
            }

            let mut result_stack = inv.get_item_in_hand(context.hand).clone();
            result_stack.set_count(1);
            result_stack.set(LODESTONE_TRACKER, tracker);

            inv.mutate_item_in_hand(context.hand, |item | item.consume_one(has_infinite_materials));

            if inv.add(&mut result_stack) {
                ItemStack::empty()
            } else {
                result_stack
            }
        });

        if !leftover.is_empty() {
            let _ = context.player.drop_item(leftover, false, false);
        }

        InteractionResult::Success
    }

    fn inventory_tick(
        &self,
        stack: &mut ItemStack,
        world: &Arc<World>,
        _player: &Player,
        _slot: usize,
        _selected: bool,
    ) {
        let Some(tracker) = stack.get(LODESTONE_TRACKER) else {
            return;
        };

        if !tracker.tracked() {
            return;
        }

        let Some(target) = tracker.target() else {
            return;
        };

        if world.key != *target.dimension() {
            return;
        }

        let target_pos = target.pos();
        if !world.is_in_world_bounds(target_pos) {
            let new_tracker = LodestoneTracker::new(None, true);
            stack.set(LODESTONE_TRACKER, new_tracker);
            return;
        }

        if !world.is_full_chunk_loaded_at(target_pos) {
            return;
        }

        let block_state = world.get_block_state(target_pos);
        if block_state.get_block() != &vanilla_blocks::LODESTONE {
            let new_tracker = LodestoneTracker::new(None, true);
            stack.set(LODESTONE_TRACKER, new_tracker);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap::init_globals_once;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};
    use glam::DVec3;
    use steel_registry::blocks::properties::Direction;
    use steel_registry::items::item::BlockHitResult;
    use steel_registry::vanilla_items;
    use steel_utils::types::InteractionHand;
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos};
    use uuid::Uuid;

    #[test]
    fn compass_binds_to_lodestone_and_invalidates_when_removed() {
        init_globals_once();

        let world = fresh_test_world("compass_test_world");
        let pos = BlockPos::new(0, 64, 0);
        let chunk_pos = ChunkPos::from_block_pos(pos);
        insert_ready_full_chunk(&world, chunk_pos);

        assert!(world.set_block(
            pos,
            vanilla_blocks::LODESTONE.default_state(),
            UpdateFlags::UPDATE_ALL
        ));

        let player = TestPlayerBuilder::new(Arc::clone(&world), "CompassTester", 1)
            .uuid(Uuid::from_u128(1))
            .build();

        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::COMPASS));

        let hit_result = BlockHitResult {
            location: DVec3::new(0.5, 64.5, 0.5),
            direction: Direction::Up,
            block_pos: pos,
            miss: false,
            inside: false,
            world_border_hit: false,
        };
        let mut context = UseOnContext::new(
            &player,
            InteractionHand::MainHand,
            hit_result,
            &world,
            player.inventory.clone(),
        );

        let behavior = CompassItem;

        let result = behavior.use_on(&mut context);
        assert_eq!(result, InteractionResult::Success);

        let mut compass = player.inventory.lock().get_selected_item().clone();
        assert!(!compass.is_empty());
        let tracker = compass
            .get(LODESTONE_TRACKER)
            .expect("should have lodestone tracker component");
        assert!(tracker.tracked());
        let target = tracker.target().expect("should have target global pos");
        assert_eq!(target.pos(), pos);
        assert_eq!(*target.dimension(), world.key);

        behavior.inventory_tick(&mut compass, &world, &player, 0, true);
        let tracker = compass
            .get(LODESTONE_TRACKER)
            .expect("should keep lodestone tracker component");
        assert!(tracker.target().is_some());

        assert!(world.set_block(
            pos,
            vanilla_blocks::AIR.default_state(),
            UpdateFlags::UPDATE_ALL
        ));
        behavior.inventory_tick(&mut compass, &world, &player, 0, true);
        let tracker = compass
            .get(LODESTONE_TRACKER)
            .expect("should keep lodestone tracker component");
        assert!(
            tracker.target().is_none(),
            "target should be invalidated and set to None"
        );
    }

    #[test]
    fn compass_invalidates_target_outside_world_bounds() {
        init_globals_once();

        let world = fresh_test_world("compass_bounds_test_world");
        let player = TestPlayerBuilder::new(Arc::clone(&world), "CompassTester", 1)
            .uuid(Uuid::from_u128(1))
            .build();

        let invalid_pos = BlockPos::new(40_000_000, 64, 0);
        let tracker =
            LodestoneTracker::new(Some(GlobalPos::new(world.key.clone(), invalid_pos)), true);

        let mut compass = ItemStack::new(&vanilla_items::COMPASS);
        compass.set(LODESTONE_TRACKER, tracker);

        let behavior = CompassItem;
        behavior.inventory_tick(&mut compass, &world, &player, 0, true);

        let tracker = compass
            .get(LODESTONE_TRACKER)
            .expect("should keep lodestone tracker component");
        assert!(
            tracker.target().is_none(),
            "target should be immediately invalidated and set to None"
        );
    }
}
