// Moved from player/tests.rs with the entities it depends on.

#[test]
fn monster_prevents_rest_blocks_sleeping_near_a_bed() {
    init_vanilla_registry();
    init_behaviors();
    let world_fixture = fresh_test_world("monster_prevents_rest");
    let world = &world_fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    world.set_clock_total_ticks(&vanilla_world_clocks::OVERWORLD, NIGHT_TIME);
    let bed_pos = BlockPos::new(8, 65, 8);
    assert!(world.set_block(
        bed_pos,
        vanilla_blocks::WHITE_BED.default_state(),
        UpdateFlags::UPDATE_ALL,
    ));
    let player = test_player(Arc::clone(world));
    player.base.set_position_local(DVec3::new(8.5, 65.0, 8.5));

    assert!(
        player
            .start_sleep_in_bed(
                bed_pos,
                &world.dimension_type.bed_rule,
                &vanilla_custom_stats::SLEEP_IN_BED
            )
            .is_ok()
    );
    assert!(player.is_sleeping());
    player.stop_sleep_in_bed(false, false);

    let endermite: SharedEntity = Arc::new(EndermiteEntity::new(
        &vanilla_entities::ENDERMITE,
        next_entity_id(),
        DVec3::new(11.0, 65.0, 8.0),
        Arc::downgrade(world),
    ));
    world
        .try_add_entity(Arc::clone(&endermite))
        .expect("test endermite should attach to the loaded chunk");

    let result = player.start_sleep_in_bed(
        bed_pos,
        &world.dimension_type.bed_rule,
        &vanilla_custom_stats::SLEEP_IN_BED,
    );
    assert!(
        matches!(result, Err(BedSleepingProblem::NotSafe(_))),
        "a monster nearby must yield the not-safe problem, got {result:?}"
    );
    assert!(!player.is_sleeping());
}
