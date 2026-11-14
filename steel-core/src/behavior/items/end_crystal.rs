//! End crystal item behavior.

use std::sync::Arc;

use glam::DVec3;
use steel_macros::item_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::item_stack::ItemStack;
use steel_registry::{vanilla_blocks, vanilla_entities, vanilla_game_events};
use steel_utils::WorldAabb;

use crate::behavior::{InteractionResult, ItemBehavior, UseOnContext};
use crate::entity::entities::EndCrystalEntity;
use crate::entity::next_entity_id;
use crate::world::game_event::GameEventContext;

/// Places an end crystal on top of obsidian or bedrock.
#[item_behavior(class = "EndCrystalItem")]
pub struct EndCrystalItem;

impl ItemBehavior for EndCrystalItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let world = context.world;
        let pos = context.hit_result.block_pos;
        let block = world.get_block_state(pos).get_block();
        if block != &vanilla_blocks::OBSIDIAN && block != &vanilla_blocks::BEDROCK {
            return InteractionResult::Fail;
        }

        let above = pos.above();
        if !world.get_block_state(above).is_air() {
            return InteractionResult::Fail;
        }

        let x = f64::from(above.x());
        let y = f64::from(above.y());
        let z = f64::from(above.z());
        // Vanilla's two-argument `getEntities` excludes spectators.
        let space = WorldAabb::new(x, y, z, x + 1.0, y + 2.0, z + 1.0);
        if world.has_entity_in_aabb_matching(&space, |entity| !entity.is_spectator()) {
            return InteractionResult::Fail;
        }

        let crystal = EndCrystalEntity::new(
            &vanilla_entities::END_CRYSTAL,
            next_entity_id(),
            DVec3::new(x + 0.5, y, z + 0.5),
            Arc::downgrade(world),
        );
        crystal.set_show_bottom(false);
        if let Err(error) = world.try_add_entity(Arc::new(crystal)) {
            log::warn!("Failed to add end crystal at {above:?}: {error}");
            return InteractionResult::Fail;
        }
        world.game_event(
            &vanilla_game_events::ENTITY_PLACE,
            above,
            &GameEventContext::new(Some(context.player.as_ref()), None),
        );
        // TODO: Call `EnderDragonFight.tryRespawn` once the dragon fight exists.

        context.inv.with_item(ItemStack::shrink_one);
        InteractionResult::Success
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::blocks::BlockRef;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{vanilla_blocks, vanilla_entities, vanilla_items};
    use steel_utils::Downcast as _;
    use steel_utils::types::{GameType, InteractionHand, UpdateFlags};
    use steel_utils::{BlockPos, ChunkPos, Direction, WorldAabb};

    use crate::behavior::{BlockHitResult, InteractionResult};
    use crate::bootstrap::init_globals;
    use crate::entity::entities::EndCrystalEntity;
    use crate::entity::{Entity as _, SharedEntity, next_entity_id};
    use crate::player::{Player, game_mode::use_item_on};
    use crate::test_support::{
        TestPlayerBuilder, TestWorld, fresh_test_world, insert_ready_full_chunk,
    };

    const POS: BlockPos = BlockPos::new(8, 64, 8);

    struct Fixture {
        world_fixture: TestWorld,
        player: Arc<Player>,
    }

    fn create_fixture(key: &'static str, base: BlockRef) -> Fixture {
        init_globals();
        let world_fixture = fresh_test_world(key);
        let world = &world_fixture.world;
        insert_ready_full_chunk(world, ChunkPos::from_block_pos(POS));
        let flags = UpdateFlags::UPDATE_NONE | UpdateFlags::UPDATE_SKIP_ON_PLACE;
        assert!(world.set_block(POS, base.default_state(), flags));

        let player =
            TestPlayerBuilder::new(Arc::clone(world), "CrystalTester", next_entity_id()).build();
        player
            .try_set_position(DVec3::new(8.5, 65.0, 11.5))
            .expect("test player should move into the loaded chunk");
        player.restore_game_modes(GameType::Survival, None);
        player
            .abilities
            .lock()
            .update_for_game_mode(GameType::Survival);
        player.inventory.lock().set_item_in_hand(
            InteractionHand::MainHand,
            ItemStack::new(&vanilla_items::END_CRYSTAL),
        );
        Fixture {
            world_fixture,
            player,
        }
    }

    fn use_crystal(fixture: &Fixture) -> InteractionResult {
        let hit = BlockHitResult {
            location: DVec3::new(8.5, 65.0, 8.5),
            direction: Direction::Up,
            block_pos: POS,
            miss: false,
            inside: false,
            world_border_hit: false,
        };
        use_item_on(
            &fixture.player,
            &fixture.world_fixture.world,
            InteractionHand::MainHand,
            &hit,
        )
    }

    fn crystals(fixture: &Fixture) -> Vec<SharedEntity> {
        let bounds = WorldAabb::new(6.0, 63.0, 6.0, 11.0, 68.0, 11.0);
        fixture
            .world_fixture
            .world
            .get_entities_in_aabb_matching(&bounds, |entity| {
                entity.entity_type() == &vanilla_entities::END_CRYSTAL
            })
    }

    fn held_count(fixture: &Fixture) -> i32 {
        fixture
            .player
            .inventory
            .lock()
            .get_item_in_hand(InteractionHand::MainHand)
            .count()
    }

    #[test]
    fn places_crystal_without_base_above_obsidian() {
        let fixture = create_fixture("end_crystal_item_place", &vanilla_blocks::OBSIDIAN);

        assert_eq!(use_crystal(&fixture), InteractionResult::Success);

        let placed = crystals(&fixture);
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].position(), DVec3::new(8.5, 65.0, 8.5));
        let crystal = placed[0]
            .as_ref()
            .downcast_ref::<EndCrystalEntity>()
            .expect("placed entity should be an end crystal");
        assert!(!crystal.shows_bottom());
        assert_eq!(held_count(&fixture), 0);
    }

    #[test]
    fn rejects_bases_other_than_obsidian_and_bedrock() {
        let fixture = create_fixture("end_crystal_item_stone", &vanilla_blocks::STONE);

        assert_eq!(use_crystal(&fixture), InteractionResult::Fail);
        assert!(crystals(&fixture).is_empty());
        assert_eq!(held_count(&fixture), 1);
    }

    #[test]
    fn rejects_when_an_entity_occupies_the_two_block_space() {
        let fixture = create_fixture("end_crystal_item_occupied", &vanilla_blocks::BEDROCK);
        let world = &fixture.world_fixture.world;
        // Overlaps only the upper block of the placement space.
        let blocker = EndCrystalEntity::new(
            &vanilla_entities::END_CRYSTAL,
            next_entity_id(),
            DVec3::new(8.5, 66.5, 8.5),
            Arc::downgrade(world),
        );
        world
            .try_add_entity(Arc::new(blocker))
            .expect("blocker should spawn");

        assert_eq!(use_crystal(&fixture), InteractionResult::Fail);
        assert_eq!(crystals(&fixture).len(), 1);
        assert_eq!(held_count(&fixture), 1);
    }
}
