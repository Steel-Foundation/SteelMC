use super::*;

pub(super) fn portal_candidate_distance_sqr(candidate: BlockPos, center: BlockPos) -> i64 {
    let dx = i64::from(candidate.x()) - i64::from(center.x());
    let dy = i64::from(candidate.y()) - i64::from(center.y());
    let dz = i64::from(candidate.z()) - i64::from(center.z());
    dx * dx + dy * dy + dz * dz
}

pub(super) fn dist_to_origin_center_sqr(pos: BlockPos) -> f64 {
    let x = f64::from(pos.x()) + 0.5;
    let y = f64::from(pos.y()) + 0.5;
    let z = f64::from(pos.z()) + 0.5;
    x * x + y * y + z * z
}

pub(super) fn closest_portal_candidate(
    candidates: impl IntoIterator<Item = BlockPos>,
    approximate_exit_pos: BlockPos,
    is_valid: impl Fn(BlockPos) -> bool,
) -> Option<BlockPos> {
    candidates
        .into_iter()
        .filter(|pos| is_valid(*pos))
        .min_by_key(|pos| {
            (
                portal_candidate_distance_sqr(*pos, approximate_exit_pos),
                pos.y(),
            )
        })
}

const NETHER_PORTAL_CREATE_RADIUS: i32 = 16;
const NETHER_PORTAL_FALLBACK_MIN_Y: i32 = 70;
const NETHER_PORTAL_FALLBACK_MAX_Y_OFFSET: i32 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MissingPortalCreationChunk;

pub(super) const fn nether_portal_frame_offset_pos(
    origin: BlockPos,
    direction: Direction,
    width: i32,
    height: i32,
    offset: i32,
) -> BlockPos {
    let clockwise = direction.rotate_y_clockwise();
    let (direction_x, _, direction_z) = direction.offset();
    let (clockwise_x, _, clockwise_z) = clockwise.offset();
    origin.offset(
        direction_x * width + clockwise_x * offset,
        height,
        direction_z * width + clockwise_z * offset,
    )
}

pub(super) fn nether_portal_creation_scan_origin(
    column_pos: BlockPos,
    direction: Direction,
    height: i32,
) -> BlockPos {
    column_pos.relative(direction.opposite()).at_y(height)
}

impl World {
    /// Finds the closest existing Nether portal POI using vanilla `PortalForcer` ordering.
    ///
    /// `to_nether` selects vanilla's 16-block Nether search radius; non-Nether targets use 128.
    ///
    /// # Panics
    ///
    /// Panics if vanilla POI registries were not initialized before portal lookup.
    #[must_use]
    pub fn find_closest_nether_portal_position(
        &self,
        approximate_exit_pos: BlockPos,
        to_nether: bool,
    ) -> Option<BlockPos> {
        let radius = if to_nether { 16 } else { 128 };
        let nether_portal_type = vanilla_poi_types::NETHER_PORTAL
            .try_id()
            .expect("vanilla nether portal POI type should be registered");
        let candidates = self.poi_storage.lock().get_in_horizontal_square(
            &|type_id| type_id == nether_portal_type,
            approximate_exit_pos,
            radius,
            OccupationStatus::Any,
        );

        closest_portal_candidate(
            candidates.into_iter().map(|(pos, _)| pos),
            approximate_exit_pos,
            |pos| {
                self.is_block_within_world_border(pos)
                    && self
                        .get_block_state(pos)
                        .try_get_value(&BlockStateProperties::HORIZONTAL_AXIS)
                        .is_some()
            },
        )
    }

