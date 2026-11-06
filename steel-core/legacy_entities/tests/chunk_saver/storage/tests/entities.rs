// Moved from chunk_saver/storage/tests/entities.rs with the entities it depends on.

#[test]
fn proto_entities_roundtrip_and_promote_to_full_chunk() {
    init_globals();

    let pos = ChunkPos::new(0, 0);
    let entity_pos = DVec3::new(5.5, 6.0, 7.5);
    let proto = Chunk::new(single_empty_section(), pos, 0, 16, Weak::new());
    let crystal = Arc::new(EndCrystalEntity::new(
        &vanilla_entities::END_CRYSTAL,
        next_entity_id(),
        entity_pos,
        Weak::new(),
    ));
    crystal.set_beam_target(Some(BlockPos::new(0, 64, 0)));
    crystal.set_invulnerable(true);
    crystal.set_fall_distance(3.75);
    crystal.set_no_gravity(true);
    crystal.set_air_supply(120);
    crystal.set_portal_cooldown(9);
    crystal.set_custom_name(Some(TextComponent::plain("End Test")));
    crystal.set_custom_name_visible(true);
    crystal.set_silent(true);
    crystal.set_glowing_tag(true);
    assert!(crystal.add_tag("steel:test".to_owned()));
    let mut custom_data = NbtCompound::new();
    custom_data.insert("marker", "roundtrip");
    crystal.set_custom_data(custom_data);
    proto.add_entity(crystal);

    let chunk = proto;
    let Some(prepared) =
        ChunkStorage::prepare_chunk_save(&chunk, ChunkStatus::Features, &[], false)
    else {
        panic!("dirty proto chunk should prepare for saving");
    };
    assert_eq!(prepared.persistent.entities.len(), 1);
    assert!((prepared.persistent.entities[0].fall_distance - 3.75).abs() <= f64::EPSILON);
    assert!(prepared.persistent.entities[0].no_gravity);
    assert!(prepared.persistent.entities[0].invulnerable);
    assert_eq!(prepared.persistent.entities[0].air_supply, 120);
    assert_eq!(prepared.persistent.entities[0].portal_cooldown, 9);
    assert!(prepared.persistent.entities[0].custom_name_visible);
    assert!(prepared.persistent.entities[0].silent);
    assert!(prepared.persistent.entities[0].glowing);
    assert_eq!(
        prepared.persistent.entities[0].tags,
        vec!["steel:test".to_owned()]
    );
    assert_ne!(prepared.persistent.entities[0].custom_name_nbt.len(), 0);
    assert_ne!(prepared.persistent.entities[0].custom_data_nbt.len(), 0);
    let custom_name_nbt = read_borrowed_compound(&mut Cursor::new(
        &prepared.persistent.entities[0].custom_name_nbt,
    ))
    .expect("saved custom name should be valid NBT");
    let custom_name_nbt = simdnbt::borrow::NbtCompound::from(&custom_name_nbt);
    assert_eq!(
        custom_name_nbt
            .string("CustomName")
            .map(|value| value.to_str().into_owned()),
        Some("End Test".to_owned())
    );

    let loaded = ChunkStorage::persistent_to_chunk(
        &prepared.persistent,
        pos,
        ChunkStatus::Features,
        0,
        16,
        Weak::new(),
    );
    assert!(loaded.pending_entities.is_empty());
    let loaded_proto = loaded.chunk;
    assert_eq!(loaded_proto.get_entities().len(), 1);

    let promoted = loaded_proto.promote_to_full();
    assert_eq!(promoted.pending_entities.len(), 1);
    assert!(promoted.pending_entities[0].is_no_gravity());
    assert!(promoted.pending_entities[0].is_invulnerable());
    assert_eq!(promoted.pending_entities[0].air_supply(), 120);
    assert_eq!(promoted.pending_entities[0].portal_cooldown(), 9);
    assert_eq!(
        promoted.pending_entities[0].custom_name(),
        Some(TextComponent::plain("End Test"))
    );
    assert!(promoted.pending_entities[0].is_custom_name_visible());
    assert!(promoted.pending_entities[0].is_silent());
    assert!(promoted.pending_entities[0].has_glowing_tag());
    assert_eq!(
        promoted.pending_entities[0].tags(),
        vec!["steel:test".to_owned()]
    );
    assert_eq!(
        promoted.pending_entities[0]
            .custom_data()
            .string("marker")
            .map(ToString::to_string),
        Some("roundtrip".to_owned())
    );
}

#[test]
fn prepared_save_reports_handled_runtime_entity_ids() {
    init_globals();

    let pos = ChunkPos::new(0, 0);
    let proto = Chunk::new(single_empty_section(), pos, 0, 16, Weak::new());
    let chunk = proto;
    let entity: SharedEntity = Arc::new(EndCrystalEntity::new(
        &vanilla_entities::END_CRYSTAL,
        next_entity_id(),
        DVec3::new(5.5, 6.0, 7.5),
        Weak::new(),
    ));

    let Some(prepared) = ChunkStorage::prepare_chunk_save(
        &chunk,
        ChunkStatus::Features,
        slice::from_ref(&entity),
        true,
    ) else {
        panic!("forced runtime entity save should prepare a chunk save");
    };

    assert_eq!(prepared.handled_runtime_entity_ids, vec![entity.id()]);
    assert_eq!(prepared.persistent.entities.len(), 1);
}

