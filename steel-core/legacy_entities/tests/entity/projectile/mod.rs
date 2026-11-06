// Moved from entity/projectile/mod.rs with the entities it depends on.

    #[test]
    fn may_break_requires_impact_projectile_tag() {
        init_vanilla_registry();

        let firework = FireworkRocketEntity::new(
            &vanilla_entities::FIREWORK_ROCKET,
            1,
            DVec3::ZERO,
            Weak::new(),
        );
        let ender_pearl = OwnerCollisionProjectile::new(2, DVec3::ZERO);

        assert!(firework.may_break(test_world()));
        assert!(!ender_pearl.may_break(test_world()));
    }

    #[test]
    fn move_vector_synthesizes_world_border_block_hit() {
        init_vanilla_registry();
        init_behaviors();

        let world = Arc::clone(world_border_projectile_test_world());
        let firework = FireworkRocketEntity::new(
            &vanilla_entities::FIREWORK_ROCKET,
            3,
            DVec3::new(4.5, 64.0, 0.0),
            Arc::downgrade(&world),
        );
        firework.set_velocity(DVec3::X);

        let Some(ProjectileHit::Block { location, hit }) = firework.get_hit_result_on_move_vector()
        else {
            panic!("firework should hit the world border");
        };

        assert!(hit.world_border_hit);
        assert!(!hit.inside);
        assert_eq!(hit.direction, Direction::East);
        assert_eq!(hit.block_pos, BlockPos::new(4, 64, 0));
        assert_eq!(location, hit.location);
        assert!(location.x < 5.0 && location.x > 4.999);
    }

    #[test]
    fn firework_deflects_without_exploding_on_deflecting_entity() {
        init_vanilla_registry();

        let world = Arc::clone(test_world());
        let owner = OwnerCollisionTestEntity::shared(6, DVec3::ZERO, true);
        let replacement: SharedEntity =
            TestPlayerBuilder::new(Arc::clone(&world), "Replacement", 7)
                .uuid(owner.uuid())
                .build();
        for cached in [false, true] {
            let firework = Arc::new(FireworkRocketEntity::new(
                &vanilla_entities::FIREWORK_ROCKET,
                4,
                DVec3::ZERO,
                Arc::downgrade(&world),
            ));
            if cached {
                firework.set_owner_entity(Some(&owner));
            } else {
                firework.set_owner_uuid(Some(owner.uuid()));
            }
            let reference = firework
                .projectile_base()
                .state
                .lock()
                .owner
                .clone()
                .expect("stored owner reference");
            firework.set_velocity(DVec3::X);
            let deflector = OwnerCollisionTestEntity::shared_with_type(
                5,
                DVec3::X,
                true,
                &vanilla_entities::BREEZE,
            );

            let deflection = Arc::clone(&firework).hit_target_or_deflect_self(
                &ProjectileHit::Entity(EntityHitResult {
                    entity: deflector,
                    location: DVec3::X,
                }),
            );

            assert_eq!(deflection, ProjectileDeflection::Reverse);
            assert_eq!(firework.velocity(), DVec3::new(-0.5, 0.0, 0.0));
            assert!(firework.needs_velocity_sync());
            assert!(!firework.is_removed());
            assert_eq!(firework.owner_uuid(), Some(owner.uuid()));
            if cached {
                assert!(Arc::ptr_eq(
                    &firework.get_owner().expect("cached owner"),
                    &owner
                ));
            } else {
                assert!(firework.get_owner().is_none());
            }
            reference.cache_entity(&replacement);
            assert!(Arc::ptr_eq(
                &firework.get_owner().expect("shared owner resolution"),
                &replacement
            ));
        }
    }

    #[test]
    fn base_block_hit_dispatches_vanilla_block_callbacks() {
        init_vanilla_registry();
        init_block_entities();
        init_behaviors();

        let world = Arc::clone(test_world());
        let chunk_map = Arc::clone(&world.chunk_map);
        let pos = BlockPos::new(1_136, 64, 1_136);
        let block_callback_test =
            chunk_map.with_full_chunks_in_radius(ChunkPos::from_block_pos(pos), 0, || {
                let unlit = vanilla_blocks::CANDLE
                    .default_state()
                    .set_value(&BlockStateProperties::LIT, false);
                assert!(world.set_block(pos, unlit, UpdateFlags::UPDATE_ALL));

                let firework = FireworkRocketEntity::new(
                    &vanilla_entities::FIREWORK_ROCKET,
                    3,
                    DVec3::new(1_136.5, 64.5, 1_136.5),
                    Arc::downgrade(&world),
                );
                firework.set_remaining_fire_ticks(1);
                firework.projectile_on_hit_block(&ClipHitResult {
                    location: firework.position(),
                    direction: Direction::Up,
                    block_pos: pos,
                    miss: false,
                    inside: false,
                    world_border_hit: false,
                });

                assert!(
                    world
                        .get_block_state(pos)
                        .get_value(&BlockStateProperties::LIT)
                );

                let campfire_pos = pos.offset(1, 0, 0);
                let unlit_campfire = vanilla_blocks::CAMPFIRE
                    .default_state()
                    .set_value(&BlockStateProperties::LIT, false)
                    .set_value(&BlockStateProperties::WATERLOGGED, false);
                assert!(
                    world.set_block(campfire_pos, unlit_campfire, UpdateFlags::UPDATE_CLIENTS,)
                );
                firework.projectile_on_hit_block(&ClipHitResult {
                    location: firework.position(),
                    direction: Direction::Up,
                    block_pos: campfire_pos,
                    miss: false,
                    inside: false,
                    world_border_hit: false,
                });
                assert!(
                    world
                        .get_block_state(campfire_pos)
                        .get_value(&BlockStateProperties::LIT)
                );

                let dripleaf_pos = pos.offset(2, 0, 0);
                assert!(world.set_block(
                    dripleaf_pos,
                    vanilla_blocks::BIG_DRIPLEAF.default_state(),
                    UpdateFlags::UPDATE_CLIENTS,
                ));
                firework.projectile_on_hit_block(&ClipHitResult {
                    location: firework.position(),
                    direction: Direction::Up,
                    block_pos: dripleaf_pos,
                    miss: false,
                    inside: false,
                    world_border_hit: false,
                });
                assert_eq!(
                    world
                        .get_block_state(dripleaf_pos)
                        .get_value(&BlockStateProperties::TILT),
                    Tilt::Full
                );

                let chorus_pos = pos.offset(3, 0, 0);
                assert!(world.set_block(
                    chorus_pos,
                    vanilla_blocks::CHORUS_FLOWER.default_state(),
                    UpdateFlags::UPDATE_CLIENTS,
                ));
                firework.projectile_on_hit_block(&ClipHitResult {
                    location: firework.position(),
                    direction: Direction::Up,
                    block_pos: chorus_pos,
                    miss: false,
                    inside: false,
                    world_border_hit: false,
                });
                assert!(world.get_block_state(chorus_pos).is_air());
            });
        let result = chunk_map.chunk_runtime.block_on(block_callback_test);

        assert_eq!(result, Some(()));
    }