    /// Creates a Nether portal using vanilla `PortalForcer.createPortal` placement rules.
    ///
    /// The caller must keep the target search area loaded as full chunks before calling. Steel
    /// returns `None` if any required chunk read or write is unavailable, rather than treating
    /// unloaded chunks as replaceable air.
    #[must_use]
    pub fn create_nether_portal(
        self: &Arc<Self>,
        origin: BlockPos,
        portal_axis: Axis,
    ) -> Option<FoundRectangle> {
        if portal_axis == Axis::Y {
            return None;
        }

        let direction = Direction::positive_for_axis(portal_axis);
        let max_placeable_y = self
            .get_max_y()
            .min(self.get_min_y() + self.dimension_type.logical_height - 1);

        let portal_origin =
            match self.find_nether_portal_creation_position(origin, direction, max_placeable_y) {
                Ok(Some(pos)) => pos,
                Ok(None) => {
                    let fallback =
                        self.fallback_nether_portal_position(origin, direction, max_placeable_y)?;
                    if !self.can_write_nether_portal_fallback_box(fallback, direction) {
                        return None;
                    }
                    if !self.clear_nether_portal_fallback_box(fallback, direction) {
                        return None;
                    }
                    fallback
                }
                Err(MissingPortalCreationChunk) => return None,
            };

        if !self.can_write_nether_portal_rectangle(portal_origin, direction) {
            return None;
        }
        if !self.place_nether_portal_frame_and_blocks(portal_origin, direction, portal_axis) {
            return None;
        }

        Some(FoundRectangle {
            min_corner: portal_origin,
            axis1_size: 2,
            axis2_size: 3,
        })
    }

    /// Adds or refreshes vanilla's portal chunk ticket for a post-teleport entity.
    pub(crate) fn place_portal_ticket(&self, ticket_position: BlockPos) {
        self.chunk_map.place_portal_ticket(ticket_position);
    }

    pub(super) fn find_nether_portal_creation_position(
        &self,
        origin: BlockPos,
        direction: Direction,
        max_placeable_y: i32,
    ) -> Result<Option<BlockPos>, MissingPortalCreationChunk> {
        let mut closest_full_position: Option<(i64, BlockPos)> = None;
        let mut closest_partial_position: Option<(i64, BlockPos)> = None;
        let border = self.world_border_snapshot();

        for column_pos in BlockPos::spiral_around(
            origin,
            NETHER_PORTAL_CREATE_RADIUS,
            Direction::East,
            Direction::South,
        ) {
            let height = self
                .height_at(
                    HeightmapType::MotionBlocking,
                    column_pos.x(),
                    column_pos.z(),
                )
                .ok_or(MissingPortalCreationChunk)?
                .min(max_placeable_y);
            if !border.is_block_within_bounds(column_pos)
                || !border.is_block_within_bounds(column_pos.relative(direction))
            {
                continue;
            }

            let mut column_pos = nether_portal_creation_scan_origin(column_pos, direction, height);
            let mut y = height;
            while y >= self.get_min_y() {
                column_pos = column_pos.at_y(y);
                if self.can_nether_portal_replace_block(column_pos)? {
                    let first_empty_y = y;

                    while y > self.get_min_y()
                        && self.can_nether_portal_replace_block(column_pos.below())?
                    {
                        y -= 1;
                        column_pos = column_pos.below();
                    }

                    if y + 4 <= max_placeable_y {
                        let delta_y = first_empty_y - y;
                        if (delta_y <= 0 || delta_y >= 3)
                            && self.can_host_nether_portal_frame(column_pos, direction, 0)?
                        {
                            let distance = portal_candidate_distance_sqr(column_pos, origin);
                            let full_frame = self
                                .can_host_nether_portal_frame(column_pos, direction, -1)?
                                && self.can_host_nether_portal_frame(column_pos, direction, 1)?;

                            if full_frame
                                && closest_full_position
                                    .is_none_or(|(closest_distance, _)| closest_distance > distance)
                            {
                                closest_full_position = Some((distance, column_pos));
                            }

                            if closest_full_position.is_none()
                                && closest_partial_position
                                    .is_none_or(|(closest_distance, _)| closest_distance > distance)
                            {
                                closest_partial_position = Some((distance, column_pos));
                            }
                        }
                    }
                }

                y -= 1;
            }
        }

        if closest_full_position.is_none() {
            closest_full_position = closest_partial_position;
        }

        Ok(closest_full_position.map(|(_, pos)| pos))
    }

