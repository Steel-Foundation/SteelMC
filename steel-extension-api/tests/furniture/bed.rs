use super::{fixture::Fixture, helpers};
use glam::DVec3;
use rustc_hash::FxHashMap;
use std::error::Error;
use steel_core::{
    entity::{Entity, LivingEntity, entities::objects::items::ItemEntity},
    player::chunk_sender::ChunkSender,
    world::test_support::move_to_time_marker,
};
use steel_registry::{
    REGISTRY,
    blocks::{block_state_ext::BlockStateExt, properties::BlockStateProperties},
    client_content::client_block_state,
    items::ItemRef,
    vanilla_blocks, vanilla_dimension_types,
};
use steel_utils::{
    BlockPos, ChunkPos, Downcast as _, Identifier, WorldAabb, locks::SyncMutex, types::GameType,
};

pub(super) fn check(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    fixture
        .player
        .try_set_position(DVec3::new(6.5, 65.0, 8.5))?;
    let foot = BlockPos::new(8, 65, 8);
    let bed = helpers::place(fixture, "custom_bed", foot)?;
    let foot_state = fixture.world.get_block_state(foot);
    let head = foot_state
        .get_value(&BlockStateProperties::HORIZONTAL_FACING)
        .relative(foot);
    assert_eq!(fixture.world.get_block_state(head).get_block(), bed);
    assert_eq!(foot_state.get_property_str("part"), Some("foot"));
    assert_eq!(
        fixture.world.get_block_state(head).get_property_str("part"),
        Some("head")
    );
    let base = REGISTRY.blocks.get_base_state_id(bed);
    let model = REGISTRY
        .client_content
        .block(bed)
        .ok_or("missing bed representation")?;
    for offset in 0..bed.state_count() {
        let state = steel_utils::BlockStateId(base.0 + offset);
        let carrier = client_block_state(state);
        assert_eq!(carrier.get_block(), &vanilla_blocks::BLUE_BED);
        for property in ["facing", "occupied", "part"] {
            assert_eq!(
                state.get_property_str(property),
                carrier.get_property_str(property)
            );
        }
        assert_eq!(model.displays_for(state).len(), 5);
        assert_eq!(
            state.get_static_collision_shape(),
            carrier.get_static_collision_shape()
        );
    }
    let loaded = helpers::saved(fixture)?;
    assert_eq!(loaded.chunk.get_block_state(foot), foot_state);
    assert_eq!(
        loaded.chunk.get_block_state(head),
        fixture.world.get_block_state(head)
    );
    check_sleep_and_models(fixture, foot, head)?;
    let item = REGISTRY.items.by_block(bed);
    let before = drops_of(fixture, item);
    helpers::mine(fixture, foot);
    assert!(fixture.world.get_block_state(foot).is_air());
    assert!(fixture.world.get_block_state(head).is_air());
    assert_eq!(
        drops_of(fixture, item),
        before + 1,
        "breaking the foot must drop exactly one bed"
    );
    helpers::place(fixture, "custom_bed", foot)?;
    let second_head = fixture
        .world
        .get_block_state(foot)
        .get_value(&BlockStateProperties::HORIZONTAL_FACING)
        .relative(foot);
    let before = drops_of(fixture, item);
    helpers::mine(fixture, second_head);
    assert!(fixture.world.get_block_state(foot).is_air());
    assert!(fixture.world.get_block_state(second_head).is_air());
    assert_eq!(
        drops_of(fixture, item),
        before + 1,
        "breaking the head must drop exactly one bed"
    );
    Ok(())
}

