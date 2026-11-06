// Moved from entity/ai/goal/eat_block_goal.rs with the entities it depends on.

    fn sheep_on_grass_world(name: &'static str) -> (TestWorld, SharedEntity) {
        use steel_registry::vanilla_blocks;
        use steel_registry::vanilla_entities;
        use steel_utils::ChunkPos;

        init_behaviors();
        let world_fixture = fresh_test_world(name);
        let world = &world_fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        let sheep = ChickenEntity::new(
            &vanilla_entities::CHICKEN,
            1,
            DVec3::new(8.0, 65.0, 8.0),
            Arc::downgrade(world),
        );
        sheep.set_sheared(true);
        let shared: SharedEntity = Arc::new(sheep);
        world
            .try_add_entity(Arc::clone(&shared))
            .expect("sheep should attach to the loaded test chunk");
        world.set_block_state(
            shared.block_position().below(),
            vanilla_blocks::GRASS_BLOCK.default_state(),
            UpdateFlags::UPDATE_CLIENTS,
        );
        (world_fixture, shared)
    }

    #[test]
    fn eat_block_goal_turns_grass_into_dirt_and_runs_mob_ate() {
        use steel_registry::vanilla_blocks;

        init_vanilla_registry();
        let (world_fixture, shared) = sheep_on_grass_world("eat_grass");
        let world = &world_fixture.world;
        let mob = shared
            .as_pathfinder_mob()
            .expect("sheep should be a pathfinder mob");

        let mut goal = EatBlockGoal::new();
        goal.start(mob);
        for _ in 0..18 {
            goal.tick(mob, &shared);
        }

        let sheep = shared
            .downcast_ref::<ChickenEntity>()
            .expect("shared entity should be a sheep");
        assert!(!sheep.is_sheared(), "eating grass should run Mob.ate");
        assert_eq!(
            world
                .get_block_state(shared.block_position().below())
                .get_block(),
            &vanilla_blocks::DIRT,
            "eating grass should turn it into dirt with mobGriefing enabled"
        );
    }

    #[test]
    fn eat_block_goal_does_not_grief_grass_with_mob_griefing_disabled() {
        use steel_registry::vanilla_blocks;

        init_vanilla_registry();
        let (world_fixture, shared) = sheep_on_grass_world("eat_grass_no_grief");
        let world = &world_fixture.world;
        world.set_game_rule(&MOB_GRIEFING, false);
        let mob = shared
            .as_pathfinder_mob()
            .expect("sheep should be a pathfinder mob");

        let mut goal = EatBlockGoal::new();
        goal.start(mob);
        for _ in 0..18 {
            goal.tick(mob, &shared);
        }

        let sheep = shared
            .downcast_ref::<ChickenEntity>()
            .expect("shared entity should be a sheep");
        assert!(
            !sheep.is_sheared(),
            "Mob.ate should still run without mobGriefing"
        );
        assert_eq!(
            world
                .get_block_state(shared.block_position().below())
                .get_block(),
            &vanilla_blocks::GRASS_BLOCK,
            "grass should survive without mobGriefing"
        );
    }