#[test]
fn full_chunk_load_defers_entities_to_world_registration() {
    init_globals();

    let pos = ChunkPos::new(0, 0);
    let proto = Chunk::new(single_empty_section(), pos, 0, 16, Weak::new());
    let chunk = proto;
    let entity: SharedEntity = Arc::new(EndCrystalEntity::new(
        &vanilla_entities::END_CRYSTAL,
        next_entity_id(),
        DVec3::new(5.5, 6.0, 7.5),
        Weak::new(),
    ));

    let Some(prepared) = ChunkStorage::prepare_chunk_save(
        &chunk,
        ChunkStatus::Features,
        slice::from_ref(&entity),
        true,
    ) else {
        panic!("forced runtime entity save should prepare a chunk save");
    };

    let loaded = ChunkStorage::persistent_to_chunk(
        &prepared.persistent,
        pos,
        ChunkStatus::Full,
        0,
        16,
        Weak::new(),
    );

    assert_eq!(loaded.status, ChunkStatus::Full);
    assert_eq!(loaded.pending_entities.len(), 1);
    assert_eq!(loaded.pending_entities[0].uuid(), entity.uuid());
}

#[test]
fn runtime_entity_passengers_save_nested_and_load_flattened_for_registration() {
    init_globals();

    let pos = ChunkPos::new(0, 0);
    let proto = Chunk::new(single_empty_section(), pos, 0, 16, Weak::new());
    let chunk = proto;
    let vehicle: SharedEntity = Arc::new(EndCrystalEntity::new(
        &vanilla_entities::END_CRYSTAL,
        next_entity_id(),
        DVec3::new(5.5, 6.0, 7.5),
        Weak::new(),
    ));
    let passenger: SharedEntity = Arc::new(EndCrystalEntity::new(
        &vanilla_entities::END_CRYSTAL,
        next_entity_id(),
        DVec3::new(5.5, 8.0, 7.5),
        Weak::new(),
    ));
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);
    let vehicle_uuid = vehicle.uuid();
    let passenger_uuid = passenger.uuid();
    let entities = [Arc::clone(&vehicle), Arc::clone(&passenger)];

    let Some(prepared) =
        ChunkStorage::prepare_chunk_save(&chunk, ChunkStatus::Features, &entities, true)
    else {
        panic!("forced runtime entity save should prepare a chunk save");
    };

    assert_eq!(prepared.persistent.entities.len(), 1);
    assert_eq!(
        prepared.persistent.entities[0].uuid,
        *vehicle_uuid.as_bytes()
    );
    assert_eq!(prepared.persistent.entities[0].passengers.len(), 1);
    assert_eq!(
        prepared.persistent.entities[0].passengers[0].uuid,
        *passenger_uuid.as_bytes()
    );

    let loaded = ChunkStorage::persistent_to_chunk(
        &prepared.persistent,
        pos,
        ChunkStatus::Full,
        0,
        16,
        Weak::new(),
    );

    assert_eq!(loaded.status, ChunkStatus::Full);
    assert_eq!(loaded.pending_entities.len(), 2);
    let Some(loaded_passenger) = loaded
        .pending_entities
        .iter()
        .find(|entity| entity.uuid() == passenger_uuid)
    else {
        panic!("passenger should load into pending registration list");
    };
    let Some(loaded_vehicle) = loaded_passenger.vehicle() else {
        panic!("passenger should restore its vehicle relationship");
    };
    assert_eq!(loaded_vehicle.uuid(), vehicle_uuid);
    assert!(loaded_vehicle.has_passenger(loaded_passenger.as_ref()));
}

#[test]
fn runtime_entity_passengers_skip_non_serializable_entities_like_vanilla() {
    init_globals();

    let pos = ChunkPos::new(0, 0);
    let proto = Chunk::new(single_empty_section(), pos, 0, 16, Weak::new());
    let chunk = proto;
    let vehicle: SharedEntity = Arc::new(EndCrystalEntity::new(
        &vanilla_entities::END_CRYSTAL,
        next_entity_id(),
        DVec3::new(5.5, 6.0, 7.5),
        Weak::new(),
    ));
    let passenger: SharedEntity = TestEntity::shared(
        next_entity_id(),
        DVec3::new(5.5, 8.0, 7.5),
        Weak::new(),
        &vanilla_entities::PLAYER,
    );
    EntityBase::restore_passenger_relationship(&vehicle, &passenger);
    let vehicle_uuid = vehicle.uuid();

    let Some(prepared) = ChunkStorage::prepare_chunk_save(
        &chunk,
        ChunkStatus::Features,
        slice::from_ref(&vehicle),
        true,
    ) else {
        panic!("forced runtime entity save should prepare a chunk save");
    };

    assert_eq!(prepared.persistent.entities.len(), 1);
    assert_eq!(
        prepared.persistent.entities[0].uuid,
        *vehicle_uuid.as_bytes()
    );
    assert!(prepared.persistent.entities[0].passengers.is_empty());

    let loaded = ChunkStorage::persistent_to_chunk(
        &prepared.persistent,
        pos,
        ChunkStatus::Full,
        0,
        16,
        Weak::new(),
    );

    assert_eq!(loaded.status, ChunkStatus::Full);
    assert_eq!(loaded.pending_entities.len(), 1);
    assert_eq!(loaded.pending_entities[0].uuid(), vehicle_uuid);
}
