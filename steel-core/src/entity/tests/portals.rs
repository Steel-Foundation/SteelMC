use super::*;

#[test]
fn default_tick_runs_vanilla_entity_base_tick() {
    let entity = PushableTestEntity::shared(1, DVec3::ZERO);
    entity.base().set_boarding_cooldown(2);
    entity.base().set_portal_cooldown(2);

    entity.default_tick();

    assert_eq!(entity.base().boarding_cooldown(), 1);
    assert_eq!(entity.base().portal_cooldown(), 1);
}

#[test]
fn can_use_portal_requires_alive_entity() {
    let entity = PushableTestEntity::shared(1, DVec3::ZERO);
    assert!(entity.can_use_portal(false));

    entity.set_removed(RemovalReason::Discarded);

    assert!(!entity.can_use_portal(true));
}

#[test]
fn static_vanilla_portal_overrides_reject_special_entities() {
    let fishing_hook = TypedTestEntity::new(1, &vanilla_entities::FISHING_BOBBER);
    let dragon = TypedTestEntity::new(2, &vanilla_entities::ENDER_DRAGON);
    let wither = TypedTestEntity::new(3, &vanilla_entities::WITHER);

    assert!(!fishing_hook.can_use_portal(true));
    assert!(!dragon.can_use_portal(true));
    assert!(!wither.can_use_portal(true));
}

#[test]
fn projectile_owner_uuid_reports_projectile_owner_identity() {
    let owner_uuid = Uuid::from_u128(42);
    let pearl = TypedTestEntity::projectile_with_owner_uuid(1, owner_uuid);
    let no_player_owner = TypedTestEntity::new(3, &vanilla_entities::ENDER_PEARL);

    assert_eq!(pearl.projectile_owner_uuid(), Some(owner_uuid));
    assert_eq!(no_player_owner.projectile_owner_uuid(), None);
}

#[test]
fn can_use_portal_respects_passenger_gate() {
    init_test_registry();

    let passenger = PushableTestEntity::shared(1, DVec3::ZERO);
    let vehicle = PushableTestEntity::shared(2, DVec3::ZERO);
    assert!(start_riding_entities(&passenger, &vehicle));

    assert!(!passenger.can_use_portal(false));
    assert!(passenger.can_use_portal(true));
}

#[test]
fn indirect_passengers_match_vanilla_preorder() {
    let vehicle = MultiPassengerTestEntity::shared(1);
    let first = MultiPassengerTestEntity::shared(2);
    let second = MultiPassengerTestEntity::shared(3);
    let nested = MultiPassengerTestEntity::shared(4);

    EntityBase::restore_passenger_relationship(&vehicle, &first);
    EntityBase::restore_passenger_relationship(&vehicle, &second);
    EntityBase::restore_passenger_relationship(&first, &nested);

    let passenger_ids = indirect_passengers(vehicle.as_ref())
        .into_iter()
        .map(|passenger| passenger.id())
        .collect::<Vec<_>>();

    assert_eq!(passenger_ids, vec![2, 4, 3]);
}

#[test]
fn passenger_transition_rotation_matches_vanilla_relative_flags() {
    let vehicle_rotation = (30.0, 10.0);
    let passenger_rotation = (70.0, -5.0);

    assert_eq!(
        passenger_transition_rotation(
            (90.0, 20.0),
            RelativeMovement::NONE,
            vehicle_rotation,
            passenger_rotation,
        ),
        (130.0, 5.0),
    );
    assert_eq!(
        passenger_transition_rotation(
            (15.0, -3.0),
            RelativeMovement::ROTATION,
            vehicle_rotation,
            passenger_rotation,
        ),
        (15.0, -3.0),
    );
    assert_eq!(
        passenger_transition_rotation(
            (-90.0, 0.0),
            RelativeMovement::new(RelativeMovement::X_ROT),
            vehicle_rotation,
            passenger_rotation,
        ),
        (-50.0, 0.0),
    );
}

#[test]
fn passenger_transition_position_preserves_vehicle_offset() {
    assert_eq!(
        passenger_transition_position(
            DVec3::new(100.0, 70.0, -40.0),
            RelativeMovement::NONE,
            DVec3::new(10.0, 64.0, 20.0),
            DVec3::new(12.5, 65.0, 17.0),
        ),
        DVec3::new(102.5, 71.0, -43.0),
    );
}

