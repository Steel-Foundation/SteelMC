//! Glass bottle item behavior (`BottleItem`).
//!
//! Filling from a water source produces a water potion via
//! `ItemUtils.createFilledResult`. Dragon-breath filling is omitted until
//! area-effect clouds exist.

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::data_components::PotionContents;
use steel_registry::data_components::vanilla_components::POTION_CONTENTS;
use steel_registry::item_stack::ItemStack;
use steel_registry::stat::vanilla_stat_types;
use steel_registry::{
    RegistryReference, sound_events, vanilla_game_events, vanilla_items, vanilla_potions,
};

use crate::behavior::context::{InteractionResult, UseItemContext};
use crate::behavior::item::ItemBehavior;
use crate::behavior::item_utils::{create_filled_result, get_player_pov_hit_result};
use crate::entity::Entity;
use crate::fluid::FluidStateExt;
use crate::world::ClipFluid;
use crate::world::game_event::GameEventContext;

/// Behavior for the glass bottle item.
#[item_behavior(class = "BottleItem")]
pub struct BottleItem;

impl ItemBehavior for BottleItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        // Vanilla `BottleItem.use` checks dragon-breath area-effect clouds first.
        // Steel has no `AreaEffectCloud` yet, so water filling is the live path.

        let hit = get_player_pov_hit_result(context.world, context.player, ClipFluid::SourceOnly);
        if hit.miss {
            return InteractionResult::Pass;
        }

        let pos = hit.block_pos;
        if !context.world.may_interact(context.player, pos) {
            return InteractionResult::Pass;
        }

        let fluid_state = context.world.get_block_state(pos).get_fluid_state();
        if !fluid_state.is_water() {
            return InteractionResult::Pass;
        }

        context.world.play_sound_at(
            &sound_events::ITEM_BOTTLE_FILL,
            SoundSource::Neutral,
            context.player.position(),
            1.0,
            1.0,
            Some(context.player.id()),
        );
        context.world.game_event(
            &vanilla_game_events::FLUID_PICKUP,
            pos,
            &GameEventContext::new(Some(context.player), None),
        );

        context.inv.with_item(|item| {
            context
                .player
                .award_stat(&vanilla_stat_types::ITEM_USED, item.item());
        });
        create_filled_result(context, water_potion_stack(), true);

        InteractionResult::Success
    }
}

/// Vanilla `PotionContents.createItemStack(Items.POTION, Potions.WATER)`.
fn water_potion_stack() -> ItemStack {
    let mut stack = ItemStack::new(&vanilla_items::POTION);
    stack.set(
        POTION_CONTENTS,
        PotionContents::new(
            Some(RegistryReference::new(&vanilla_potions::WATER)),
            None,
            Vec::new(),
            None,
        ),
    );
    stack
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::blocks::block_state_ext::BlockStateExt;
    use steel_registry::data_components::vanilla_components::POTION_CONTENTS;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_items, vanilla_potions};
    use steel_utils::types::{InteractionHand, UpdateFlags};
    use steel_utils::{BlockPos, BlockStateId, ChunkPos};

    use crate::behavior::item::ItemBehavior;
    use crate::behavior::{InteractionResult, UseItemContext, init_behaviors};
    use crate::entity::Entity;
    use crate::player::Player;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};
    use crate::world::World;

    use super::BottleItem;

    fn looking_down_at(
        world: &Arc<World>,
        feet: DVec3,
        target: BlockPos,
        state: BlockStateId,
    ) -> Arc<Player> {
        insert_ready_full_chunk(world, ChunkPos::from_block_pos(target));
        if world.get_block_state(target) != state {
            assert!(world.set_block(target, state, UpdateFlags::UPDATE_NONE));
        }

        let player = TestPlayerBuilder::new(Arc::clone(world), "BottleTester", 1).build();
        player
            .try_set_position(feet)
            .expect("test player should move onto the target chunk");
        player.set_rotation((0.0, 90.0));
        player
    }

    fn use_bottle(player: &Player, world: &Arc<World>, count: i32) -> InteractionResult {
        player.inventory.lock().set_item_in_hand(
            InteractionHand::MainHand,
            ItemStack::with_count(&vanilla_items::GLASS_BOTTLE, count),
        );
        let mut context = UseItemContext::new(
            player,
            InteractionHand::MainHand,
            world,
            player.inventory.clone(),
        );
        BottleItem.use_item(&mut context)
    }

    fn hand_item(player: &Player) -> ItemStack {
        player
            .inventory
            .lock()
            .get_item_in_hand(InteractionHand::MainHand)
            .clone()
    }

    #[test]
    fn fills_from_a_water_source_and_leaves_the_source() {
        init_vanilla_registry();
        init_behaviors();

        let world = fresh_test_world("bottle_fill_water");
        let water_pos = BlockPos::new(0, 80, 0);
        let player = looking_down_at(
            &world,
            DVec3::new(0.5, 81.0, 0.5),
            water_pos,
            vanilla_blocks::WATER.default_state(),
        );

        assert_eq!(use_bottle(&player, &world, 1), InteractionResult::Success);

        let filled = hand_item(&player);
        assert_eq!(filled.item.key, vanilla_items::POTION.key);
        assert!(
            filled
                .get(POTION_CONTENTS)
                .is_some_and(|contents| contents.is(&vanilla_potions::WATER))
        );
        assert_eq!(
            world.get_block_state(water_pos).get_block(),
            &vanilla_blocks::WATER
        );
    }

    #[test]
    fn non_water_blocks_and_misses_do_not_fill() {
        init_vanilla_registry();
        init_behaviors();

        let world = fresh_test_world("bottle_fill_stone");
        let player = looking_down_at(
            &world,
            DVec3::new(0.5, 81.0, 0.5),
            BlockPos::new(0, 80, 0),
            vanilla_blocks::STONE.default_state(),
        );

        assert_eq!(use_bottle(&player, &world, 1), InteractionResult::Pass);
        assert_eq!(hand_item(&player).item.key, vanilla_items::GLASS_BOTTLE.key);

        let air_world = fresh_test_world("bottle_fill_air");
        let air_player = looking_down_at(
            &air_world,
            DVec3::new(0.5, 81.0, 0.5),
            BlockPos::new(0, 80, 0),
            vanilla_blocks::AIR.default_state(),
        );
        assert_eq!(
            use_bottle(&air_player, &air_world, 1),
            InteractionResult::Pass
        );
    }
}
