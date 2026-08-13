use std::sync::Arc;

use serde_json::json;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::fluid::FluidStateExt;
use steel_registry::vanilla_game_rules::ADVANCE_TIME;
use steel_registry::{REGISTRY, vanilla_blocks, vanilla_fluids};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, ChunkPos, Identifier};
use uuid::Uuid;

use super::{InMemoryWorld, TestHarnessError};
use crate::chunk::chunk_ticket_manager::ChunkTicketLevel;
use crate::chunk::status::ChunkStatus;
use crate::world::ScheduledTickAccess;

#[test]
fn worlds_keep_block_state_isolated() -> Result<(), TestHarnessError> {
    let first = InMemoryWorld::new()?;
    let second = InMemoryWorld::new()?;
    let pos = BlockPos::new(3, 64, 5);
    let chunk = ChunkPos::from_block_pos(pos);
    first.ensure_chunk(chunk)?;
    second.ensure_chunk(chunk)?;

    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
    assert!(first.world().set_block(pos, stone, UpdateFlags::UPDATE_ALL));
    assert_eq!(first.world().get_block_state(pos), stone);
    assert_eq!(second.world().get_block_state(pos), air);
    Ok(())
}

#[test]
fn ensured_chunk_is_full_and_block_ticking() -> Result<(), TestHarnessError> {
    let mut harness = InMemoryWorld::new()?;
    let pos = ChunkPos::new(4, -2);
    harness.ensure_chunk(pos)?;
    let holder = harness
        .world()
        .chunk_map
        .chunks
        .read_sync(&pos, |_, holder| Arc::clone(holder))
        .ok_or(TestHarnessError::ChunkNotFull { pos, status: None })?;

    assert_eq!(holder.published_status(), Some(ChunkStatus::Full));
    assert!(holder.ticking_readiness_snapshot().is_block_ticking());
    assert!(
        holder
            .simulation_level()
            .is_some_and(ChunkTicketLevel::is_block_ticking)
    );
    assert_full_halo(&harness, pos, 1, ChunkTicketLevel::BLOCK_TICKING_CHUNK);
    let timings = harness.tick_once()?;
    assert_eq!(timings.chunk_map.tickable_count, 1);
    Ok(())
}

#[test]
fn scheduled_water_crosses_a_chunk_edge_through_the_full_halo() -> Result<(), TestHarnessError> {
    let mut harness = InMemoryWorld::new()?;
    let center = ChunkPos::new(0, 0);
    harness.ensure_chunk(center)?;

    let source = BlockPos::new(15, 64, 0);
    let across_edge = BlockPos::new(16, 64, 0);
    let stone = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::STONE);
    let water = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::WATER);
    let fixture_flags = UpdateFlags::UPDATE_NONE | UpdateFlags::UPDATE_SKIP_ON_PLACE;

    for pos in [
        source.below(),
        across_edge.below(),
        BlockPos::new(14, 64, 0),
        BlockPos::new(15, 64, -1),
        BlockPos::new(15, 64, 1),
    ] {
        assert!(harness.world().set_block(pos, stone, fixture_flags));
    }
    assert!(harness.world().set_block(source, water, fixture_flags));
    assert!(harness.world().get_block_state(across_edge).is_air());
    assert!(
        harness
            .world()
            .schedule_fluid_tick_default(source, &vanilla_fluids::WATER, 1,)
    );

    let _ = harness.tick_once()?;
    let resulting_fluid = harness
        .world()
        .get_block_state(across_edge)
        .get_fluid_state();
    assert!(resulting_fluid.is_water());
    assert!(!resulting_fluid.is_empty());
    Ok(())
}