#[test]
fn dimension_transition_persistence_keeps_non_chunk_serializable_entities() {
    let entity: SharedEntity = Arc::new(TypedTestEntity::new(1, &vanilla_entities::FISHING_BOBBER));
    entity
        .base()
        .set_position_local(DVec3::new(12.25, 64.0, -8.75));
    entity.set_rotation((45.0, -10.0));
    entity.set_velocity(DVec3::new(0.1, 0.2, 0.3));

    assert!(ChunkStorage::entity_tree_to_persistent(&entity).is_none());
    let persistent = ChunkStorage::entity_to_dimension_transition_persistent(&entity)
        .expect("dimension transitions mirror vanilla saveWithoutId without chunk-save filtering");

    assert_eq!(persistent.entity_type, vanilla_entities::FISHING_BOBBER.key);
    assert_eq!(
        persistent.pos.map(f64::to_bits),
        [12.25_f64, 64.0, -8.75].map(f64::to_bits),
    );
    assert_eq!(
        persistent.rotation.map(f32::to_bits),
        [45.0_f32, -10.0].map(f32::to_bits),
    );
    assert_eq!(
        persistent.motion.map(f64::to_bits),
        [0.1_f64, 0.2, 0.3].map(f64::to_bits),
    );
}

#[test]
fn remove_after_changing_dimensions_clears_old_mob_leash_and_equipment() {
    init_test_registry();

    let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
    let holder: SharedEntity = Arc::new(PigEntity::new(
        &vanilla_entities::PIG,
        2,
        DVec3::new(1.0, 0.0, 0.0),
        Weak::new(),
    ));
    let Some(mob) = pig.as_mob() else {
        panic!("pig should expose mob behavior");
    };
    assert!(mob.set_leashed_to(&holder));
    pig.living_base().equipment().lock().set(
        EquipmentSlot::Saddle,
        ItemStack::new(&vanilla_items::SADDLE),
    );

    remove_after_changing_dimensions(&pig);

    assert!(!mob.is_leashed());
    assert!(
        pig.living_base()
            .equipment()
            .lock()
            .get_ref(EquipmentSlot::Saddle)
            .is_empty()
    );
}

#[test]
fn can_use_portal_rejects_sleeping_living_entities() {
    init_test_registry();

    let entity = LivingFluidTestEntity::new(0.0, 0.0, true);
    assert!(entity.can_use_portal(false));

    entity.set_sleeping_pos(BlockPos::ZERO);

    assert!(!entity.can_use_portal(false));
}

#[test]
fn dimension_changing_delay_uses_vanilla_class_overrides() {
    let base = TypedTestEntity::new(1, &vanilla_entities::ITEM);
    assert_eq!(base.dimension_changing_delay(), 300);

    let unimplemented_minecart = TypedTestEntity::new(2, &vanilla_entities::MINECART);
    assert_eq!(unimplemented_minecart.dimension_changing_delay(), 300);

    let minecart = ChestMinecartEntity::new(
        &vanilla_entities::CHEST_MINECART,
        3,
        DVec3::ZERO,
        Weak::new(),
    );
    assert_eq!(minecart.dimension_changing_delay(), 10);

    let arrow = TypedTestEntity::new(4, &vanilla_entities::ARROW);
    assert_eq!(arrow.dimension_changing_delay(), 2);
}

#[test]
fn set_as_inside_portal_starts_portal_process_when_not_on_cooldown() {
    let entity = TypedTestEntity::new(1, &vanilla_entities::ITEM);
    let entry_position = BlockPos::new(2, 64, 2);

    entity.set_as_inside_portal(PortalKind::Nether, entry_position);

    let process = entity.base().portal_process().expect("portal process");
    assert_eq!(process.portal(), PortalKind::Nether);
    assert_eq!(process.entry_position(), entry_position);
}

#[test]
fn set_as_inside_portal_resets_cooldown_without_starting_process() {
    let entity = TypedTestEntity::new(1, &vanilla_entities::ARROW);
    entity.set_portal_cooldown(1);

    entity.set_as_inside_portal(PortalKind::Nether, BlockPos::new(2, 64, 2));

    assert_eq!(entity.portal_cooldown(), 2);
    assert_eq!(entity.base().portal_process(), None);
}
