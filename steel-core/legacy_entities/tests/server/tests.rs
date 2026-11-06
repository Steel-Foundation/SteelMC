// Moved from server/tests.rs with the entities it depends on.

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "keeps session pacing and pearl ownership assertions in the same End respawn lifecycle"
)]
fn end_credits_respawn_replaces_the_detached_player_incarnation() {
    let target_world_fixture = fresh_test_world_in_domain("survival", "overworld");
    let target_world = &target_world_fixture.world;
    let source_world_fixture = fresh_test_derived_world(target_world, "the_end");
    let source_world = &source_world_fixture.world;
    prepare_respawn_test_world(source_world);
    prepare_respawn_test_world(target_world);
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };

    runtime.block_on(async {
        let domain = ResolvedDomainConfig {
            name: "survival".to_owned(),
            default_world: target_world.key.clone(),
            worlds: vec![source_world.key.clone(), target_world.key.clone()],
        };
        let worlds = [Arc::clone(source_world), Arc::clone(target_world)];
        let server = test_server_with_worlds(
            domain.name.clone(),
            slice::from_ref(&domain),
            &worlds,
            PermissionSubjectIndex::new(),
        )
        .await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let old_player = test_player(&server, Arc::clone(source_world));

        assert!(server.online_players.insert(Arc::clone(&old_player)));
        assert!(source_world.add_player(Arc::clone(&old_player), ResetReason::InitialJoin));
        let _ = old_player.mark_joined_world();

        let pearl = Arc::new(EnderPearlEntity::new(
            &vanilla_entities::ENDER_PEARL,
            2,
            DVec3::new(8.5, 100.0, 8.5),
            Arc::downgrade(source_world),
        ));
        let old_owner: SharedEntity = Arc::<Player>::clone(&old_player);
        pearl.set_owner_entity(Some(&old_owner));
        let source = DamageSource::environment(&vanilla_damage_types::THROWN)
            .with_causing_entity(Arc::clone(&old_owner))
            .with_direct_entity(Arc::<EnderPearlEntity>::clone(&pearl));
        old_player.record_last_damage_source(&source);
        let shared_pearl: SharedEntity = Arc::<EnderPearlEntity>::clone(&pearl);
        if let Err(error) = source_world.try_add_entity(Arc::clone(&shared_pearl)) {
            panic!("test pearl should be added: {error}");
        }
        old_player.register_ender_pearl(&shared_pearl);
        let leave_game = vanilla_stat_types::CUSTOM.get(&vanilla_custom_stats::LEAVE_GAME);
        let leave_game_count = |player: &Player| {
            player
                .stats()
                .iter()
                .find_map(|(stat, count)| (*stat == leave_game).then_some(*count))
                .unwrap_or(0)
        };
        let leave_game_before_credits = leave_game_count(&old_player);

        let requested_chunks_per_tick = 2;
        assert_ne!(
            send_test_chunk_batch(&server, &old_player, source_world),
            Vec::<ChunkPos>::new()
        );

        old_player.show_end_credits();
        assert!(!source_world.contains_player(&old_player));
        assert!(server.owns_online_player(&old_player));
        old_player.handle_client_command(ClientCommandAction::PerformRespawn);
        assert_eq!(server.jobs.len(), 1);
        finish_test_respawn_job(&server, target_world).await;

        let replacement = current_respawn_replacement(&server, target_world, &old_player);
        assert_eq!(
            replacement
                .chunk_sender()
                .lock()
                .unacknowledged_batch_count_for_test(),
            1
        );
        assert!(
            replacement
                .chunk_sender()
                .lock()
                .on_chunk_batch_received_by_client(requested_chunks_per_tick as f32)
        );
        // Respawn queues the target player's tickets after the job's scheduling pass.
        target_world.chunk_map.advance_scheduling();
        assert_eq!(
            send_test_chunk_batch(&server, &replacement, target_world).len(),
            requested_chunks_per_tick
        );
        assert_eq!(
            replacement
                .chunk_sender()
                .lock()
                .unacknowledged_batch_count_for_test(),
            1
        );
        assert!(replacement.has_seen_credits());
        assert!(replacement.last_damage_source().is_none());
        assert!(old_player.last_damage_source().is_some());
        assert!(Arc::ptr_eq(
            source.causing_entity().expect("original owner"),
            &old_owner
        ));
        assert!(!replacement.has_won_game());
        assert!(!replacement.experience.lock().dirty);
        assert_eq!(
            leave_game_count(&replacement),
            leave_game_before_credits,
            "End credits are not a disconnect"
        );
        assert!(old_player.is_removed());
        assert_eq!(
            old_player.removal_reason(),
            Some(RemovalReason::ChangedWorld)
        );

        assert!(replacement.ender_pearls().is_empty());
        Arc::clone(&pearl).tick();
        assert!(
            pearl
                .projectile_owner()
                .is_some_and(|owner| owner.generation() == replacement.generation())
        );
        let registered_pearls = replacement.ender_pearls();
        assert_eq!(registered_pearls.len(), 1);
        assert_eq!(registered_pearls[0].generation(), pearl.generation());

        target_world.remove_player_for_world_change(&replacement);
        assert!(server.remove_online_player_sync(&replacement).is_some());
        pearl.set_removed(RemovalReason::Discarded);
        source_world.chunk_map.stop_generation_refill_loop();
        target_world.chunk_map.stop_generation_refill_loop();
        source_world.chunk_map.task_tracker.close();
        target_world.chunk_map.task_tracker.close();
        tokio::join!(
            source_world.chunk_map.task_tracker.wait(),
            target_world.chunk_map.task_tracker.wait(),
        );
        drop((old_player, replacement, shared_pearl, pearl, server));
    });
}
// Moved from server/tests.rs with the entities it depends on.

