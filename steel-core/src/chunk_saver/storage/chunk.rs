use super::{
    BlockPos, BlockStateId, BlockTickList, CarvingMask, ChunkAccess, ChunkBuilder, ChunkHeightmaps,
    ChunkPos, ChunkSection, ChunkStatus, ChunkStorage, FluidTickList, Heightmap, HeightmapType,
    LevelChunk, LoadedChunk, Ordering, PalettedContainer, PersistentBiomeData, PersistentChunk,
    PersistentHeightmap, PersistentPoi, PersistentSection, ProtoChunk, REGISTRY, RegistryEntry,
    RegistryExt, SectionHolder, Sections, Weak, World, bits_for_palette_len, pack_indices,
    unpack_indices, vanilla_biomes,
};

impl ChunkStorage {
    /// Converts a runtime section to persistent format.
    pub(super) fn section_to_persistent(
        section: &SectionHolder,
        builder: &mut ChunkBuilder,
    ) -> PersistentSection {
        let section = section.read();
        let biomes = Self::biomes_to_persistent(&section.biomes, builder);

        match &section.states {
            PalettedContainer::Homogeneous(block_id) => {
                let block_idx = builder.ensure_block_state(*block_id);
                PersistentSection::Homogeneous {
                    block_state: block_idx,
                    biomes,
                }
            }
            PalettedContainer::Heterogeneous(data) => {
                // Build section-local palette (indices into chunk's block_states)
                let palette: Vec<u16> = data
                    .palette
                    .iter()
                    .map(|(block_id, _)| builder.ensure_block_state(*block_id))
                    .collect();

                // Pack block indices (indices into section-local palette)
                let bits = bits_for_palette_len(palette.len())
                    .expect("Heterogeneous section should have palette length >= 2");
                let indices: Vec<u32> = data
                    .cube
                    .iter()
                    .flatten()
                    .flatten()
                    .map(|block_id| {
                        data.palette
                            .iter()
                            .position(|(v, _)| v == block_id)
                            .unwrap_or(0) as u32
                    })
                    .collect();

                let block_data = pack_indices(&indices, bits);

                PersistentSection::Heterogeneous {
                    palette,
                    bits_per_entry: bits,
                    block_data,
                    biomes,
                }
            }
            PalettedContainer::Building(_) => panic!(
                "section_to_persistent called on a section still in worldgen Building mode; \
                 finalize_building must be called before serialization"
            ),
        }
    }

    /// Converts runtime biome data to persistent format.
    pub(super) fn biomes_to_persistent(
        biomes: &PalettedContainer<u16, 4>,
        builder: &mut ChunkBuilder,
    ) -> PersistentBiomeData {
        match biomes {
            PalettedContainer::Homogeneous(biome_id) => {
                let biome_idx = builder.ensure_biome(*biome_id);
                PersistentBiomeData::Homogeneous { biome: biome_idx }
            }
            PalettedContainer::Heterogeneous(data) => {
                // Build section-local palette (indices into chunk's biomes)
                let palette: Vec<u16> = data
                    .palette
                    .iter()
                    .map(|(biome_id, _)| builder.ensure_biome(*biome_id))
                    .collect();

                let bits = bits_for_palette_len(palette.len())
                    .expect("Heterogeneous biome data should have palette length >= 2");
                let indices: Vec<u32> = data
                    .cube
                    .iter()
                    .flatten()
                    .flatten()
                    .map(|biome_id| {
                        data.palette
                            .iter()
                            .position(|(v, _)| v == biome_id)
                            .unwrap_or(0) as u32
                    })
                    .collect();

                let biome_data = pack_indices(&indices, bits);

                PersistentBiomeData::Heterogeneous {
                    palette,
                    bits_per_entry: bits,
                    biome_data,
                }
            }
            PalettedContainer::Building(_) => panic!(
                "biomes_to_persistent called on a section still in worldgen Building mode; \
                 finalize_building must be called before serialization"
            ),
        }
    }

