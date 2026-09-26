use steel_macros::item_behavior;
use steel_registry::{
    blocks::{
        Block,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, BoolProperty},
    },
    level_events, sound_events,
    vanilla_block_tags::BlockTag,
    vanilla_blocks, vanilla_game_events,
};
use steel_utils::Direction;
use steel_utils::types::UpdateFlags;

use crate::{
    behavior::{InteractionResult, ItemBehavior, UseOnContext},
    entity::Entity,
    world::game_event::GameEventContext,
};

const FLATTENABLES: [&Block; 6] = [
    &vanilla_blocks::GRASS_BLOCK,
    &vanilla_blocks::DIRT,
    &vanilla_blocks::PODZOL,
    &vanilla_blocks::COARSE_DIRT,
    &vanilla_blocks::MYCELIUM,
    &vanilla_blocks::ROOTED_DIRT,
];

const LIT_PROPERTY: BoolProperty = BlockStateProperties::LIT;

/// Behavior for Shovels, extinguishes campfires and turns grass blocks into paths
#[item_behavior]
pub struct ShovelItem;

impl ItemBehavior for ShovelItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        if context.hit_result.direction == Direction::Down {
            return InteractionResult::Pass;
        }

        let block_state = context.world.get_block_state(context.hit_result.block_pos);
        let block = block_state.get_block();

        // Flattenables — vanilla checks these first
        if FLATTENABLES.contains(&block) {
            if !context
                .world
                .get_block_state(context.hit_result.block_pos.above())
                .is_air()
            {
                return InteractionResult::Pass;
            }
            context.world.play_block_sound(
                &sound_events::ITEM_SHOVEL_FLATTEN,
                context.hit_result.block_pos,
                1.0,
                1.0,
                Some(context.player.id()),
            );
            let infinite_materials = context.player.has_infinite_materials();
            context
                .inv
                .with_item(|item| item.hurt_and_break(1, infinite_materials));
            let updated_state = vanilla_blocks::DIRT_PATH.default_state();
            context.world.set_block(
                context.hit_result.block_pos,
                updated_state,
                UpdateFlags::UPDATE_ALL_IMMEDIATE,
            );
            context.world.game_event(
                &vanilla_game_events::BLOCK_CHANGE,
                context.hit_result.block_pos,
                &GameEventContext::new(Some(context.player), Some(updated_state)),
            );
            return InteractionResult::Success;
        }

        // Campfire extinguishing
        if block.has_tag(&BlockTag::CAMPFIRES) {
            if !block_state.get_value(&LIT_PROPERTY) {
                return InteractionResult::Pass;
            }
            context.world.level_event(
                level_events::SOUND_EXTINGUISH_FIRE,
                context.hit_result.block_pos,
                0,
                None,
            );
            let updated_state = block_state.set_value(&LIT_PROPERTY, false);
            context.world.set_block(
                context.hit_result.block_pos,
                updated_state,
                UpdateFlags::UPDATE_ALL_IMMEDIATE,
            );
            let infinite_materials = context.player.has_infinite_materials();
            context
                .inv
                .with_item(|item| item.hurt_and_break(1, infinite_materials));
            context.world.game_event(
                &vanilla_game_events::BLOCK_CHANGE,
                context.hit_result.block_pos,
                &GameEventContext::new(Some(context.player), Some(updated_state)),
            );
            return InteractionResult::Success;
        }

        InteractionResult::Pass
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::{
        blocks::{block_state_ext::BlockStateExt, properties::BlockStateProperties},
        init_vanilla_registry,
        item_stack::ItemStack,
        vanilla_blocks, vanilla_items,
    };
    use steel_utils::{
        BlockPos, ChunkPos, Direction,
        types::{InteractionHand, UpdateFlags},
    };

    use super::ShovelItem;
    use crate::{
        behavior::{BlockHitResult, InteractionResult, ItemBehavior, UseOnContext, init_behaviors},
        block_entity::init_block_entities,
        inventory::container::Container as _,
        player::Player,
        test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk},
        world::World,
    };

    struct ShovelFixture {
        world: Arc<World>,
        player: Arc<Player>,
        pos: BlockPos,
    }

    fn hit_result(pos: BlockPos) -> BlockHitResult {
        BlockHitResult {
            location: DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()) + 0.5,
                f64::from(pos.z()) + 0.5,
            ),
            direction: Direction::Up,
            block_pos: pos,
            miss: false,
            inside: false,
            world_border_hit: false,
        }
    }

    fn create_fixture(name: &'static str) -> ShovelFixture {
        init_vanilla_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world(name);
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let player = TestPlayerBuilder::new(Arc::clone(&world), "ShovelTester", 1).build();
        player
            .inventory
            .lock()
            .set_item(0, ItemStack::new(&vanilla_items::DIAMOND_SHOVEL));
        ShovelFixture {
            world,
            player,
            pos: BlockPos::new(0, 64, 0),
        }
    }

    fn use_shovel(fixture: &ShovelFixture) -> InteractionResult {
        let mut context = UseOnContext::new(
            &fixture.player,
            InteractionHand::MainHand,
            hit_result(fixture.pos),
            &fixture.world,
            Arc::clone(&fixture.player.inventory),
        );
        ShovelItem.use_on(&mut context)
    }

    #[test]
    fn extinguishing_campfire_damages_shovel_and_sets_unlit() {
        let fixture = create_fixture("shovel_campfire");
        let campfire = vanilla_blocks::CAMPFIRE
            .default_state()
            .set_value(&BlockStateProperties::LIT, true);
        fixture
            .world
            .set_block(fixture.pos, campfire, UpdateFlags::empty());

        assert_eq!(use_shovel(&fixture), InteractionResult::Success);

        let state = fixture.world.get_block_state(fixture.pos);
        assert!(!state.get_value(&BlockStateProperties::LIT));
        assert_eq!(
            fixture
                .player
                .inventory
                .lock()
                .get_item(0)
                .get_damage_value(),
            1
        );
    }

    #[test]
    fn flattening_grass_damages_shovel() {
        let fixture = create_fixture("shovel_flatten");
        fixture.world.set_block(
            fixture.pos,
            vanilla_blocks::GRASS_BLOCK.default_state(),
            UpdateFlags::empty(),
        );
        fixture.world.set_block(
            fixture.pos.above(),
            vanilla_blocks::AIR.default_state(),
            UpdateFlags::empty(),
        );

        assert_eq!(use_shovel(&fixture), InteractionResult::Success);

        let state = fixture.world.get_block_state(fixture.pos);
        assert_eq!(state.get_block(), &vanilla_blocks::DIRT_PATH);
        assert_eq!(
            fixture
                .player
                .inventory
                .lock()
                .get_item(0)
                .get_damage_value(),
            1
        );
    }
}
