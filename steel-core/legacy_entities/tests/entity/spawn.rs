// Moved from entity/spawn.rs with the entities it depends on.

    #[test]
    fn item_sheep_color_overrides_spawn_state() {
        init_vanilla_registry();

        let sheep = Arc::new(ChickenEntity::new(
            &vanilla_entities::CHICKEN,
            1,
            DVec3::ZERO,
            Weak::new(),
        ));
        let entity: SharedEntity = Arc::<ChickenEntity>::clone(&sheep);

        let mut spawn_egg = ItemStack::new(&vanilla_items::SHEEP_SPAWN_EGG);
        spawn_egg.set(SHEEP_COLOR, DyeColor::Pink);

        apply_item_stack_components(&entity, &spawn_egg, false)
            .expect("valid sheep color should apply");

        assert_eq!(sheep.color(), DyeColor::Pink);
    }
// Moved from entity/spawn.rs with the entities it depends on.

    #[test]
    fn item_cow_components_override_spawn_state() {
        init_vanilla_registry();

        let cow = Arc::new(ChickenEntity::new(
            &vanilla_entities::CHICKEN,
            1,
            DVec3::ZERO,
            Weak::new(),
        ));
        let entity: SharedEntity = Arc::<ChickenEntity>::clone(&cow);

        let mut spawn_egg = ItemStack::new(&vanilla_items::COW_SPAWN_EGG);
        spawn_egg.set(
            COW_VARIANT,
            RegistryReference::new(&vanilla_cow_variants::COLD),
        );
        spawn_egg.set(
            COW_SOUND_VARIANT,
            RegistryReference::new(&vanilla_cow_sound_variants::MOODY),
        );

        apply_item_stack_components(&entity, &spawn_egg, false)
            .expect("valid cow components should apply");

        assert_eq!(cow.variant().key, vanilla_cow_variants::COLD.key);
        assert_eq!(
            cow.sound_variant().key,
            vanilla_cow_sound_variants::MOODY.key
        );
    }