    pub(super) fn can_nether_portal_replace_block(
        &self,
        pos: BlockPos,
    ) -> Result<bool, MissingPortalCreationChunk> {
        let state = self
            .loaded_block_state(pos)
            .ok_or(MissingPortalCreationChunk)?;
        Ok(state.is_replaceable() && state.get_fluid_state().is_empty())
    }

    pub(super) fn can_host_nether_portal_frame(
        &self,
        origin: BlockPos,
        direction: Direction,
        offset: i32,
    ) -> Result<bool, MissingPortalCreationChunk> {
        for width in -1..3 {
            for height in -1..4 {
                let pos = nether_portal_frame_offset_pos(origin, direction, width, height, offset);
                if height < 0 {
                    let state = self
                        .loaded_block_state(pos)
                        .ok_or(MissingPortalCreationChunk)?;
                    if !state.is_solid() {
                        return Ok(false);
                    }
                } else if !self.can_nether_portal_replace_block(pos)? {
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }

    pub(super) fn loaded_block_state(&self, pos: BlockPos) -> Option<BlockStateId> {
        if !self.is_in_valid_bounds(pos) {
            return Some(REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR));
        }

        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map
            .with_full_chunk(chunk_pos, |chunk| chunk.get_block_state(pos))
    }

    pub(super) fn fallback_nether_portal_position(
        &self,
        origin: BlockPos,
        direction: Direction,
        max_placeable_y: i32,
    ) -> Option<BlockPos> {
        let min_start_y = (self.get_min_y() + 1).max(NETHER_PORTAL_FALLBACK_MIN_Y);
        let max_start_y = max_placeable_y - NETHER_PORTAL_FALLBACK_MAX_Y_OFFSET;
        if max_start_y < min_start_y {
            return None;
        }

        let (direction_x, _, direction_z) = direction.offset();
        let pos = BlockPos::new(
            origin.x() - direction_x,
            origin.y().clamp(min_start_y, max_start_y),
            origin.z() - direction_z,
        );

        Some(self.world_border_snapshot().clamp_to_bounds(
            f64::from(pos.x()),
            f64::from(pos.y()),
            f64::from(pos.z()),
        ))
    }

    pub(super) fn can_write_nether_portal_fallback_box(
        &self,
        origin: BlockPos,
        direction: Direction,
    ) -> bool {
        for box_offset in -1..2 {
            for width in 0..2 {
                for height in -1..3 {
                    let pos = nether_portal_frame_offset_pos(
                        origin, direction, width, height, box_offset,
                    );
                    if !self.can_write_loaded_block(pos) {
                        return false;
                    }
                }
            }
        }

        self.can_write_nether_portal_rectangle(origin, direction)
    }

    pub(super) fn can_write_nether_portal_rectangle(
        &self,
        origin: BlockPos,
        direction: Direction,
    ) -> bool {
        for width in -1..3 {
            for height in -1..4 {
                let pos = nether_portal_frame_offset_pos(origin, direction, width, height, 0);
                if !self.can_write_loaded_block(pos) {
                    return false;
                }
            }
        }

        true
    }

    pub(super) fn can_write_loaded_block(&self, pos: BlockPos) -> bool {
        if !self.is_in_valid_bounds(pos) {
            return false;
        }

        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map.with_full_chunk(chunk_pos, |_| ()).is_some()
    }

    /// Mirrors vanilla `EndPlatformFeature.createEndPlatform` for runtime End portal travel.
    pub(crate) fn create_end_platform(self: &Arc<Self>, origin: BlockPos) -> bool {
        let obsidian = vanilla_blocks::OBSIDIAN.default_state();
        let air = vanilla_blocks::AIR.default_state();

        for dz in -2..=2 {
            for dx in -2..=2 {
                for dy in -1..3 {
                    let pos = origin.offset(dx, dy, dz);
                    let state = if dy == -1 { obsidian } else { air };
                    if self.get_block_state(pos).get_block() != state.get_block() {
                        let _ = self.destroy_block(pos, true);
                        if !self.set_block(pos, state, UpdateFlags::UPDATE_ALL) {
                            return false;
                        }
                    }
                }
            }
        }

        true
    }

    /// Mirrors vanilla `TheEndGatewayBlockEntity.isChunkEmpty`.
    pub(crate) fn is_end_gateway_chunk_empty(&self, chunk_pos: ChunkPos) -> Option<bool> {
        self.chunk_map.with_full_chunk(chunk_pos, |chunk| {
            chunk
                .as_full()
                .is_some_and(|chunk| chunk.highest_filled_section_index().is_none())
        })
    }

    /// Mirrors vanilla `TheEndGatewayBlockEntity.findValidSpawnInChunk`.
    pub(crate) fn find_end_gateway_valid_spawn_in_chunk(
        &self,
        chunk_pos: ChunkPos,
    ) -> Option<BlockPos> {
        self.chunk_map
            .with_full_chunk(chunk_pos, |chunk| {
                let chunk = chunk.as_full()?;
                let min_x = chunk_pos.0.x * 16;
                let min_z = chunk_pos.0.y * 16;
                let max_x = min_x + 15;
                let max_z = min_z + 15;
                let max_y = chunk.highest_section_position() + 16 - 1;
                let min_y = 30.min(max_y);
                let max_y = 30.max(max_y);
                let mut closest = None;
                let mut closest_dist = 0.0;

                for z in min_z..=max_z {
                    for y in min_y..=max_y {
                        for x in min_x..=max_x {
                            let pos = BlockPos::new(x, y, z);
                            let state = chunk.get_block_state(pos);
                            let above = pos.above();
                            let above_two = pos.above_n(2);
                            if state.get_block() != &vanilla_blocks::END_STONE
                                || self.is_collision_shape_full_block_at(
                                    above,
                                    chunk.get_block_state(above),
                                )
                                || self.is_collision_shape_full_block_at(
                                    above_two,
                                    chunk.get_block_state(above_two),
                                )
                            {
                                continue;
                            }

                            let dist = dist_to_origin_center_sqr(pos);
                            if closest.is_none() || dist < closest_dist {
                                closest = Some(pos);
                                closest_dist = dist;
                            }
                        }
                    }
                }

                closest
            })
            .flatten()
    }

    /// Mirrors vanilla `TheEndGatewayBlockEntity.findTallestBlock`.
    pub(crate) fn find_end_gateway_tallest_block(
        &self,
        around: BlockPos,
        dist: i32,
        allow_bedrock: bool,
    ) -> BlockPos {
        let mut tallest = None;

        for dx in -dist..=dist {
            for dz in -dist..=dist {
                if dx == 0 && dz == 0 && !allow_bedrock {
                    continue;
                }

                let min_y = tallest.map_or(self.get_min_y(), |pos: BlockPos| pos.y());
                for y in (min_y + 1..=self.get_max_y()).rev() {
                    let pos = BlockPos::new(around.x() + dx, y, around.z() + dz);
                    let state = self.get_block_state(pos);
                    if self.is_collision_shape_full_block_at(pos, state)
                        && (allow_bedrock || state.get_block() != &vanilla_blocks::BEDROCK)
                    {
                        tallest = Some(pos);
                        break;
                    }
                }
            }
        }

        tallest.unwrap_or(around)
    }

    pub(super) fn is_collision_shape_full_block_at(
        &self,
        pos: BlockPos,
        state: BlockStateId,
    ) -> bool {
        is_shape_full_block(self.block_collision_shape(pos, state))
    }

    /// Mirrors vanilla `EndIslandFeature.place` for runtime End gateway island creation.
    pub(crate) fn create_end_island(self: &Arc<Self>, origin: BlockPos) -> bool {
        let end_stone = vanilla_blocks::END_STONE.default_state();
        let mut random = LegacyRandom::from_seed(PackedBlockPos::from(origin).as_raw() as u64);
        let mut size = random.next_i32_bounded(3) as f32 + 4.0;
        let mut y = 0;

        while size > 0.5 {
            let min = (-size).floor() as i32;
            let max = size.ceil() as i32;
            for x in min..=max {
                for z in min..=max {
                    if (x * x + z * z) as f32 <= (size + 1.0) * (size + 1.0)
                        && !self.set_block(
                            origin.offset(x, y, z),
                            end_stone,
                            UpdateFlags::UPDATE_CLIENTS,
                        )
                    {
                        return false;
                    }
                }
            }

            size -= random.next_i32_bounded(2) as f32 + 0.5;
            y -= 1;
        }

        true
    }

    /// Mirrors vanilla `EndGatewayFeature.place` for runtime End gateway creation.
    pub(crate) fn create_end_gateway_portal(
        self: &Arc<Self>,
        origin: BlockPos,
        exit: BlockPos,
        exact: bool,
    ) -> bool {
        for dy in -2_i32..=2 {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    let same_x = dx == 0;
                    let same_y = dy == 0;
                    let same_z = dz == 0;
                    let end = dy.abs() == 2;
                    let state = if same_x && same_y && same_z {
                        vanilla_blocks::END_GATEWAY.default_state()
                    } else if same_y {
                        vanilla_blocks::AIR.default_state()
                    } else if (end && same_x && same_z) || ((same_x || same_z) && !end) {
                        vanilla_blocks::BEDROCK.default_state()
                    } else {
                        vanilla_blocks::AIR.default_state()
                    };

                    if !self.set_block(origin.offset(dx, dy, dz), state, UpdateFlags::UPDATE_ALL) {
                        return false;
                    }
                }
            }
        }

        let Some(block_entity) = self.get_block_entity(origin) else {
            return false;
        };
        let Some(gateway) = block_entity.downcast_ref::<EndGatewayBlockEntity>() else {
            return false;
        };
        gateway.set_exit_position(exit, exact);
        true
    }