#[test]
fn ticks_and_daytime_are_exact() -> Result<(), TestHarnessError> {
    let mut harness = InMemoryWorld::new()?;
    assert_eq!(harness.current_tick(), 0);
    assert_eq!(harness.daytime()?, 0);

    harness.set_daytime(23_999)?;
    assert_eq!(harness.daytime()?, 23_999);
    let _ = harness.tick_once()?;
    assert_eq!(harness.current_tick(), 1);
    assert_eq!(harness.daytime()?, 0);

    harness.set_daytime(123)?;
    harness.set_game_rule(&Identifier::vanilla_static("advance_time"), &json!(false))?;
    let _ = harness.tick_once()?;
    assert_eq!(harness.current_tick(), 2);
    assert_eq!(harness.daytime()?, 123);
    assert!(matches!(
        harness.set_daytime(24_000),
        Err(TestHarnessError::InvalidDaytime { .. })
    ));
    Ok(())
}

#[test]
fn player_is_attached_through_world_lifecycle() -> Result<(), TestHarnessError> {
    let harness = InMemoryWorld::new()?;
    let uuid = Uuid::from_u128(1);
    let test_player = harness.create_player(uuid, "FlintPlayer", 7)?;
    assert_full_halo(
        &harness,
        ChunkPos::new(0, 0),
        2,
        ChunkTicketLevel::ENTITY_TICKING_CHUNK,
    );
    assert!(
        harness
            .world()
            .chunk_map
            .tickable_full_chunk_positions()
            .contains(&ChunkPos::new(0, 0))
    );
    let registered = harness
        .world()
        .players
        .get_by_uuid(&uuid)
        .ok_or(TestHarnessError::DuplicatePlayerUuid { uuid })?;

    assert!(Arc::ptr_eq(&registered, test_player.player()));
    assert!(harness.world().get_entity_by_id(7).is_some());
    assert!(!test_player.connection().events().is_empty());
    drop(test_player);
    assert!(harness.world().players.is_empty());
    assert!(harness.world().get_entity_by_id(7).is_none());
    Ok(())
}

#[test]
fn game_rule_values_are_registry_validated() -> Result<(), TestHarnessError> {
    let harness = InMemoryWorld::new()?;
    let advance_time = Identifier::vanilla_static("advance_time");
    harness.set_game_rule(&advance_time, &json!(false))?;
    assert!(!harness.world().get_game_rule(&ADVANCE_TIME));
    assert!(matches!(
        harness.set_game_rule(&advance_time, &json!("false")),
        Err(TestHarnessError::InvalidGameRuleValue { .. })
    ));
    assert!(matches!(
        harness.set_game_rule(&Identifier::vanilla_static("missing"), &json!(true)),
        Err(TestHarnessError::UnknownGameRule { .. })
    ));
    Ok(())
}

#[test]
fn repeated_world_shutdown_has_no_generation_work() -> Result<(), TestHarnessError> {
    for index in 0..8 {
        let mut harness = InMemoryWorld::new()?;
        harness.ensure_chunk(ChunkPos::new(index, -index))?;
        let _ = harness.tick_once()?;
        assert_eq!(harness.world().chunk_map.task_tracker.len(), 1);
        drop(harness);
    }
    Ok(())
}

fn assert_full_halo(
    harness: &InMemoryWorld,
    center: ChunkPos,
    radius: u8,
    center_level: ChunkTicketLevel,
) {
    let side = usize::from(radius) * 2 + 1;
    let radius = i32::from(radius);
    assert_eq!(harness.world().chunk_map.chunks.len(), side * side);
    for z in center.0.y - radius..=center.0.y + radius {
        for x in center.0.x - radius..=center.0.x + radius {
            let pos = ChunkPos::new(x, z);
            let holder = harness
                .world()
                .chunk_map
                .chunks
                .read_sync(&pos, |_, holder| Arc::clone(holder));
            let Some(holder) = holder else {
                panic!("Full halo is missing {pos:?}");
            };
            assert_eq!(holder.published_status(), Some(ChunkStatus::Full));
            if pos == center {
                assert_eq!(holder.load_level(), Some(center_level));
                assert_eq!(holder.simulation_level(), Some(center_level));
            } else {
                assert_eq!(holder.load_level(), Some(ChunkTicketLevel::FULL_CHUNK));
                assert_eq!(holder.simulation_level(), None);
                assert!(!holder.ticking_readiness_snapshot().is_block_ticking());
            }
        }
    }
}
