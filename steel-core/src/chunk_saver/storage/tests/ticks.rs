use super::*;

#[test]
fn persisted_proto_ticks_deduplicate_while_full_ticks_retain_saved_entries() {
    init_test_core();
    let pos = ChunkPos::new(0, 0);
    let duplicate_ticks = vec![
        PersistentTick {
            x: 1,
            y: 2,
            z: 3,
            delay: 7,
            priority: TickPriority::High as i8,
            tick_type: vanilla_blocks::DIRT.key.clone(),
        },
        PersistentTick {
            x: 1,
            y: 2,
            z: 3,
            delay: 2,
            priority: TickPriority::Low as i8,
            tick_type: vanilla_blocks::DIRT.key.clone(),
        },
    ];
    let persistent = ChunkStorage::to_persistent(
        &single_empty_section(),
        &[],
        &[],
        &[],
        duplicate_ticks,
        Vec::new(),
        Vec::new(),
        ChunkStorage::light_to_persistent(&ChunkLightData::for_valid_world_height(0, 16)),
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        pos,
    );

    let proto_loaded = ChunkStorage::persistent_to_chunk(
        &persistent,
        pos,
        ChunkStatus::Carvers,
        0,
        16,
        Weak::new(),
    );
    let proto = proto_loaded.chunk;
    let Some(proto_ticks) = proto.scheduled_ticks.pending_block_snapshot() else {
        panic!("loaded proto ticks should remain pending");
    };
    assert_eq!(proto_ticks.len(), 1);
    assert_eq!(proto_ticks[0].delay, 7);
    assert_eq!(proto_ticks[0].priority, TickPriority::High);

    let full_loaded =
        ChunkStorage::persistent_to_chunk(&persistent, pos, ChunkStatus::Full, 0, 16, Weak::new());
    let full = FullChunkRef::from_full_context(&full_loaded.chunk);
    assert_eq!(full.scheduled_tick_snapshot().block.len(), 2);
}

#[test]
fn persisted_tick_priorities_clamp_to_vanilla_extremes() {
    init_vanilla_registry();
    let chunk_pos = ChunkPos::new(0, 0);
    let block_ticks = ChunkStorage::persistent_to_block_saved_ticks(
        &[
            PersistentTick {
                x: 1,
                y: 64,
                z: 1,
                delay: 0,
                priority: -4,
                tick_type: vanilla_blocks::STONE.key.clone(),
            },
            PersistentTick {
                x: 2,
                y: 64,
                z: 2,
                delay: 0,
                priority: 4,
                tick_type: vanilla_blocks::STONE.key.clone(),
            },
        ],
        chunk_pos,
    );
    assert_eq!(block_ticks[0].priority, TickPriority::ExtremelyHigh);
    assert_eq!(block_ticks[1].priority, TickPriority::ExtremelyLow);

    let fluid_ticks = ChunkStorage::persistent_to_fluid_saved_ticks(
        &[
            PersistentTick {
                x: 3,
                y: 64,
                z: 3,
                delay: 0,
                priority: i8::MIN,
                tick_type: vanilla_fluids::WATER.key.clone(),
            },
            PersistentTick {
                x: 4,
                y: 64,
                z: 4,
                delay: 0,
                priority: i8::MAX,
                tick_type: vanilla_fluids::WATER.key.clone(),
            },
        ],
        chunk_pos,
    );
    assert_eq!(fluid_ticks[0].priority, TickPriority::ExtremelyHigh);
    assert_eq!(fluid_ticks[1].priority, TickPriority::ExtremelyLow);
}

#[test]
fn forced_prepare_preserves_dirty_set_after_save_decision() {
    init_vanilla_registry();
    let chunk = Chunk::new(
        single_empty_section(),
        ChunkPos::new(0, 0),
        0,
        16,
        Weak::new(),
    );

    assert!(chunk.take_dirty());
    chunk.mark_dirty();

    let Some(_prepared) = ChunkStorage::prepare_chunk_save(&chunk, ChunkStatus::Empty, &[], true)
    else {
        panic!("forced save prep should serialize the chunk");
    };
    assert!(chunk.is_dirty());
}

#[test]
fn full_chunk_save_snapshots_chunk_owned_scheduled_ticks() {
    init_test_core();
    let world = fresh_test_world("chunk_owned_tick_save");
    let chunk_pos = ChunkPos::new(0, 0);
    let holder = insert_ready_full_chunk(&world, chunk_pos);
    let block_pos = BlockPos::new(1, 64, 2);
    let fluid_pos = BlockPos::new(3, 64, 4);
    world.schedule_block_tick(block_pos, &vanilla_blocks::STONE, 7, TickPriority::High);
    world.schedule_fluid_tick(fluid_pos, &vanilla_fluids::WATER, 11, TickPriority::Low);

    let Some(chunk) = holder.try_chunk(ChunkStatus::Full) else {
        panic!("inserted test chunk must remain Full");
    };
    let Some(prepared) = ChunkStorage::prepare_chunk_save(chunk, ChunkStatus::Full, &[], true)
    else {
        panic!("forced Full-chunk save must produce a snapshot");
    };

    assert_eq!(prepared.persistent.block_ticks.len(), 1);
    assert_eq!(prepared.persistent.block_ticks[0].delay, 7);
    assert_eq!(
        prepared.persistent.block_ticks[0].priority,
        TickPriority::High as i8
    );
    assert_eq!(prepared.persistent.fluid_ticks.len(), 1);
    assert_eq!(prepared.persistent.fluid_ticks[0].delay, 11);
    assert_eq!(
        prepared.persistent.fluid_ticks[0].priority,
        TickPriority::Low as i8
    );
}