    pub(super) fn clear_nether_portal_fallback_box(
        self: &Arc<Self>,
        origin: BlockPos,
        direction: Direction,
    ) -> bool {
        let obsidian = vanilla_blocks::OBSIDIAN.default_state();
        let air = vanilla_blocks::AIR.default_state();

        for box_offset in -1..2 {
            for width in 0..2 {
                for height in -1..3 {
                    let state = if height < 0 { obsidian } else { air };
                    let pos = nether_portal_frame_offset_pos(
                        origin, direction, width, height, box_offset,
                    );
                    if !self.set_block(pos, state, UpdateFlags::UPDATE_ALL) {
                        return false;
                    }
                }
            }
        }

        true
    }

    pub(super) fn place_nether_portal_frame_and_blocks(
        self: &Arc<Self>,
        origin: BlockPos,
        direction: Direction,
        portal_axis: Axis,
    ) -> bool {
        let obsidian = vanilla_blocks::OBSIDIAN.default_state();
        for width in -1..3 {
            for height in -1..4 {
                if width == -1 || width == 2 || height == -1 || height == 3 {
                    let pos = nether_portal_frame_offset_pos(origin, direction, width, height, 0);
                    if !self.set_block(pos, obsidian, UpdateFlags::UPDATE_ALL) {
                        return false;
                    }
                }
            }
        }

        let portal_state = vanilla_blocks::NETHER_PORTAL
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_AXIS, portal_axis);
        let portal_flags = UpdateFlags::UPDATE_CLIENTS | UpdateFlags::UPDATE_KNOWN_SHAPE;
        for width in 0..2 {
            for height in 0..3 {
                let pos = nether_portal_frame_offset_pos(origin, direction, width, height, 0);
                if !self.set_block(pos, portal_state, portal_flags) {
                    return false;
                }
            }
        }

        true
    }
}