fn check_sleep_and_models(
    fixture: &Fixture,
    foot: BlockPos,
    head: BlockPos,
) -> Result<(), Box<dyn Error>> {
    let _runtime = fixture.runtime.enter();
    move_to_time_marker(&fixture.world, &Identifier::vanilla_static("night"))?;
    assert!(fixture.world.is_dark_outside());
    let epoch = SyncMutex::new(0);
    let mut sender = ChunkSender::default();
    let chunk = ChunkPos::from_block_pos(foot);
    sender.mark_chunk_pending_to_send(chunk);
    let batch = sender
        .prepare_batch(&fixture.world, chunk, &epoch)
        .ok_or("chunk is not ready")?;
    let encoded = ChunkSender::encode_batch(&batch, &mut FxHashMap::default(), None, &fixture.pool);
    fixture.packets.lock().clear();
    sender.commit_batch(&batch, encoded, &fixture.player.connection, &epoch);
    assert_eq!(fixture.display_ids()?.len(), 10);
    fixture.packets.lock().clear();
    helpers::interact(fixture, foot);
    assert!(
        fixture.player.is_sleeping(),
        "custom bed must enter the actual player sleep state"
    );
    assert_eq!(fixture.player.sleeping_pos(), Some(head));
    assert!(
        fixture.player.respawn_config().is_some(),
        "custom bed must set the normal respawn target"
    );
    assert_eq!(
        fixture
            .player
            .respawn_config()
            .ok_or("missing respawn point")?
            .respawn_data
            .global_pos
            .pos,
        head
    );
    assert_eq!(
        fixture
            .world
            .get_block_state(head)
            .get_property_str("occupied"),
        Some("true")
    );
    assert_eq!(
        client_block_state(fixture.world.get_block_state(head)).get_property_str("occupied"),
        Some("true")
    );
    sender.update_client_block(
        &fixture.player.connection,
        head,
        fixture.world.get_block_state(head),
    );
    assert_eq!(fixture.removed_ids()?.len(), 5);
    assert_eq!(fixture.display_ids()?.len(), 5);
    fixture.player.stop_sleeping();
    assert!(!fixture.player.is_sleeping());
    assert_eq!(
        fixture
            .world
            .get_block_state(head)
            .get_property_str("occupied"),
        Some("false")
    );
    fixture
        .player
        .try_set_position(DVec3::new(6.5, 65.0, 8.5))?;
    helpers::interact(fixture, foot);
    assert!(fixture.player.is_sleeping());
    for tick in 3..=122 {
        fixture.world.tick_game(tick, true);
    }
    assert!(
        !fixture.player.is_sleeping(),
        "sleeping through the night must wake the player"
    );
    assert!(
        fixture.world.is_bright_outside(),
        "the real world clock must advance to morning"
    );
    fixture.packets.lock().clear();
    sender.drop_chunk(&fixture.player.connection, chunk);
    assert_eq!(fixture.removed_ids()?.len(), 10);
    Ok(())
}

pub(super) fn check_explosion() -> Result<(), Box<dyn Error>> {
    let mut fixture = Fixture::with_dimension(&vanilla_dimension_types::THE_NETHER, true)?;
    helpers::floor(&fixture);
    fixture.join_player()?;
    fixture.player.set_game_mode(GameType::Creative);
    let foot = BlockPos::new(8, 65, 8);
    let bed = helpers::place(&fixture, "custom_bed", foot)?;
    let head = fixture
        .world
        .get_block_state(foot)
        .get_value(&BlockStateProperties::HORIZONTAL_FACING)
        .relative(foot);
    assert!(fixture.world.dimension_type.bed_rule.destroy_on_use);
    helpers::interact(&fixture, foot);
    assert_ne!(fixture.world.get_block_state(foot).get_block(), bed);
    assert_ne!(fixture.world.get_block_state(head).get_block(), bed);
    assert!(!fixture.player.is_sleeping());
    Ok(())
}

fn drops_of(fixture: &Fixture, item: ItemRef) -> i32 {
    fixture
        .world
        .get_entities_in_aabb(&WorldAabb::of_size(
            DVec3::new(8.5, 65.5, 8.5),
            8.0,
            8.0,
            8.0,
        ))
        .iter()
        .filter_map(|entity| entity.downcast_ref::<ItemEntity>())
        .map(ItemEntity::get_item)
        .filter(|stack| stack.is(item))
        .map(|stack| stack.count())
        .sum()
}
