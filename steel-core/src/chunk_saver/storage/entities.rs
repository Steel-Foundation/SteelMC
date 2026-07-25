use super::*;

impl ChunkStorage {
    pub(super) fn entities_to_persistent(entities: &[SharedEntity]) -> Vec<PersistentEntity> {
        let mut visited = FxHashSet::default();
        entities
            .iter()
            .filter(|entity| !entity.is_passenger())
            .filter_map(|entity| {
                Self::entity_to_persistent(entity, &mut visited, EntityPersistenceMode::ChunkSave)
            })
            .collect()
    }

    pub(crate) fn entity_tree_to_persistent(entity: &SharedEntity) -> Option<PersistentEntity> {
        let mut visited = FxHashSet::default();
        Self::entity_to_persistent(entity, &mut visited, EntityPersistenceMode::ChunkSave)
    }

    pub(crate) fn entity_to_dimension_transition_persistent(
        entity: &SharedEntity,
    ) -> Option<PersistentEntity> {
        let mut visited = FxHashSet::default();
        Self::entity_to_persistent(
            entity,
            &mut visited,
            EntityPersistenceMode::DimensionTransition,
        )
    }

    pub(super) fn custom_name_to_persistent(custom_name: Option<&TextComponent>) -> Vec<u8> {
        let Some(custom_name) = custom_name else {
            return Vec::new();
        };

        let mut root = NbtCompound::new();
        root.insert("CustomName", custom_name.to_codec_nbt());
        let mut bytes = Vec::new();
        root.write(&mut bytes);
        bytes
    }

    pub(super) fn custom_name_from_persistent(
        bytes: &[u8],
        uuid: uuid::Uuid,
    ) -> Option<TextComponent> {
        if bytes.is_empty() {
            return None;
        }

        let Ok(root) = read_borrowed_compound(&mut Cursor::new(bytes)) else {
            tracing::warn!(
                ?uuid,
                "Failed to parse entity custom name NBT, defaulting to no custom name"
            );
            return None;
        };
        let root = simdnbt::borrow::NbtCompound::from(&root);
        let tag = root.get("CustomName")?;
        let custom_name = TextComponent::from_nbt(&tag.to_owned());
        if custom_name.is_none() {
            tracing::warn!(
                ?uuid,
                "Failed to decode entity custom name, defaulting to no custom name"
            );
            return None;
        }
        custom_name
    }

    pub(super) fn compound_to_persistent(compound: &NbtCompound) -> Vec<u8> {
        if compound.is_empty() {
            return Vec::new();
        }

        let mut bytes = Vec::new();
        compound.write(&mut bytes);
        bytes
    }

    pub(super) fn compound_from_persistent(bytes: &[u8], uuid: uuid::Uuid) -> NbtCompound {
        if bytes.is_empty() {
            return NbtCompound::new();
        }

        let Ok(compound) = read_borrowed_compound(&mut Cursor::new(bytes)) else {
            tracing::warn!(
                ?uuid,
                "Failed to parse entity custom data NBT, defaulting to empty custom data"
            );
            return NbtCompound::new();
        };
        simdnbt::borrow::NbtCompound::from(&compound).to_owned()
    }

    pub(super) fn save_data_from_persistent(
        persistent: &PersistentEntity,
        uuid: uuid::Uuid,
    ) -> EntityBaseSaveData {
        EntityBaseSaveData {
            air_supply: persistent.air_supply,
            portal_cooldown: persistent.portal_cooldown,
            no_gravity: persistent.no_gravity,
            invulnerable: persistent.invulnerable,
            custom_name: Self::custom_name_from_persistent(&persistent.custom_name_nbt, uuid),
            custom_name_visible: persistent.custom_name_visible,
            silent: persistent.silent,
            glowing: persistent.glowing,
            tags: persistent
                .tags
                .iter()
                .take(MAX_ENTITY_TAGS)
                .cloned()
                .collect(),
            custom_data: Self::compound_from_persistent(&persistent.custom_data_nbt, uuid),
        }
    }

    pub(super) fn clamp_loaded_entity_position(pos: DVec3) -> DVec3 {
        DVec3::new(
            pos.x.clamp(
                -ENTITY_LOAD_MAX_HORIZONTAL_POSITION,
                ENTITY_LOAD_MAX_HORIZONTAL_POSITION,
            ),
            pos.y.clamp(
                -ENTITY_LOAD_MAX_VERTICAL_POSITION,
                ENTITY_LOAD_MAX_VERTICAL_POSITION,
            ),
            pos.z.clamp(
                -ENTITY_LOAD_MAX_HORIZONTAL_POSITION,
                ENTITY_LOAD_MAX_HORIZONTAL_POSITION,
            ),
        )
    }

