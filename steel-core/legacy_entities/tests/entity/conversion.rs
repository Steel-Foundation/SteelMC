// Moved from entity/conversion.rs with the entities it depends on.

    #[test]
    fn convert_to_refuses_hostile_types_on_peaceful_difficulty() {
        init_vanilla_registry();
        init_entities();
        let world_fixture = conversion_world();
        let world = &world_fixture.world;

        world.set_difficulty(Difficulty::Peaceful);
        let pig = add_pig(world);
        let pig_mob = pig.as_mob().expect("pig is a mob");
        assert!(
            pig.downcast_ref::<ChickenEntity>()
                .expect("pig is concrete")
                .convert_to(
                    &vanilla_entities::ENDERMITE,
                    ConversionParams::single(true, true),
                    None,
                )
                .is_none(),
            "hostile conversions are refused on peaceful"
        );
        assert!(
            !pig_mob.is_removed(),
            "refused conversions leave the source untouched"
        );

        world.set_difficulty(Difficulty::Normal);
        let other_pig = add_pig(world);
        assert!(
            other_pig
                .downcast_ref::<ChickenEntity>()
                .expect("pig is concrete")
                .convert_to(
                    &vanilla_entities::ENDERMITE,
                    ConversionParams::single(true, true),
                    None,
                )
                .is_some(),
            "hostile conversions are allowed on non-peaceful difficulties"
        );
    }