#[test]
fn domain_restore_jobs_follow_same_session_player_replacement() {
    init_vanilla_registry();
    init_entities();
    init_behaviors();
    let world_fixture = fresh_test_world_in_domain("survival", "respawn_restore");
    let world = &world_fixture.world;
    insert_ready_full_chunk(world, ChunkPos::new(0, 0));
    let runtime = Builder::new_current_thread().enable_all().build();
    let Ok(runtime) = runtime else {
        panic!("test runtime should initialize");
    };
    runtime.block_on(async {
        let server = test_server(Arc::clone(world), PermissionSubjectIndex::new()).await;
        let Ok(server) = server else {
            panic!("test server should initialize");
        };
        let old_player =
            test_player_with_packets(&server, Arc::clone(world), "TestPlayer", next_entity_id()).0;
        assert!(server.online_players.insert(Arc::clone(&old_player)));
        assert!(world.add_player(Arc::clone(&old_player), ResetReason::InitialJoin));
        let _ = old_player.mark_joined_world();

        let residence_token = old_player.domain_residence_token();
        let root_uuid = [6; 16];
        let pearl_uuid = [7; 16];
        let root = PersistentRootVehicle {
            attach: root_uuid,
            entity: test_persistent_entity(&vanilla_entities::CHICKEN, root_uuid),
        };
        let mut pearl_entity = test_persistent_entity(&vanilla_entities::ENDER_PEARL, pearl_uuid);
        let mut pearl_nbt = NbtCompound::new();
        pearl_nbt.insert(
            "Owner",
            NbtTag::IntArray(old_player.gameprofile.id().to_int_array().to_vec()),
        );
        pearl_nbt.write(&mut pearl_entity.nbt_data);
        let pearl = PersistentEnderPearl {
            world: world.key.to_string(),
            entity: pearl_entity,
        };
        assert!(old_player.install_pending_domain_restores(
            residence_token,
            world,
            Some(root.clone()),
            vec![pearl.clone()],
        ));
        let root_job = RootVehicleRestoreJob::new(
            Arc::clone(&old_player),
            Arc::clone(world),
            &root,
            residence_token,
            true,
        );
        let Some(root_job) = root_job else {
            panic!("valid root vehicle data should create a restore job");
        };
        let pearl_job = EnderPearlRestoreJob::new(
            Arc::clone(&old_player),
            Arc::clone(world),
            pearl.entity,
            residence_token,
        );
        let Some(pearl_job) = pearl_job else {
            panic!("valid pearl data should create a restore job");
        };
        server.jobs.spawn(root_job);
        server.jobs.spawn(pearl_job);

        let replacement = old_player.new_respawn_replacement(Arc::clone(world), true, false, true);
        assert!(world.detach_player_for_respawn(&old_player, true));
        assert!(world.install_respawned_player(Arc::clone(&replacement), Some(&old_player),));
        assert!(server.replace_online_player(&old_player, Arc::clone(&replacement)));
        let _ = replacement.mark_joined_world();
        assert!(old_player.session.replace_player(&old_player, &replacement));

        finish_test_respawn_job(&server, world).await;

        assert!(replacement.is_domain_residence_current(residence_token));
        assert!(
            replacement
                .pending_root_vehicle_for_current_world()
                .is_none()
        );
        assert!(replacement.pending_ender_pearls().is_empty());
        assert!(old_player.vehicle().is_none());
        assert_eq!(
            replacement.vehicle().map(|vehicle| vehicle.uuid()),
            Some(Uuid::from_bytes(root_uuid))
        );
        let restored_pearls = replacement.ender_pearls();
        assert_eq!(restored_pearls.len(), 1);
        assert_eq!(restored_pearls[0].uuid(), Uuid::from_bytes(pearl_uuid));
        assert!(
            restored_pearls[0]
                .projectile_owner()
                .is_some_and(|owner| owner.generation() == replacement.generation())
        );

        world.remove_player_for_world_change(&replacement);
        assert!(server.remove_online_player_sync(&replacement).is_some());
        world.chunk_map.stop_generation_refill_loop();
        world.chunk_map.task_tracker.close();
        world.chunk_map.task_tracker.wait().await;
        drop((old_player, replacement, server));
    });
}
