// Moved from enchantment_helper.rs with the entities it depends on.

    #[test]
    fn projectile_spawned_effect_ignites_firework() {
        init_vanilla_registry();

        let projectile = FireworkRocketEntity::new(
            &vanilla_entities::FIREWORK_ROCKET,
            2,
            DVec3::ZERO,
            Weak::new(),
        );
        let mut rocket = enchanted_item(
            &vanilla_items::FIREWORK_ROCKET,
            Identifier::vanilla_static("flame"),
            1,
        );

        on_projectile_spawned(test_world(), &mut rocket, &projectile, None);

        assert_eq!(projectile.remaining_fire_ticks(), 2_000);
    }
