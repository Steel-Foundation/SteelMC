use super::ContainerOpenersCounter;
use crate::{
    behavior::{BLOCK_BEHAVIORS, BlockHitResult, InventoryAccess, init_behaviors},
    block_entity::init_block_entities,
    entity::{Entity as _, LivingEntity as _},
    inventory::lock::ContainerRef,
    player::ResetReason,
    test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk},
};
use glam::DVec3;
use std::sync::Arc;
use steel_registry::{init_vanilla_registry, vanilla_attributes, vanilla_blocks};
use steel_utils::types::{GameType, InteractionHand};
use steel_utils::{BlockPos, ChunkPos, types::UpdateFlags};

#[test]
fn recheck_tracks_live_menus_ranges_and_spectators() {
    init_vanilla_registry();
    init_behaviors();
    init_block_entities();
    let fixture = fresh_test_world("chest_openers_recheck");
    let world = &fixture.world;
    let pos = BlockPos::new(5, 64, 5);
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    assert!(world.set_block(
        pos,
        vanilla_blocks::CHEST.default_state(),
        UpdateFlags::UPDATE_NONE
    ));
    let player = TestPlayerBuilder::new(Arc::clone(world), "Viewer", 42).build();
    player.set_client_loaded(true);
    player.base().set_position_local(DVec3::new(5.5, 64.0, 5.5));
    assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
    let container = ContainerRef::from_block_entity(world.get_block_entity(pos).expect("chest"))
        .expect("inventory");
    let hit = BlockHitResult {
        location: DVec3::new(5.5, 64.5, 5.5),
        direction: steel_utils::Direction::Up,
        block_pos: pos,
        miss: false,
        inside: false,
        world_border_hit: false,
    };
    let mut inv = InventoryAccess::new(Arc::clone(&player.inventory), InteractionHand::MainHand);
    let behavior = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::CHEST);
    behavior.use_without_item(
        world.get_block_state(pos),
        world,
        pos,
        &player,
        &hit,
        &mut inv,
    );
    assert!(player.has_container_open());
    let counter = ContainerOpenersCounter::default();
    assert_eq!(counter.increment(player.as_ref()).current, 1);
    assert_eq!(counter.increment(player.as_ref()).current, 2);
    assert_eq!(
        counter
            .recheck(world, pos, container.container_id())
            .current,
        1,
        "one live menu corrects stale increments"
    );
    player
        .attributes()
        .lock()
        .set_base_value(vanilla_attributes::BLOCK_INTERACTION_RANGE, 2.0);
    counter.recheck(world, pos, container.container_id());
    assert_eq!(counter.state.lock().max_interaction_range, 2.0);
    player.restore_game_modes(GameType::Spectator, None);
    assert_eq!(
        counter
            .recheck(world, pos, container.container_id())
            .current,
        0
    );
    assert_eq!(counter.state.lock().max_interaction_range, 0.0);
    player.restore_game_modes(GameType::Survival, None);
    assert_eq!(
        counter
            .recheck(world, pos, container.container_id())
            .current,
        1
    );
    player.do_close_container();
    assert_eq!(
        counter
            .recheck(world, pos, container.container_id())
            .current,
        0
    );
    world.remove_player_for_world_change(&player);
}
