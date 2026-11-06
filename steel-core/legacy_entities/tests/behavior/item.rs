// Moved from behavior/item.rs with the entities it depends on.

    #[test]
    fn potion_items_default_to_their_vanilla_base_potion() {
        init_vanilla_registry();
        init_behaviors();

        for item in [
            &vanilla_items::POTION,
            &vanilla_items::SPLASH_POTION,
            &vanilla_items::LINGERING_POTION,
        ] {
            let stack = ITEM_BEHAVIORS.default_instance(item);
            assert!(stack.is(item));
            assert_eq!(stack.count(), 1);
            let contents = stack
                .get(vanilla_components::POTION_CONTENTS)
                .expect("potion default instance carries potion contents");
            assert!(contents.is(&vanilla_potions::WATER));
        }

        let arrow = ITEM_BEHAVIORS.default_instance(&vanilla_items::TIPPED_ARROW);
        let contents = arrow
            .get(vanilla_components::POTION_CONTENTS)
            .expect("tipped arrow default instance carries potion contents");
        assert!(contents.is(&vanilla_potions::POISON));
    }