    pub(super) fn entity_to_persistent(
        entity: &SharedEntity,
        visited: &mut FxHashSet<i32>,
        mode: EntityPersistenceMode,
    ) -> Option<PersistentEntity> {
        if !Self::entity_should_persist(entity.as_ref(), mode) {
            return None;
        }

        if !visited.insert(entity.id()) {
            tracing::warn!(
                uuid = ?entity.uuid(),
                "Entity passenger tree contains duplicate entity id {}, skipping duplicate save",
                entity.id()
            );
            return None;
        }

        let pos = entity.position();
        let stored_pos = if let Some(vehicle) = entity.vehicle() {
            let vehicle_pos = vehicle.position();
            DVec3::new(vehicle_pos.x, pos.y, vehicle_pos.z)
        } else {
            pos
        };
        let vel = entity.velocity();
        let (yaw, pitch) = entity.rotation();
        let fire_freeze = entity.fire_freeze_state();
        let save_data = entity.base().save_data();

        if !stored_pos.x.is_finite() || !stored_pos.y.is_finite() || !stored_pos.z.is_finite() {
            tracing::warn!(
                uuid = ?entity.uuid(),
                "Entity has non-finite position {:?}, skipping save",
                stored_pos
            );
            return None;
        }

        let mut nbt = NbtCompound::new();
        entity.save_additional(&mut nbt);
        let mut nbt_bytes = Vec::new();
        nbt.write(&mut nbt_bytes);

        let passengers = entity
            .passengers()
            .iter()
            .filter_map(|passenger| Self::entity_to_persistent(passenger, visited, mode))
            .collect();

        Some(PersistentEntity {
            entity_type: entity.entity_type().key.clone(),
            uuid: *entity.uuid().as_bytes(),
            pos: [stored_pos.x, stored_pos.y, stored_pos.z],
            motion: [vel.x, vel.y, vel.z],
            rotation: [yaw, pitch],
            fall_distance: entity.fall_distance(),
            remaining_fire_ticks: fire_freeze.remaining_fire_ticks(),
            ticks_frozen: fire_freeze.ticks_frozen(),
            is_in_powder_snow: fire_freeze.is_in_powder_snow(),
            was_in_powder_snow: fire_freeze.was_in_powder_snow(),
            has_visual_fire: fire_freeze.has_visual_fire(),
            on_ground: entity.on_ground(),
            no_gravity: save_data.no_gravity,
            invulnerable: save_data.invulnerable,
            air_supply: save_data.air_supply,
            portal_cooldown: save_data.portal_cooldown,
            custom_name_nbt: Self::custom_name_to_persistent(save_data.custom_name.as_ref()),
            custom_name_visible: save_data.custom_name_visible,
            silent: save_data.silent,
            glowing: save_data.glowing,
            tags: save_data.tags.iter().cloned().collect(),
            custom_data_nbt: Self::compound_to_persistent(&save_data.custom_data),
            nbt_data: nbt_bytes,
            passengers,
        })
    }

    pub(super) fn entity_should_save(entity: &dyn Entity) -> bool {
        (!entity.is_removed()
            || entity
                .removal_reason()
                .is_some_and(RemovalReason::should_save))
            && entity.entity_type().can_serialize
    }

    pub(super) fn entity_should_persist(entity: &dyn Entity, mode: EntityPersistenceMode) -> bool {
        match mode {
            EntityPersistenceMode::ChunkSave => Self::entity_should_save(entity),
            EntityPersistenceMode::DimensionTransition => !entity.is_removed(),
        }
    }

    /// Converts a runtime section to persistent format.
    pub(super) fn persistent_block_entity_pos(
        persistent: &PersistentBlockEntity,
        chunk_pos: ChunkPos,
    ) -> BlockPos {
        let abs_x = chunk_pos.0.x * 16 + i32::from(persistent.x);
        let abs_z = chunk_pos.0.y * 16 + i32::from(persistent.z);
        BlockPos::new(abs_x, i32::from(persistent.y), abs_z)
    }

    /// Converts a persistent block entity to runtime format.
    pub(super) fn persistent_to_block_entity(
        persistent: &PersistentBlockEntity,
        chunk_pos: ChunkPos,
        chunk: &LevelChunk,
    ) -> Option<SharedBlockEntity> {
        let pos = Self::persistent_block_entity_pos(persistent, chunk_pos);
        let state = chunk.get_block_state(pos);
        Self::persistent_to_block_entity_at(persistent, pos, chunk.level_weak(), state)
    }

    pub(super) fn persistent_to_block_entity_at(
        persistent: &PersistentBlockEntity,
        pos: BlockPos,
        level: Weak<World>,
        state: BlockStateId,
    ) -> Option<SharedBlockEntity> {
        // Look up the block entity type
        let block_entity_type_key = persistent.entity_type.as_ref()?;
        let block_entity_type = REGISTRY.block_entity_types.by_key(block_entity_type_key)?;
        if !block_entity_type.is_valid(state.get_block()) {
            log::warn!(
                "Skipping block entity {} at {pos:?}: block {} does not accept that type",
                block_entity_type.key,
                state.get_block().key,
            );
            return None;
        }

        // Parse and load NBT data
        if persistent.nbt_data.is_empty() {
            // No NBT data, just create the entity without loading
            Some(BLOCK_ENTITIES.create_or_raw(block_entity_type, level, pos, state))
        } else {
            // Parse NBT from bytes as borrowed
            let Ok(nbt) = read_borrowed_compound(&mut Cursor::new(&persistent.nbt_data)) else {
                log::warn!(
                    "Skipping block entity {} at {pos:?}: malformed NBT",
                    block_entity_type.key,
                );
                return None;
            };

            // Create the block entity and load NBT
            Some(BLOCK_ENTITIES.create_and_load_or_raw(block_entity_type, level, pos, state, &nbt))
        }
    }