    /// Converts a persistent chunk to runtime format.
    /// The returned chunk is not dirty (freshly loaded from disk).
    ///
    /// # Arguments
    /// * `persistent` - The persistent chunk data
    /// * `pos` - The chunk position
    /// * `status` - The chunk status
    /// * `min_y` - The minimum Y coordinate of the world
    /// * `height` - The total height of the world
    /// * `level` - Weak reference to the world for `LevelChunk`
    #[expect(
        clippy::too_many_lines,
        reason = "chunk persistence conversion is a linear field-by-field transform"
    )]
    pub(crate) fn persistent_to_chunk(
        persistent: &PersistentChunk<'_>,
        pos: ChunkPos,
        status: ChunkStatus,
        min_y: i32,
        height: i32,
        level: Weak<World>,
    ) -> LoadedChunk {
        let sections: Vec<ChunkSection> = persistent
            .sections
            .iter()
            .map(|section| Self::persistent_to_section(section, persistent))
            .collect();

        // Reconstruct structure data
        let structure_starts = Self::persistent_to_structure_starts(&persistent.structure_starts);
        let structure_references =
            Self::persistent_to_structure_references(&persistent.structure_references);
        let light = Self::persistent_to_light(&persistent.light, min_y, height, status);

        if status == ChunkStatus::Full {
            // Reconstruct scheduled ticks from persistent data
            let block_ticks = BlockTickList::from_saved_ticks(
                Self::persistent_to_block_saved_ticks(&persistent.block_ticks, pos),
            );
            let fluid_ticks = FluidTickList::from_saved_ticks(
                Self::persistent_to_fluid_saved_ticks(&persistent.fluid_ticks, pos),
            );

            // Reconstruct heightmaps from persistent data
            let heightmaps = Self::persistent_to_heightmaps(&persistent.heightmaps, min_y, height);

            let chunk = LevelChunk::from_disk(
                Sections::from_owned(sections.into_boxed_slice()),
                pos,
                min_y,
                height,
                level.clone(),
                block_ticks,
                fluid_ticks,
                heightmaps,
                persistent.postprocessing.iter().map(Vec::clone).collect(),
                structure_starts,
                structure_references,
                light,
            );

            // Load block entities
            for persistent_be in &persistent.block_entities {
                if persistent_be.entity_type.is_none() {
                    let block_entity_pos = Self::persistent_block_entity_pos(persistent_be, pos);
                    chunk.set_pending_block_entity(block_entity_pos);
                    continue;
                }
                if let Some(block_entity) =
                    Self::persistent_to_block_entity(persistent_be, pos, &chunk)
                {
                    let _ = chunk.add_and_register_block_entity(block_entity);
                }
            }

            let mut pending_entities = Vec::with_capacity(persistent.entities.len());
            let level_weak = chunk.level_weak();
            for persistent_entity in &persistent.entities {
                let mut loaded_entities =
                    Self::persistent_to_entity_tree_at_level(persistent_entity, pos, &level_weak);
                pending_entities.append(&mut loaded_entities);
            }

            // Restore POI ticket state (populate_poi ran in from_disk, now apply saved occupancy)
            if !persistent.pois.is_empty()
                && let Some(world) = level.upgrade()
            {
                let tickets: Vec<_> = persistent
                    .pois
                    .iter()
                    .map(|p| {
                        let block_pos = BlockPos::new(
                            pos.0.x * 16 + i32::from(p.x),
                            i32::from(p.y),
                            pos.0.y * 16 + i32::from(p.z),
                        );
                        (block_pos, p.free_tickets)
                    })
                    .collect();
                world.poi_storage.lock().restore_tickets(pos, &tickets);
            }

            // Clear dirty flag since we just loaded (add_and_register marks dirty)
            chunk.dirty.store(false, Ordering::Release);

            LoadedChunk {
                chunk: ChunkAccess::Full(chunk),
                status,
                pending_entities,
            }
        } else {
            let block_ticks = BlockTickList::from_proto_saved_ticks(
                Self::persistent_to_block_saved_ticks(&persistent.block_ticks, pos),
            );
            let fluid_ticks = FluidTickList::from_proto_saved_ticks(
                Self::persistent_to_fluid_saved_ticks(&persistent.fluid_ticks, pos),
            );
            let carving_mask = persistent
                .carving_mask
                .as_deref()
                .map(|packed| CarvingMask::from_packed_u64s(height, min_y, packed));

            let chunk = ProtoChunk::from_disk(
                Sections::from_owned(sections.into_boxed_slice()),
                pos,
                status,
                min_y,
                height,
                structure_starts,
                structure_references,
                carving_mask,
                persistent.postprocessing.iter().map(Vec::clone).collect(),
                block_ticks,
                fluid_ticks,
                level.clone(),
                light,
            );

            for persistent_be in &persistent.block_entities {
                let block_entity_pos = Self::persistent_block_entity_pos(persistent_be, pos);
                if persistent_be.entity_type.is_none() {
                    chunk.set_pending_block_entity(block_entity_pos);
                    continue;
                }
                let state = chunk.get_block_state(block_entity_pos);
                if let Some(block_entity) = Self::persistent_to_block_entity_at(
                    persistent_be,
                    block_entity_pos,
                    level.clone(),
                    state,
                ) {
                    let _ = chunk.set_block_entity(block_entity);
                }
            }

            for persistent_entity in &persistent.entities {
                let loaded_entities =
                    Self::persistent_to_entity_tree_at_level(persistent_entity, pos, &level);
                for entity in loaded_entities {
                    chunk.add_entity(entity);
                }
            }

            chunk.dirty.store(false, Ordering::Release);

            LoadedChunk {
                chunk: ChunkAccess::Proto(chunk),
                status,
                pending_entities: Vec::new(),
            }
        }
    }

    /// Converts chunk heightmaps to persistent format for saving.
    pub(super) fn heightmaps_to_persistent(
        heightmaps: &ChunkHeightmaps,
    ) -> Vec<PersistentHeightmap> {
        HeightmapType::final_types()
            .iter()
            .enumerate()
            .map(|(i, &hm_type)| {
                let hm = heightmaps.get(hm_type);
                PersistentHeightmap {
                    heightmap_type: i as u8,
                    data: hm.raw_data().to_vec(),
                }
            })
            .collect()
    }

    /// Reconstructs chunk heightmaps from persistent data.
    pub(super) fn persistent_to_heightmaps(
        persistent: &[PersistentHeightmap],
        min_y: i32,
        height: i32,
    ) -> ChunkHeightmaps {
        let final_types = HeightmapType::final_types();
        let mut heightmaps = ChunkHeightmaps::new(min_y, height);

        for ph in persistent {
            let Some(&hm_type) = final_types.get(ph.heightmap_type as usize) else {
                continue;
            };
            if ph.data.len() != 256 {
                tracing::warn!(
                    "Heightmap data length mismatch: expected 256, got {}. Skipping.",
                    ph.data.len()
                );
                continue;
            }
            let mut data = Box::new([0u16; 256]);
            data.copy_from_slice(&ph.data);
            *heightmaps.get_mut(hm_type) = Heightmap::from_raw_data(hm_type, min_y, height, data);
        }

        heightmaps
    }

    /// Collects POI occupancy data from the world's POI storage for this chunk.
    pub(super) fn pois_to_persistent(
        chunk: &LevelChunk,
        chunk_pos: ChunkPos,
    ) -> Vec<PersistentPoi> {
        let Some(world) = chunk.get_level() else {
            return Vec::new();
        };
        world
            .poi_storage
            .lock()
            .collect_for_chunk(chunk_pos)
            .into_iter()
            .map(|(pos, free_tickets)| PersistentPoi {
                x: (pos.0.x - chunk_pos.0.x * 16) as u8,
                y: pos.0.y as i16,
                z: (pos.0.z - chunk_pos.0.y * 16) as u8,
                free_tickets,
            })
            .collect()
    }

    /// Converts a persistent section to runtime format.
    pub(super) fn persistent_to_section(
        persistent: &PersistentSection,
        chunk: &PersistentChunk<'_>,
    ) -> ChunkSection {
        match persistent {
            PersistentSection::Homogeneous {
                block_state,
                biomes,
            } => {
                let block_id = Self::resolve_block_state(chunk, *block_state);
                let biome_data = Self::persistent_to_biomes(biomes, chunk);
                ChunkSection::new_with_biomes(PalettedContainer::Homogeneous(block_id), biome_data)
            }
            PersistentSection::Heterogeneous {
                palette,
                bits_per_entry,
                block_data,
                biomes,
            } => {
                let mut indices = unpack_indices(block_data, *bits_per_entry);
                let runtime_palette: Vec<BlockStateId> = palette
                    .iter()
                    .map(|&idx| Self::resolve_block_state(chunk, idx))
                    .collect();
                let mut cube = Box::new([[[BlockStateId(0); 16]; 16]; 16]);
                for plane in &mut cube {
                    for row in plane {
                        for cell in row {
                            *cell = runtime_palette[indices.next().expect(
                                "this should never fail, we know the iterator is long enough",
                            ) as usize];
                        }
                    }
                }
                let states = PalettedContainer::from_cube(cube);
                let biome_data = Self::persistent_to_biomes(biomes, chunk);
                ChunkSection::new_with_biomes(states, biome_data)
            }
        }
    }

    /// Converts persistent biome data to runtime format.
    pub(super) fn persistent_to_biomes(
        persistent: &PersistentBiomeData,
        chunk: &PersistentChunk<'_>,
    ) -> PalettedContainer<u16, 4> {
        match persistent {
            PersistentBiomeData::Homogeneous { biome } => {
                let biome_id = Self::resolve_biome(chunk, *biome);
                PalettedContainer::Homogeneous(biome_id)
            }
            PersistentBiomeData::Heterogeneous {
                palette,
                bits_per_entry,
                biome_data,
            } => {
                let mut indices = unpack_indices(biome_data, *bits_per_entry);
                let runtime_palette: Vec<u16> = palette
                    .iter()
                    .map(|&idx| Self::resolve_biome(chunk, idx))
                    .collect();
                let mut cube = [[[0u16; 4]; 4]; 4];
                for plane in &mut cube {
                    for row in plane {
                        for cell in row {
                            *cell = runtime_palette[indices.next().expect(
                                "this should never fail, we know the iterator is long enough",
                            ) as usize];
                        }
                    }
                }
                PalettedContainer::from_cube(Box::new(cube))
            }
        }
    }

    /// Resolves a chunk palette index to a runtime `BlockStateId`.
    pub(super) fn resolve_block_state(chunk: &PersistentChunk<'_>, index: u16) -> BlockStateId {
        if let Some(state) = chunk.block_states.get(index as usize)
            && let Some(state_id) = REGISTRY
                .blocks
                .state_id_from_properties(&state.name, &state.properties)
        {
            return state_id;
        }
        BlockStateId(0) // Air fallback
    }

    /// Resolves a chunk palette index to a runtime biome ID.
    pub(super) fn resolve_biome(chunk: &PersistentChunk<'_>, index: u16) -> u16 {
        if let Some(biome_key) = chunk.biomes.get(index as usize)
            && let Some(id) = REGISTRY.biomes.id_from_key(biome_key)
        {
            return id as u16;
        }
        vanilla_biomes::PLAINS.id() as u16
    }
}
