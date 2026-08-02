use super::{
    BlockPos, BlockStateId, BlockTickList, CarvingMask, Chunk, ChunkBuilder, ChunkHeightmaps,
    ChunkPos, ChunkSection, ChunkStatus, ChunkStorage, DATA_LAYER_SIZE, FluidTickList,
    FullChunkRef, FxHashSet, Heightmap, HeightmapType, LoadedChunk, Ordering, PalettedContainer,
    PersistentBiomeData, PersistentChunk, PersistentHeightmap, PersistentLightSection,
    PersistentPoi, PersistentSection, REGISTRY, RegistryExt, SectionHolder, Sections, Weak, World,
    bits_for_palette_len, io, pack_indices, unpack_indices,
};

impl ChunkStorage {
    fn invalid_chunk_data(message: impl Into<String>) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, message.into())
    }

    fn validate_packed_palette(
        palette: &[u16],
        bits_per_entry: u8,
        data: &[u64],
        entry_count: usize,
        global_palette_len: usize,
        name: &str,
    ) -> io::Result<()> {
        let Some(expected_bits) = bits_for_palette_len(palette.len()) else {
            return Err(Self::invalid_chunk_data(format!(
                "heterogeneous {name} palette has fewer than two entries"
            )));
        };
        if bits_per_entry != expected_bits {
            return Err(Self::invalid_chunk_data(format!(
                "heterogeneous {name} palette uses {bits_per_entry} bits, expected {expected_bits}"
            )));
        }
        let values_per_word = 64 / usize::from(bits_per_entry);
        let expected_words = entry_count.div_ceil(values_per_word);
        if data.len() != expected_words {
            return Err(Self::invalid_chunk_data(format!(
                "heterogeneous {name} data has {} words, expected {expected_words}",
                data.len()
            )));
        }
        if let Some(invalid) = palette
            .iter()
            .find(|&&index| usize::from(index) >= global_palette_len)
        {
            return Err(Self::invalid_chunk_data(format!(
                "{name} palette references missing global entry {invalid}"
            )));
        }
        if let Some(invalid) = unpack_indices(data, bits_per_entry)
            .take(entry_count)
            .find(|&index| index as usize >= palette.len())
        {
            return Err(Self::invalid_chunk_data(format!(
                "{name} data references missing local palette entry {invalid}"
            )));
        }
        Ok(())
    }

    #[expect(
        clippy::too_many_lines,
        reason = "keeps complete chunk payload validation in one ordered pass"
    )]
    fn validate_persistent_chunk(
        persistent: &PersistentChunk<'_>,
        status: ChunkStatus,
        min_y: i32,
        height: i32,
    ) -> io::Result<()> {
        if height <= 0 || height % 16 != 0 || min_y % 16 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "chunk world range must be section-aligned, got min_y={min_y}, height={height}"
                ),
            ));
        }
        let expected_sections = (height / 16) as usize;
        if persistent.sections.len() != expected_sections {
            return Err(Self::invalid_chunk_data(format!(
                "chunk has {} sections, expected {expected_sections}",
                persistent.sections.len()
            )));
        }

        for (section_index, section) in persistent.sections.iter().enumerate() {
            let biomes = match section {
                PersistentSection::Homogeneous {
                    block_state,
                    biomes,
                } => {
                    if usize::from(*block_state) >= persistent.block_states.len() {
                        return Err(Self::invalid_chunk_data(format!(
                            "section {section_index} references missing block-state entry {block_state}"
                        )));
                    }
                    biomes
                }
                PersistentSection::Heterogeneous {
                    palette,
                    bits_per_entry,
                    block_data,
                    biomes,
                } => {
                    Self::validate_packed_palette(
                        palette,
                        *bits_per_entry,
                        block_data,
                        4096,
                        persistent.block_states.len(),
                        "block-state",
                    )?;
                    biomes
                }
            };
            match biomes {
                PersistentBiomeData::Homogeneous { biome } => {
                    if usize::from(*biome) >= persistent.biomes.len() {
                        return Err(Self::invalid_chunk_data(format!(
                            "section {section_index} references missing biome entry {biome}"
                        )));
                    }
                }
                PersistentBiomeData::Heterogeneous {
                    palette,
                    bits_per_entry,
                    biome_data,
                } => Self::validate_packed_palette(
                    palette,
                    *bits_per_entry,
                    biome_data,
                    64,
                    persistent.biomes.len(),
                    "biome",
                )?,
            }
        }

        let mut heightmap_types = FxHashSet::default();
        for heightmap in &persistent.heightmaps {
            let Some(heightmap_type) = HeightmapType::from_persistence_id(heightmap.heightmap_type)
            else {
                return Err(Self::invalid_chunk_data(format!(
                    "unknown heightmap type {}",
                    heightmap.heightmap_type
                )));
            };
            if heightmap.data.len() != 256 {
                return Err(Self::invalid_chunk_data(format!(
                    "{heightmap_type:?} heightmap has {} columns, expected 256",
                    heightmap.data.len()
                )));
            }
            if !heightmap_types.insert(heightmap_type) {
                return Err(Self::invalid_chunk_data(format!(
                    "duplicate {heightmap_type:?} heightmap"
                )));
            }
            if let Some(value) = heightmap
                .data
                .iter()
                .find(|&&value| i32::from(value) > height)
            {
                return Err(Self::invalid_chunk_data(format!(
                    "{heightmap_type:?} heightmap contains out-of-range value {value} for height {height}"
                )));
            }
        }

        let light_section_count = expected_sections + 2;
        for (layer_name, sections) in [
            ("block", persistent.light.block.as_slice()),
            ("sky", persistent.light.sky.as_slice()),
        ] {
            let mut indices = FxHashSet::default();
            for section in sections {
                let index = usize::try_from(section.section_index()).map_err(|_| {
                    Self::invalid_chunk_data(format!(
                        "{layer_name} light section index does not fit this platform"
                    ))
                })?;
                if index >= light_section_count {
                    return Err(Self::invalid_chunk_data(format!(
                        "{layer_name} light section index {index} is outside 0..{light_section_count}"
                    )));
                }
                if !indices.insert(index) {
                    return Err(Self::invalid_chunk_data(format!(
                        "duplicate {layer_name} light section {index}"
                    )));
                }
                match section {
                    PersistentLightSection::Initialized { data, .. }
                    | PersistentLightSection::Internal { data, .. }
                        if data.len() != DATA_LAYER_SIZE =>
                    {
                        return Err(Self::invalid_chunk_data(format!(
                            "{layer_name} light section {index} has {} bytes, expected {DATA_LAYER_SIZE}",
                            data.len()
                        )));
                    }
                    _ => {}
                }
            }
        }

        let max_y = min_y.checked_add(height).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "world height range overflowed")
        })?;
        for (name, x, y, z) in persistent
            .block_entities
            .iter()
            .map(|entry| ("block entity", entry.x, entry.y, entry.z))
            .chain(
                persistent
                    .block_ticks
                    .iter()
                    .map(|entry| ("block tick", entry.x, entry.y, entry.z)),
            )
            .chain(
                persistent
                    .fluid_ticks
                    .iter()
                    .map(|entry| ("fluid tick", entry.x, entry.y, entry.z)),
            )
            .chain(
                persistent
                    .pois
                    .iter()
                    .map(|entry| ("POI", entry.x, entry.y, entry.z)),
            )
        {
            let y = i32::from(y);
            if x >= 16 || z >= 16 || y < min_y || y >= max_y {
                return Err(Self::invalid_chunk_data(format!(
                    "{name} position ({x}, {y}, {z}) is outside the chunk"
                )));
            }
        }

        if status == ChunkStatus::Full && persistent.carving_mask.is_some() {
            return Err(Self::invalid_chunk_data(
                "Full chunk contains a proto carving mask",
            ));
        }
        if let Some(mask) = &persistent.carving_mask {
            let max_words = (256usize * height as usize).div_ceil(64);
            if mask.len() > max_words {
                return Err(Self::invalid_chunk_data(format!(
                    "carving mask has {} words, maximum is {max_words}",
                    mask.len()
                )));
            }
        }
        if persistent.postprocessing.len() > expected_sections {
            return Err(Self::invalid_chunk_data(format!(
                "chunk has {} postprocessing section lists, maximum is {expected_sections}",
                persistent.postprocessing.len()
            )));
        }

        Ok(())
    }

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
    /// * `level` - Weak reference to the world for Full chunk runtime access
    #[expect(
        clippy::too_many_lines,
        reason = "chunk persistence conversion is a linear field-by-field transform"
    )]
    pub(crate) fn try_persistent_to_chunk(
        persistent: &PersistentChunk<'_>,
        pos: ChunkPos,
        status: ChunkStatus,
        min_y: i32,
        height: i32,
        level: Weak<World>,
    ) -> io::Result<LoadedChunk> {
        // Validate every persisted shape that materialization relies on before
        // constructing a Chunk. Full construction populates world POI state, so
        // a late validation failure would otherwise leak partial loaded state.
        Self::validate_persistent_chunk(persistent, status, min_y, height)?;
        let sections: Vec<ChunkSection> = persistent
            .sections
            .iter()
            .map(|section| Self::persistent_to_section(section, persistent))
            .collect::<io::Result<_>>()?;
        let sections = Sections::from_owned(sections.into_boxed_slice());

        // Reconstruct structure data
        let structure_starts = Self::persistent_to_structure_starts(&persistent.structure_starts);
        let structure_references =
            Self::persistent_to_structure_references(&persistent.structure_references);
        let light = Self::persistent_to_light(&persistent.light, min_y, height, status);
        let mut heightmaps =
            Self::persistent_to_heightmaps(&persistent.heightmaps, status, min_y, height);
        heightmaps.prime_from_sections(
            status.heightmaps_after(),
            min_y,
            height,
            &sections.sections,
        );

        if status == ChunkStatus::Full {
            // Reconstruct scheduled ticks from persistent data
            let block_ticks = BlockTickList::from_saved_ticks(
                Self::persistent_to_block_saved_ticks(&persistent.block_ticks, pos),
            );
            let fluid_ticks = FluidTickList::from_saved_ticks(
                Self::persistent_to_fluid_saved_ticks(&persistent.fluid_ticks, pos),
            );

            let chunk = Chunk::from_full_disk(
                sections,
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
            let full = FullChunkRef::from_full_context(&chunk);

            // Load block entities
            for persistent_be in &persistent.block_entities {
                if persistent_be.entity_type.is_none() {
                    let block_entity_pos = Self::persistent_block_entity_pos(persistent_be, pos);
                    full.set_pending_block_entity(block_entity_pos);
                    continue;
                }
                if let Some(block_entity) =
                    Self::persistent_to_block_entity(persistent_be, pos, full)
                {
                    let _ = full.add_and_register_block_entity(block_entity);
                }
            }

            let mut pending_entities = Vec::with_capacity(persistent.entities.len());
            let level_weak = full.level_weak();
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
            full.common().dirty.store(false, Ordering::Release);

            Ok(LoadedChunk {
                chunk,
                status,
                pending_entities,
            })
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

            let chunk = Chunk::from_disk(
                sections,
                pos,
                status,
                min_y,
                height,
                heightmaps,
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

            Ok(LoadedChunk {
                chunk,
                status,
                pending_entities: Vec::new(),
            })
        }
    }

    #[cfg(test)]
    pub(crate) fn persistent_to_chunk(
        persistent: &PersistentChunk<'_>,
        pos: ChunkPos,
        status: ChunkStatus,
        min_y: i32,
        height: i32,
        level: Weak<World>,
    ) -> LoadedChunk {
        Self::try_persistent_to_chunk(persistent, pos, status, min_y, height, level)
            .expect("test persistent chunk should be valid")
    }

    /// Converts chunk heightmaps to persistent format for saving.
    pub(super) fn heightmaps_to_persistent(
        heightmaps: &ChunkHeightmaps,
        status: ChunkStatus,
    ) -> Vec<PersistentHeightmap> {
        status
            .heightmaps_after()
            .iter()
            .filter_map(|&hm_type| {
                let hm = heightmaps.get(hm_type)?;
                Some(PersistentHeightmap {
                    heightmap_type: hm_type.persistence_id(),
                    data: hm.raw_data().to_vec(),
                })
            })
            .collect()
    }

    /// Reconstructs chunk heightmaps from persistent data.
    pub(super) fn persistent_to_heightmaps(
        persistent: &[PersistentHeightmap],
        status: ChunkStatus,
        min_y: i32,
        height: i32,
    ) -> ChunkHeightmaps {
        let mut heightmaps = ChunkHeightmaps::empty();

        for ph in persistent {
            let Some(hm_type) = HeightmapType::from_persistence_id(ph.heightmap_type) else {
                continue;
            };
            if !status.heightmaps_after().contains(&hm_type) {
                continue;
            }
            if ph.data.len() != 256 {
                tracing::warn!(
                    "Heightmap data length mismatch: expected 256, got {}. Skipping.",
                    ph.data.len()
                );
                continue;
            }
            let mut data = Box::new([0u16; 256]);
            data.copy_from_slice(&ph.data);
            heightmaps.replace(Heightmap::from_raw_data(hm_type, min_y, height, data));
        }

        heightmaps
    }

    /// Collects POI occupancy data from the world's POI storage for this chunk.
    pub(super) fn pois_to_persistent(
        chunk: FullChunkRef<'_>,
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
    ) -> io::Result<ChunkSection> {
        match persistent {
            PersistentSection::Homogeneous {
                block_state,
                biomes,
            } => {
                let block_id = Self::resolve_block_state(chunk, *block_state)?;
                let biome_data = Self::persistent_to_biomes(biomes, chunk)?;
                Ok(ChunkSection::new_with_biomes(
                    PalettedContainer::Homogeneous(block_id),
                    biome_data,
                ))
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
                    .collect::<io::Result<_>>()?;
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
                let biome_data = Self::persistent_to_biomes(biomes, chunk)?;
                Ok(ChunkSection::new_with_biomes(states, biome_data))
            }
        }
    }

    /// Converts persistent biome data to runtime format.
    pub(super) fn persistent_to_biomes(
        persistent: &PersistentBiomeData,
        chunk: &PersistentChunk<'_>,
    ) -> io::Result<PalettedContainer<u16, 4>> {
        match persistent {
            PersistentBiomeData::Homogeneous { biome } => {
                let biome_id = Self::resolve_biome(chunk, *biome)?;
                Ok(PalettedContainer::Homogeneous(biome_id))
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
                    .collect::<io::Result<_>>()?;
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
                Ok(PalettedContainer::from_cube(Box::new(cube)))
            }
        }
    }

    /// Resolves a chunk palette index to a runtime `BlockStateId`.
    pub(super) fn resolve_block_state(
        chunk: &PersistentChunk<'_>,
        index: u16,
    ) -> io::Result<BlockStateId> {
        let Some(state) = chunk.block_states.get(index as usize) else {
            return Err(Self::invalid_chunk_data(format!(
                "missing block-state palette entry {index}"
            )));
        };
        let Some(state_id) = REGISTRY
            .blocks
            .state_id_from_properties(&state.name, &state.properties)
        else {
            return Err(Self::invalid_chunk_data(format!(
                "unresolvable block state {} with properties {:?}",
                state.name, state.properties
            )));
        };
        let canonical = REGISTRY.blocks.get_properties(state_id);
        let unique_names = state
            .properties
            .iter()
            .map(|(name, _)| *name)
            .collect::<FxHashSet<_>>();
        if unique_names.len() != state.properties.len()
            || canonical.len() != state.properties.len()
            || canonical
                .iter()
                .any(|property| !state.properties.contains(property))
        {
            return Err(Self::invalid_chunk_data(format!(
                "noncanonical block state {} with properties {:?}",
                state.name, state.properties
            )));
        }
        Ok(state_id)
    }

    /// Resolves a chunk palette index to a runtime biome ID.
    pub(super) fn resolve_biome(chunk: &PersistentChunk<'_>, index: u16) -> io::Result<u16> {
        let Some(biome_key) = chunk.biomes.get(index as usize) else {
            return Err(Self::invalid_chunk_data(format!(
                "missing biome palette entry {index}"
            )));
        };
        let Some(id) = REGISTRY.biomes.id_from_key(biome_key) else {
            return Err(Self::invalid_chunk_data(format!(
                "unknown biome {biome_key}"
            )));
        };
        u16::try_from(id).map_err(|_| {
            Self::invalid_chunk_data(format!("biome {biome_key} id {id} does not fit u16"))
        })
    }
}
