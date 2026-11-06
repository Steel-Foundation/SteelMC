// Moved from entity/potion_contents.rs with the entities it depends on.

    #[test]
    fn splash_rebuild_takes_its_icon_flag_from_visibility() {
        init_vanilla_registry();
        let effect = RegistryMobEffectInstance::new(
            vanilla_mob_effects::LUCK,
            100,
            0,
            false,
            true,
            false,
            None,
        );

        let drunk = to_runtime_instance(&effect, 100);
        assert!(drunk.is_visible());
        assert!(
            !drunk.show_icon(),
            "drinking preserves the source icon flag"
        );

        let splashed = to_runtime_instance_icon_from_visibility(&effect, 100);
        assert!(splashed.is_visible());
        assert!(
            splashed.show_icon(),
            "the splash rebuild must take show_icon from visible"
        );

        // Everything else stays identical between the two paths.
        assert_eq!(splashed.effect(), drunk.effect());
        assert_eq!(splashed.duration(), drunk.duration());
        assert_eq!(splashed.amplifier(), drunk.amplifier());
        assert_eq!(splashed.is_ambient(), drunk.is_ambient());
    }
