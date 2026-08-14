use super::{
    Arc, BlockPos, BlockStateExt, ChunkGenerator, ChunkPos, Direction, HeightmapType, SectionPos,
    World, is_offset_face_full, vanilla_dimension_types,
};

const fn chunk_min_block_x(pos: ChunkPos) -> i32 {
    pos.0.x << 4
}

const fn chunk_min_block_z(pos: ChunkPos) -> i32 {
    pos.0.y << 4
}

const fn chunk_max_block_x(pos: ChunkPos) -> i32 {
    (pos.0.x << 4) + 15
}

const fn chunk_max_block_z(pos: ChunkPos) -> i32 {
    (pos.0.y << 4) + 15
}

impl World {
    /// Initializes this world's default spawn using vanilla's first-world spawn search.
    pub async fn initialize_spawn_if_needed(self: &Arc<Self>) -> Result<(), String> {
        if self.level_data.read().data().initialized {
            return Ok(());
        }

        if self.dimension_type.key != vanilla_dimension_types::OVERWORLD.key {
            self.level_data.write().data_mut().initialized = true;
            return Ok(());
        }

        log::info!("Selecting global world spawn for {}...", self.key);

        let origin = self
            .chunk_map
            .world_gen_context
            .generator
            .initial_spawn_search_origin();
        let spawn_chunk = ChunkPos::new(
            SectionPos::block_to_section_coord(origin.x()),
            SectionPos::block_to_section_coord(origin.z()),
        );

        let mut spawn_y = self
            .chunk_map
            .world_gen_context
            .generator
            .spawn_height(self.get_min_y(), self.get_height());
        if spawn_y < self.get_min_y() {
            let x = chunk_min_block_x(spawn_chunk) + 8;
            let z = chunk_min_block_z(spawn_chunk) + 8;
            spawn_y = self
                .height_at(HeightmapType::WorldSurface, x, z)
                .unwrap_or(self.get_min_y());
        }

        let mut spawn_pos = BlockPos::new(
            chunk_min_block_x(spawn_chunk) + 8,
            spawn_y,
            chunk_min_block_z(spawn_chunk) + 8,
        );

        spawn_pos = self
            .chunk_map
            .with_full_chunks_in_radius(spawn_chunk, 5, || {
                self.find_spawn_in_loaded_radius(spawn_chunk)
                    .unwrap_or(spawn_pos)
            })
            .await
            .unwrap_or(spawn_pos);

        {
            let mut level_data = self.level_data.write();
            let data = level_data.data_mut();
            data.set_spawn_pos(spawn_pos);
            data.spawn.angle = 0.0;
            data.initialized = true;
        }

        log::info!("World {} spawn initialized at {spawn_pos:?}", self.key);
        Ok(())
    }

    #[expect(
        clippy::similar_names,
        reason = "dx_chunk/dz_chunk mirror vanilla's dXChunk/dZChunk"
    )]
    pub(super) fn find_spawn_in_loaded_radius(&self, spawn_chunk: ChunkPos) -> Option<BlockPos> {
        let mut x_chunk_offset = 0;
        let mut z_chunk_offset = 0;
        let mut dx_chunk = 0;
        let mut dz_chunk = -1;

        for _ in 0..(11 * 11) {
            if (-5..=5).contains(&x_chunk_offset) && (-5..=5).contains(&z_chunk_offset) {
                let candidate_chunk = ChunkPos::new(
                    spawn_chunk.0.x + x_chunk_offset,
                    spawn_chunk.0.y + z_chunk_offset,
                );
                if let Some(candidate) = self.spawn_pos_in_chunk(candidate_chunk) {
                    return Some(candidate);
                }
            }

            if x_chunk_offset == z_chunk_offset
                || (x_chunk_offset < 0 && x_chunk_offset == -z_chunk_offset)
                || (x_chunk_offset > 0 && x_chunk_offset == 1 - z_chunk_offset)
            {
                let old_dx = dx_chunk;
                dx_chunk = -dz_chunk;
                dz_chunk = old_dx;
            }

            x_chunk_offset += dx_chunk;
            z_chunk_offset += dz_chunk;
        }

        None
    }

    pub(super) fn spawn_pos_in_chunk(&self, chunk_pos: ChunkPos) -> Option<BlockPos> {
        for x in chunk_min_block_x(chunk_pos)..=chunk_max_block_x(chunk_pos) {
            for z in chunk_min_block_z(chunk_pos)..=chunk_max_block_z(chunk_pos) {
                if let Some(pos) = self.level_respawn_pos(x, z) {
                    return Some(pos);
                }
            }
        }

        None
    }

    pub(super) fn level_respawn_pos(&self, x: i32, z: i32) -> Option<BlockPos> {
        let top_y = if self.dimension_type.has_ceiling {
            self.chunk_map
                .world_gen_context
                .generator
                .spawn_height(self.get_min_y(), self.get_height())
        } else {
            self.vanilla_chunk_height_at(HeightmapType::MotionBlocking, x, z)?
        };

        if top_y < self.get_min_y() {
            return None;
        }

        let surface = self.vanilla_chunk_height_at(HeightmapType::WorldSurface, x, z)?;
        let ocean_floor = self.vanilla_chunk_height_at(HeightmapType::OceanFloor, x, z)?;
        if surface <= top_y && surface > ocean_floor {
            return None;
        }

        for y in (self.get_min_y()..=top_y + 1).rev() {
            let pos = BlockPos::new(x, y, z);
            let state = self.get_block_state(pos);
            if state.has_fluid() {
                break;
            }

            if is_offset_face_full(state.get_collision_shape_at(pos), Direction::Up) {
                return Some(BlockPos::new(x, y + 1, z));
            }
        }

        None
    }

    pub(crate) fn height_at(&self, heightmap_type: HeightmapType, x: i32, z: i32) -> Option<i32> {
        let chunk_pos = ChunkPos::new(
            SectionPos::block_to_section_coord(x),
            SectionPos::block_to_section_coord(z),
        );
        self.chunk_map.with_full_chunk(chunk_pos, |chunk| {
            chunk.get_height(heightmap_type, (x & 15) as usize, (z & 15) as usize)
        })
    }

    pub(super) fn vanilla_chunk_height_at(
        &self,
        heightmap_type: HeightmapType,
        x: i32,
        z: i32,
    ) -> Option<i32> {
        self.height_at(heightmap_type, x, z)
            .map(|first_available| first_available - 1)
    }

    pub(super) fn heightmap_pos(&self, heightmap_type: HeightmapType, pos: BlockPos) -> BlockPos {
        BlockPos::new(
            pos.x(),
            self.level_height_at(heightmap_type, pos.x(), pos.z()),
            pos.z(),
        )
    }

    /// Mirrors vanilla `Entity.adjustSpawnLocation` for cross-world returns.
    #[must_use]
    pub(crate) fn adjust_spawn_location(&self, spawn_suggestion: BlockPos) -> BlockPos {
        self.heightmap_pos(HeightmapType::MotionBlockingNoLeaves, spawn_suggestion)
    }

    pub(super) fn level_height_at(&self, heightmap_type: HeightmapType, x: i32, z: i32) -> i32 {
        if !Self::is_in_world_bounds_horizontal(BlockPos::new(x, 0, z)) {
            return self.sea_level + 1;
        }

        self.height_at(heightmap_type, x, z)
            .unwrap_or_else(|| self.get_min_y())
    }
}
