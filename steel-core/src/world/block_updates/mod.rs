use super::*;
use crate::chunk::Chunk;

mod neighbor_updater;

pub(in crate::world) use neighbor_updater::{CollectingNeighborUpdater, ShapeUpdate};

static LARGE_BLOCK_REGION_WARNING_EMITTED: AtomicBool = AtomicBool::new(false);

impl World {
    /// Gets the block state at the given position.
    ///
    /// Returns void air out of bounds and air when the containing chunk is not loaded.
    #[must_use]
    pub fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        if !self.is_in_valid_bounds(pos) {
            return REGISTRY.blocks.get_base_state_id(&vanilla_blocks::VOID_AIR);
        }

        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map
            .with_full_chunk(chunk_pos, |chunk| chunk.get_block_state(pos))
            .unwrap_or_else(|| REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR))
    }

    ///Vanilla equivalent: `level.getBrightness()`
    pub fn light_value_at(&self, layer: LightLayer, pos: BlockPos) -> u8 {
        if layer == LightLayer::Sky && !self.dimension_type.has_skylight {
            return 0;
        }
        if !self.is_in_valid_bounds_horizontal(pos) {
            return self.default_light_value(layer);
        }

        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map
            .with_chunk_at_status(chunk_pos, ChunkStatus::Light, |chunk| {
                let light = chunk.light();
                light.get_light_value(layer, pos)
            })
            .unwrap_or_else(|| self.default_light_value(layer))
    }

    pub(crate) fn is_entity_ticking_chunk_loaded(&self, pos: BlockPos) -> bool {
        self.chunk_map
            .is_entity_ticking_full_chunk_loaded(Self::chunk_pos_for_block(pos))
    }

    pub(crate) fn is_full_chunk_loaded_at(&self, pos: BlockPos) -> bool {
        self.chunk_map
            .with_full_chunk(Self::chunk_pos_for_block(pos), |_| ())
            .is_some()
    }

    pub(crate) fn queue_light_change_after_block_set(
        &self,
        pos: BlockPos,
        old_state: BlockStateId,
        new_state: BlockStateId,
        empty_section_change: Option<LightSectionEmptinessChange>,
    ) {
        let light_properties_changed = has_different_light_properties(old_state, new_state);
        if !light_properties_changed && empty_section_change.is_none() {
            return;
        }

        self.chunk_map
            .queue_light_change(pos, light_properties_changed, empty_section_change);
    }

    pub(super) const fn default_light_value(&self, layer: LightLayer) -> u8 {
        match layer {
            LightLayer::Sky if self.dimension_type.has_skylight => MAX_LIGHT_LEVEL,
            LightLayer::Sky | LightLayer::Block => 0,
        }
    }

    /// Returns whether every block state in the vanilla AABB block range is air.
    ///
    /// Matches `BlockGetter.getBlockStates(AABB)` using
    /// `BlockPos.betweenClosedStream(AABB)`: both min and max coordinates are
    /// floored before iterating the inclusive block range. Large ranges fall back
    /// to streaming reads instead of acquiring an unbounded section workset.
    #[must_use]
    pub fn block_states_in_aabb_are_air(&self, aabb: WorldAabb) -> bool {
        let min_x = aabb.min_x().floor() as i32;
        let min_y = aabb.min_y().floor() as i32;
        let min_z = aabb.min_z().floor() as i32;
        let max_x = aabb.max_x().floor() as i32;
        let max_y = aabb.max_y().floor() as i32;
        let max_z = aabb.max_z().floor() as i32;

        let bounds = BlockRegionBounds::from_corners(
            BlockPos::new(min_x, min_y, min_z),
            BlockPos::new(max_x, max_y, max_z),
        );

        let streaming_read = || {
            for y in min_y..=max_y {
                for z in min_z..=max_z {
                    for x in min_x..=max_x {
                        if !self.get_block_state(BlockPos::new(x, y, z)).is_air() {
                            return false;
                        }
                    }
                }
            }
            true
        };

        let Some(all_air) = self.try_with_block_region(bounds, |region| {
            for y in min_y..=max_y {
                for z in min_z..=max_z {
                    for x in min_x..=max_x {
                        let Some(state) = region.get_block_state(BlockPos::new(x, y, z)) else {
                            return false;
                        };
                        if !state.is_air() {
                            return false;
                        }
                    }
                }
            }
            true
        }) else {
            if !LARGE_BLOCK_REGION_WARNING_EMITTED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    min_x,
                    min_y,
                    min_z,
                    max_x,
                    max_y,
                    max_z,
                    max_workset_slots = MAX_BLOCK_REGION_WORKSET_SLOTS,
                    "Block-state AABB exceeds the bulk-read limit; using streaming reads"
                );
            }
            return streaming_read();
        };
        all_air
    }

    /// Sets a block at the given position.
    ///
    /// Returns `true` if the block was successfully set, `false` otherwise.
    /// Uses the default update limit of 512 (matching vanilla).
    ///
    /// Live gameplay callers must run in Steel's serialized world-mutation phase. Palette and
    /// block-entity ownership claims are atomic, but the following Vanilla-ordered callbacks,
    /// neighbor updates, and derived-cache writes are intentionally not one concurrent
    /// transaction for the same position.
    pub fn set_block(
        self: &Arc<Self>,
        pos: BlockPos,
        block_state: BlockStateId,
        flags: UpdateFlags,
    ) -> bool {
        self.set_block_with_limit(pos, block_state, flags, 512)
    }

    /// Sets a block at the given position with a custom update limit.
    ///
    /// The update limit bounds recursive shape propagation. The block mutation
    /// itself still occurs when the limit is zero or negative, matching vanilla.
    ///
    /// Returns `true` if the block was successfully set, `false` otherwise.
    /// See [`Self::set_block`] for the serialized world-mutation requirement.
    pub fn set_block_with_limit(
        self: &Arc<Self>,
        pos: BlockPos,
        block_state: BlockStateId,
        flags: UpdateFlags,
        update_limit: i32,
    ) -> bool {
        if !self.is_in_valid_bounds(pos) {
            return false;
        }

        let chunk_pos = Self::chunk_pos_for_block(pos);
        let Some(old_state) = self
            .chunk_map
            .with_full_chunk(chunk_pos, |chunk| {
                chunk.set_block_state(pos, block_state, flags)
            })
            .flatten()
        else {
            return false;
        };

        self.finish_block_set(pos, old_state, block_state, flags, update_limit);
        true
    }

    /// Replaces a block only if it still has `expected_state`.
    ///
    /// The comparison and palette write are performed under one chunk-section write lock. This
    /// prevents two consumers of the same observed block state from both succeeding. Block
    /// callbacks still run after that state claim, so callers must remain in a serialized
    /// world-mutation phase such as an exclusive packet handler or ordered tick commit.
    pub fn set_block_if_unchanged(
        self: &Arc<Self>,
        pos: BlockPos,
        expected_state: BlockStateId,
        new_state: BlockStateId,
        flags: UpdateFlags,
    ) -> ConditionalBlockSetResult {
        self.set_block_if_unchanged_with_limit(pos, expected_state, new_state, flags, 512)
    }

    /// Conditional variant of [`Self::set_block_with_limit`].
    pub fn set_block_if_unchanged_with_limit(
        self: &Arc<Self>,
        pos: BlockPos,
        expected_state: BlockStateId,
        new_state: BlockStateId,
        flags: UpdateFlags,
        update_limit: i32,
    ) -> ConditionalBlockSetResult {
        if !self.is_in_valid_bounds(pos) {
            return ConditionalBlockSetResult::Unavailable;
        }

        let chunk_pos = Self::chunk_pos_for_block(pos);
        let Some(result) = self
            .chunk_map
            .with_full_chunk(chunk_pos, |chunk| {
                chunk.set_block_state_if_unchanged(pos, expected_state, new_state, flags)
            })
            .flatten()
        else {
            return ConditionalBlockSetResult::Unavailable;
        };

        match result {
            FullChunkBlockSetResult::Changed(old_state) => {
                self.finish_block_set(pos, old_state, new_state, flags, update_limit);
                ConditionalBlockSetResult::Changed
            }
            FullChunkBlockSetResult::Unchanged => ConditionalBlockSetResult::Unchanged,
            FullChunkBlockSetResult::Stale(current_state) => {
                ConditionalBlockSetResult::Stale(current_state)
            }
        }
    }

    pub(super) fn finish_block_set(
        self: &Arc<Self>,
        pos: BlockPos,
        old_state: BlockStateId,
        block_state: BlockStateId,
        flags: UpdateFlags,
        update_limit: i32,
    ) {
        let new_state = self.get_block_state(pos);
        if new_state != block_state {
            return;
        }

        let chunk_pos = Self::chunk_pos_for_block(pos);
        if flags.contains(UpdateFlags::UPDATE_CLIENTS)
            && self.chunk_map.is_block_ticking_full_chunk_loaded(chunk_pos)
        {
            self.chunk_map.block_changed(pos);
            self.update_navigating_mobs_after_block_collision_change(pos, old_state, block_state);
        }

        if flags.contains(UpdateFlags::UPDATE_NEIGHBORS) {
            self.update_neighbors_at(pos, old_state.get_block());
            let behavior = BLOCK_BEHAVIORS.get_behavior(block_state.get_block());
            if behavior.has_analog_output_signal(block_state) {
                self.update_neighbor_for_output_signal(pos, block_state.get_block());
            }
        }

        if !flags.contains(UpdateFlags::UPDATE_KNOWN_SHAPE) && update_limit > 0 {
            let neighbor_flags =
                flags & !(UpdateFlags::UPDATE_NEIGHBORS | UpdateFlags::UPDATE_SUPPRESS_DROPS);
            let old_behavior = BLOCK_BEHAVIORS.get_behavior(old_state.get_block());
            old_behavior.update_indirect_neighbour_shapes(
                old_state,
                self,
                pos,
                neighbor_flags,
                update_limit - 1,
            );
            self.update_neighbour_shapes(block_state, pos, neighbor_flags, update_limit - 1);
            let new_behavior = BLOCK_BEHAVIORS.get_behavior(block_state.get_block());
            new_behavior.update_indirect_neighbour_shapes(
                block_state,
                self,
                pos,
                neighbor_flags,
                update_limit - 1,
            );
        }

        if REGISTRY.poi_types.type_id_for_state(old_state)
            != REGISTRY.poi_types.type_id_for_state(new_state)
        {
            self.poi_storage
                .lock()
                .on_block_state_change(pos, old_state, new_state);
        }
    }

    pub(super) fn update_navigating_mobs_after_block_collision_change(
        self: &Arc<Self>,
        pos: BlockPos,
        old_state: BlockStateId,
        new_state: BlockStateId,
    ) {
        let collision_shape_changed = self.block_collision_shape_changed(pos, old_state, new_state);
        let game_time = self.game_time();
        for entity_id in self.navigating_mob_ids() {
            let Some(entity) = self.entity_manager.get_by_id(entity_id) else {
                self.untrack_navigating_mob(entity_id);
                continue;
            };
            let Some(pathfinder) = entity.as_pathfinder_mob() else {
                self.untrack_navigating_mob(entity_id);
                continue;
            };
            {
                let mut navigation = pathfinder.mob_base().navigation().lock();
                navigation.invalidate_path_type(pos);
            }
            if !collision_shape_changed {
                continue;
            }
            if !pathfinder.is_path_finding() {
                continue;
            }

            let should_recompute = {
                let navigation = pathfinder.mob_base().navigation().lock();
                navigation.should_recompute_path(pos, pathfinder.position())
            };
            if !should_recompute {
                continue;
            }

            let request = {
                let mut navigation = pathfinder.mob_base().navigation().lock();
                navigation.request_recompute_path(game_time, pathfinder.can_update_path())
            };
            if let Some(request) = request {
                pathfinder.recompute_path(request);
            }
        }
    }

    pub(super) fn navigating_mob_ids(&self) -> Vec<i32> {
        self.navigating_mobs.ids()
    }

    pub(super) fn block_collision_shape_changed(
        &self,
        pos: BlockPos,
        old_state: BlockStateId,
        new_state: BlockStateId,
    ) -> bool {
        let old_shape = self.block_collision_shape(pos, old_state);
        let new_shape = self.block_collision_shape(pos, new_state);
        join_is_not_empty(old_shape, new_shape, BooleanOp::NotSame)
    }

    pub(super) fn block_collision_shape(&self, pos: BlockPos, state: BlockStateId) -> VoxelShape {
        BLOCK_BEHAVIORS
            .get_behavior(state.get_block())
            .get_collision_shape(state, self, pos, BlockCollisionContext::empty())
    }

    /// Updates all neighbors of the given position about a block change.
    ///
    /// This is the Rust equivalent of vanilla's `Level.updateNeighborsAt()`.
    pub fn update_neighbors_at(self: &Arc<Self>, pos: BlockPos, source_block: BlockRef) {
        self.neighbor_updater
            .update_neighbors_at_except_from_facing(self, pos, source_block, None);
    }

    /// Updates all neighbors except the one in `skip_direction`.
    ///
    /// Mirrors vanilla `Level.updateNeighborsAtExceptFromFacing` without the
    /// experimental redstone `Orientation` value.
    pub fn update_neighbors_at_except_from_facing(
        self: &Arc<Self>,
        pos: BlockPos,
        source_block: BlockRef,
        skip_direction: Direction,
    ) {
        self.neighbor_updater
            .update_neighbors_at_except_from_facing(self, pos, source_block, Some(skip_direction));
    }

    /// Updates comparators that can read analog output from `pos`.
    ///
    /// Mirrors vanilla `Level.updateNeighbourForOutputSignal`.
    /// Steel intentionally never synchronously loads the second neighbor chunk:
    /// block-ticking chunks have a radius-one Full-chunk safety border, while
    /// other call sites retain the game-tick no-blocking policy.
    pub(crate) fn update_neighbor_for_output_signal(
        self: &Arc<Self>,
        pos: BlockPos,
        changed_block: BlockRef,
    ) {
        for direction in Direction::HORIZONTAL {
            let mut relative_pos = pos.relative(direction);
            if !self.has_full_chunk(Self::chunk_pos_for_block(relative_pos)) {
                continue;
            }

            let mut state = self.get_block_state(relative_pos);
            if state.get_block() == &vanilla_blocks::COMPARATOR {
                self.neighbor_changed_with_state(state, relative_pos, changed_block, false);
                continue;
            }

            if !self.is_redstone_conductor(state, relative_pos) {
                continue;
            }

            relative_pos = relative_pos.relative(direction);
            if !self.has_full_chunk(Self::chunk_pos_for_block(relative_pos)) {
                continue;
            }
            state = self.get_block_state(relative_pos);
            if state.get_block() == &vanilla_blocks::COMPARATOR {
                self.neighbor_changed_with_state(state, relative_pos, changed_block, false);
            }
        }
    }

    pub(crate) fn update_neighbour_shapes(
        self: &Arc<Self>,
        state: BlockStateId,
        pos: BlockPos,
        flags: UpdateFlags,
        update_limit: i32,
    ) {
        for direction in Direction::UPDATE_SHAPE_ORDER {
            let neighbor_pos = pos.relative(direction);
            self.neighbor_shape_changed(
                direction.opposite(),
                neighbor_pos,
                pos,
                state,
                flags,
                update_limit,
            );
        }
    }

    /// Recomputes a state against all neighbors in vanilla shape-update order.
    pub(crate) fn update_from_neighbor_shapes(
        self: &Arc<Self>,
        state: BlockStateId,
        pos: BlockPos,
    ) -> BlockStateId {
        let mut updated = state;
        for direction in Direction::UPDATE_SHAPE_ORDER {
            let neighbor_pos = pos.relative(direction);
            let neighbor_state = self.get_block_state(neighbor_pos);
            updated = BLOCK_BEHAVIORS
                .get_behavior(updated.get_block())
                .update_shape(updated, self, pos, direction, neighbor_pos, neighbor_state);
        }
        updated
    }

    /// Called when a neighbor's shape changes, to update this block's state.
    ///
    /// This is the Rust equivalent of vanilla's `NeighborUpdater.executeShapeUpdate()`.
    pub(crate) fn neighbor_shape_changed(
        self: &Arc<Self>,
        direction: Direction,
        pos: BlockPos,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
        flags: UpdateFlags,
        update_limit: i32,
    ) {
        self.neighbor_updater.shape_update(
            self,
            ShapeUpdate::new(
                direction,
                neighbor_state,
                pos,
                neighbor_pos,
                flags,
                update_limit,
            ),
        );
    }

    pub(super) fn execute_neighbor_shape_update(
        self: &Arc<Self>,
        direction: Direction,
        pos: BlockPos,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
        flags: UpdateFlags,
        update_limit: i32,
    ) {
        if !self.is_in_valid_bounds(pos) {
            return;
        }

        let current_state = self.get_block_state(pos);

        if flags.contains(UpdateFlags::UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE)
            && current_state.get_block() == &vanilla_blocks::REDSTONE_WIRE
        {
            return;
        }

        let block_behaviors = &*BLOCK_BEHAVIORS;
        let behavior = block_behaviors.get_behavior(current_state.get_block());
        let new_state = behavior.update_shape(
            current_state,
            self,
            pos,
            direction,
            neighbor_pos,
            neighbor_state,
        );

        self.update_or_destroy(current_state, new_state, pos, flags, update_limit);
    }

    pub(crate) fn update_or_destroy(
        self: &Arc<World>,
        old_state: BlockStateId,
        new_state: BlockStateId,
        pos: BlockPos,
        flags: UpdateFlags,
        recursion_left: i32,
    ) {
        if new_state == old_state {
            return;
        }

        if new_state.is_air() {
            self.destroy_block_with_limit(
                pos,
                !flags.contains(UpdateFlags::UPDATE_SUPPRESS_DROPS),
                recursion_left,
            );
        } else {
            self.set_block_with_limit(
                pos,
                new_state,
                flags & !UpdateFlags::UPDATE_SUPPRESS_DROPS,
                recursion_left,
            );
        }
    }

    /// Called when a block changed with a command (setblock, fill, ...)
    ///
    /// This is the Rust equivalent of vanilla's `ServerLevel.updateNeighborsOnBlockSet()`.
    pub(crate) fn update_neighbour_on_block_set(
        self: &Arc<Self>,
        pos: BlockPos,
        old_state: BlockStateId,
    ) {
        let block_state = self.get_block_state(pos);
        // For block behaviors
        let behavior = BLOCK_BEHAVIORS.get_behavior(old_state.get_block());

        if old_state != block_state {
            behavior.affect_neighbors_after_removal(old_state, self, pos, false);
        }

        self.update_neighbors_at(pos, block_state.get_block());

        if behavior.has_analog_output_signal(block_state) {
            self.update_neighbor_for_output_signal(pos, block_state.get_block());
        }
    }

    /// Notifies a block that one of its neighbors changed.
    ///
    /// This is the Rust equivalent of vanilla's `Level.neighborChanged()`.
    pub(crate) fn neighbor_changed(self: &Arc<Self>, pos: BlockPos, source_block: BlockRef) {
        self.neighbor_updater
            .neighbor_changed(self, pos, source_block);
    }

    pub(crate) fn neighbor_changed_with_state(
        self: &Arc<Self>,
        state: BlockStateId,
        pos: BlockPos,
        source_block: BlockRef,
        moved_by_piston: bool,
    ) {
        self.neighbor_updater.neighbor_changed_with_state(
            self,
            state,
            pos,
            source_block,
            moved_by_piston,
        );
    }

    pub(super) fn execute_neighbor_update(
        self: &Arc<Self>,
        state: BlockStateId,
        pos: BlockPos,
        source_block: BlockRef,
        moved_by_piston: bool,
    ) {
        if !self.is_in_valid_bounds(pos) {
            return;
        }
        let block_behaviors = &*BLOCK_BEHAVIORS;
        let behavior = block_behaviors.get_behavior(state.get_block());
        behavior.handle_neighbor_changed(state, self, pos, source_block, moved_by_piston);
    }

    pub(super) const fn chunk_pos_for_block(pos: BlockPos) -> ChunkPos {
        ChunkPos::new(
            SectionPos::block_to_section_coord(pos.0.x),
            SectionPos::block_to_section_coord(pos.0.z),
        )
    }

    /// Gets a block entity at the given position.
    ///
    /// Returns `None` if the chunk is not loaded or there is no block entity at the position.
    #[must_use]
    pub fn get_block_entity(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map
            .with_full_chunk(chunk_pos, |chunk| chunk.get_block_entity_immediate(pos))
            .flatten()
    }

    /// Adds a block entity to the loaded full chunk at its position.
    pub(crate) fn set_block_entity(&self, block_entity: SharedBlockEntity) -> bool {
        let pos = block_entity.get_block_pos();
        if !self.is_in_valid_bounds(pos) {
            return false;
        }

        self.chunk_map
            .with_full_chunk(Self::chunk_pos_for_block(pos), |chunk| {
                chunk.add_and_register_block_entity(block_entity)
            })
            .unwrap_or(false)
    }

    /// Removes a block entity only while it still owns its position.
    pub(crate) fn remove_block_entity_if_same(&self, expected: &dyn BlockEntity) -> bool {
        let pos = expected.get_block_pos();
        if !self.is_in_valid_bounds(pos) {
            return false;
        }

        self.chunk_map
            .with_full_chunk(Self::chunk_pos_for_block(pos), |chunk| {
                chunk.remove_block_entity_if_same(expected)
            })
            .unwrap_or(false)
    }

    /// Called when a block entity's data changes.
    ///
    /// Marks the containing chunk as unsaved so it will be persisted to disk.
    pub fn block_entity_changed(&self, pos: BlockPos) {
        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map.packet_content_changed(chunk_pos);
        self.mark_chunk_dirty(chunk_pos);
    }

    /// Queues a same-state block update and its block-entity update packet.
    ///
    /// Mirrors vanilla `Level.sendBlockUpdated` for callers that changed only
    /// block-entity data.
    pub(crate) fn send_block_updated(&self, pos: BlockPos) {
        self.chunk_map.block_changed(pos);
    }

    /// Marks a chunk as dirty (unsaved) so it will be persisted to disk.
    ///
    /// Called when entities move, are added/removed, or when block entities change.
    pub fn mark_chunk_dirty(&self, chunk_pos: ChunkPos) {
        self.chunk_map
            .with_chunk_at_status(chunk_pos, ChunkStatus::Empty, Chunk::mark_dirty);
    }
}