    /// Converts a persistent entity tree to runtime format.
    pub(crate) fn persistent_to_entity_tree_at_level(
        persistent: &PersistentEntity,
        chunk_pos: ChunkPos,
        level: &Weak<World>,
    ) -> Vec<SharedEntity> {
        let mut entities = Vec::new();
        let Some(entity) = Self::persistent_to_entity_at_level(persistent, chunk_pos, level) else {
            return entities;
        };

        entities.push(Arc::clone(&entity));
        for persistent_passenger in &persistent.passengers {
            Self::load_persistent_passenger_tree(
                persistent_passenger,
                chunk_pos,
                level,
                &entity,
                &mut entities,
            );
        }
        entities
    }

    pub(super) fn load_persistent_passenger_tree(
        persistent: &PersistentEntity,
        chunk_pos: ChunkPos,
        level: &Weak<World>,
        vehicle: &SharedEntity,
        entities: &mut Vec<SharedEntity>,
    ) {
        let Some(passenger) = Self::persistent_to_entity_at_level(persistent, chunk_pos, level)
        else {
            return;
        };

        EntityBase::restore_passenger_relationship(vehicle, &passenger);
        entities.push(Arc::clone(&passenger));
        for persistent_passenger in &persistent.passengers {
            Self::load_persistent_passenger_tree(
                persistent_passenger,
                chunk_pos,
                level,
                &passenger,
                entities,
            );
        }
    }

    /// Converts one persistent entity to runtime format without loading passengers.
    pub(super) fn persistent_to_entity_at_level(
        persistent: &PersistentEntity,
        chunk_pos: ChunkPos,
        level: &Weak<World>,
    ) -> Option<SharedEntity> {
        use uuid::Uuid;

        // Reconstruct base fields
        let stored_pos = DVec3::new(persistent.pos[0], persistent.pos[1], persistent.pos[2]);
        let mut velocity = DVec3::new(
            persistent.motion[0],
            persistent.motion[1],
            persistent.motion[2],
        );
        let rotation = (persistent.rotation[0], persistent.rotation[1]);
        let uuid = Uuid::from_bytes(persistent.uuid);

        // Validate position is finite
        if !stored_pos.x.is_finite() || !stored_pos.y.is_finite() || !stored_pos.z.is_finite() {
            tracing::warn!(
                ?uuid,
                "Entity has non-finite position {:?}, skipping load",
                stored_pos
            );
            return None;
        }

        if !rotation.0.is_finite() || !rotation.1.is_finite() {
            tracing::warn!(
                ?uuid,
                "Entity has non-finite rotation {rotation:?}, skipping load"
            );
            return None;
        }

        let pos = Self::clamp_loaded_entity_position(stored_pos);

        // Validate position is within expected chunk (sanity check)
        let expected_chunk = ChunkPos::from_entity_pos(pos);
        if chunk_pos != expected_chunk {
            tracing::warn!(
                ?uuid,
                "Entity position {:?} doesn't match chunk {:?}, loading anyway",
                pos,
                chunk_pos
            );
        }

        // Clamp motion values > 10.0 to 0 (vanilla behavior to prevent corruption)
        if velocity.x.abs() > 10.0 {
            velocity.x = 0.0;
        }
        if velocity.y.abs() > 10.0 {
            velocity.y = 0.0;
        }
        if velocity.z.abs() > 10.0 {
            velocity.z = 0.0;
        }

        // Look up entity type
        let entity_type = REGISTRY.entity_types.by_key(&persistent.entity_type)?;
        let save_data = Self::save_data_from_persistent(persistent, uuid);

        // Parse NBT from bytes (or use empty compound data)
        let nbt_bytes = if persistent.nbt_data.is_empty() {
            // Empty compound body for `simdnbt::borrow::read_compound`.
            &[0x00][..]
        } else {
            &persistent.nbt_data[..]
        };

        let Ok(nbt) = read_borrowed_compound(&mut Cursor::new(nbt_bytes)) else {
            tracing::warn!(?uuid, "Failed to parse entity NBT, skipping");
            return None;
        };

        Some(ENTITIES.create_and_load_or_raw(
            EntityLoadRequest {
                entity_type,
                position: pos,
                uuid,
                velocity,
                rotation,
                fall_distance: persistent.fall_distance,
                fire_freeze: EntityFireFreezeState::from_parts(
                    persistent.remaining_fire_ticks,
                    persistent.ticks_frozen,
                    persistent.is_in_powder_snow,
                    persistent.was_in_powder_snow,
                    persistent.has_visual_fire,
                ),
                on_ground: persistent.on_ground,
                save_data,
                world: Weak::clone(level),
            },
            &nbt,
        ))
    }
}
