use super::{
    AddEntityError, Arc, BLOCK_DROPS, BlockPos, ChunkPos, ChunkStatus, DVec3, Direction, Entity,
    EntityChunkCallback, EntityLifecycleChanges, EntityOwnership, EntityTracker, EntityVisibility,
    ExperienceOrbEntity, FxHashSet, GameEventContext, GameEventDispatcher, GameEventListenerCount,
    GameEventListenerStorage, GameEventRef, InactiveEntityCallback, ItemEntity, ItemStack, Player,
    RemovalReason, SectionPos, SharedEntity, SharedGameEventListener, SyncMutex, World, WorldAabb,
    WorldChangeRequest, block_entity_ticker, mem, vanilla_entities,
};

pub(super) struct NavigatingMobTracker {
    ids: SyncMutex<FxHashSet<i32>>,
}

impl NavigatingMobTracker {
    pub(super) fn new() -> Self {
        Self {
            ids: SyncMutex::new(FxHashSet::default()),
        }
    }

    pub(super) fn track(&self, entity: &SharedEntity) {
        if entity.as_pathfinder_mob().is_some() {
            self.ids.lock().insert(entity.id());
        }
    }

    pub(super) fn untrack(&self, entity_id: i32) {
        self.ids.lock().remove(&entity_id);
    }

    pub(super) fn ids(&self) -> Vec<i32> {
        self.ids.lock().iter().copied().collect()
    }
}

pub(super) fn nearest_player_distance_in_range(
    distance_sqr: f64,
    max_distance: f64,
    max_distance_sqr: f64,
) -> bool {
    max_distance < 0.0 || distance_sqr < max_distance_sqr
}

impl World {
    /// Returns the world-global block-entity ticker owner.
    #[must_use]
    pub(crate) const fn block_entity_tickers(
        &self,
    ) -> &block_entity_ticker::WorldBlockEntityTickers {
        &self.block_entity_tickers
    }

    /// Shares the counter used to skip game-event dispatch when no chunk has listeners.
    #[must_use]
    pub(crate) fn game_event_listener_count(&self) -> Arc<GameEventListenerCount> {
        Arc::clone(&self.game_event_listener_count)
    }

    /// Returns the entity tracker for managing player-entity visibility.
    #[must_use]
    pub const fn entity_tracker(&self) -> &EntityTracker {
        &self.entity_tracker
    }

    pub(super) fn attach_managed_entity_callback(self: &Arc<Self>, entity: &SharedEntity) {
        let callback = Arc::new(EntityChunkCallback::new(entity.id(), Arc::downgrade(self)));
        entity.set_level_callback(callback);
        self.entity_manager.commit_bounding_box_change(entity.id());
    }

    pub(crate) fn add_entity_to_tracker(self: &Arc<Self>, entity: &SharedEntity) {
        self.entity_tracker.add(
            entity,
            |chunk| self.get_packet_tracking_players(chunk),
            |id| self.players.get_by_entity_id(id),
        );
        self.track_navigating_mob(entity);
    }

    pub(crate) fn remove_entity_from_tracker(&self, entity_id: i32) {
        self.entity_tracker.remove(entity_id, |player_id| {
            self.players.get_by_entity_id(player_id)
        });
        self.untrack_navigating_mob(entity_id);
    }

    pub(crate) fn apply_entity_lifecycle_changes(
        self: &Arc<Self>,
        changes: EntityLifecycleChanges,
    ) {
        for entity in changes.tracking_stopped {
            self.remove_entity_from_tracker(entity.id());
        }
        for entity in changes.tracking_started {
            self.add_entity_to_tracker(&entity);
        }
    }

    pub(super) fn track_navigating_mob(&self, entity: &SharedEntity) {
        self.navigating_mobs.track(entity);
    }

    pub(super) fn untrack_navigating_mob(&self, entity_id: i32) {
        self.navigating_mobs.untrack(entity_id);
    }

    pub(crate) fn register_loaded_entity(
        self: &Arc<Self>,
        entity: SharedEntity,
    ) -> Result<(), AddEntityError> {
        let lifecycle = self
            .entity_manager
            .add_live_entity(entity.clone(), EntityOwnership::ManagerOwned)?;
        self.attach_managed_entity_callback(&entity);
        self.apply_entity_lifecycle_changes(lifecycle);
        Ok(())
    }

    pub(crate) fn register_loaded_entity_tree(
        self: &Arc<Self>,
        entities: &[SharedEntity],
    ) -> Result<(), AddEntityError> {
        let lifecycle = self
            .entity_manager
            .add_live_entity_tree(entities, EntityOwnership::ManagerOwned)?;
        for entity in entities {
            self.attach_managed_entity_callback(entity);
        }
        self.apply_entity_lifecycle_changes(lifecycle);
        Ok(())
    }

    pub(crate) fn register_loaded_chunk_entities(
        self: &Arc<Self>,
        source_chunk: ChunkPos,
        persisted_status: ChunkStatus,
        entities: Vec<SharedEntity>,
    ) {
        for tree in Self::loaded_entity_trees(entities) {
            let Some(root) = tree.first() else {
                continue;
            };
            let root_id = root.id();
            let root_uuid = root.uuid();
            let root_type = root.entity_type();
            let root_pos = root.position();
            let root_chunk = ChunkPos::from_entity_pos(root_pos);
            let mut dirty_chunks = FxHashSet::default();
            for entity in &tree {
                let entity_chunk = ChunkPos::from_entity_pos(entity.position());
                if entity_chunk != source_chunk {
                    dirty_chunks.insert(source_chunk);
                    dirty_chunks.insert(entity_chunk);
                }
            }

            if let Err(error) = self.register_loaded_entity_tree(&tree) {
                tracing::warn!(
                    source_chunk = ?source_chunk,
                    ?persisted_status,
                    root_id,
                    uuid = ?root_uuid,
                    entity_type = ?root_type.key,
                    position = ?root_pos,
                    entity_chunk = ?root_chunk,
                    entity_count = tree.len(),
                    "Discarding loaded chunk entity tree that could not be registered: {error}; source_chunk={source_chunk:?}, persisted_status={persisted_status:?}, root_id={root_id}, uuid={root_uuid}, entity_type={:?}, position={root_pos:?}, entity_chunk={root_chunk:?}, entity_count={}",
                    root_type.key,
                    tree.len(),
                );
                Self::discard_loaded_entity_tree(&tree);
                self.mark_chunk_dirty(source_chunk);
                continue;
            }

            for chunk in dirty_chunks {
                self.mark_chunk_dirty(chunk);
            }
        }
    }

    pub(super) fn loaded_entity_trees(entities: Vec<SharedEntity>) -> Vec<Vec<SharedEntity>> {
        let mut seen = FxHashSet::default();
        let mut trees = Vec::new();

        for entity in &entities {
            if entity.is_passenger() {
                continue;
            }
            let mut tree = Vec::new();
            Self::collect_loaded_entity_tree(entity, &mut seen, &mut tree);
            if !tree.is_empty() {
                trees.push(tree);
            }
        }

        for entity in &entities {
            if seen.contains(&entity.id()) {
                continue;
            }
            let mut tree = Vec::new();
            Self::collect_loaded_entity_tree(entity, &mut seen, &mut tree);
            if !tree.is_empty() {
                trees.push(tree);
            }
        }

        trees
    }

    pub(super) fn collect_loaded_entity_tree(
        entity: &SharedEntity,
        seen: &mut FxHashSet<i32>,
        tree: &mut Vec<SharedEntity>,
    ) {
        if !seen.insert(entity.id()) {
            return;
        }
        tree.push(Arc::clone(entity));
        for passenger in entity.passengers() {
            Self::collect_loaded_entity_tree(&passenger, seen, tree);
        }
    }

    pub(super) fn discard_loaded_entity_tree(entities: &[SharedEntity]) {
        for entity in entities {
            entity.set_removed(RemovalReason::Discarded);
        }
    }

    pub(crate) fn has_full_chunk(&self, chunk_pos: ChunkPos) -> bool {
        self.chunk_map
            .with_full_chunk(chunk_pos, |_| true)
            .unwrap_or(false)
    }

    /// Adds a runtime entity to the world.
    pub fn try_add_entity(self: &Arc<Self>, entity: SharedEntity) -> Result<(), AddEntityError> {
        let chunk_pos = ChunkPos::from_entity_pos(entity.position());
        if !self.has_full_chunk(chunk_pos) {
            return Err(AddEntityError::ChunkNotLoaded {
                entity_id: entity.id(),
                chunk: chunk_pos,
            });
        }
        self.register_loaded_entity(entity)?;
        self.mark_chunk_dirty(chunk_pos);
        Ok(())
    }

    pub(crate) fn on_entity_chunk_loaded(self: &Arc<Self>, pos: ChunkPos) {
        // Runtime entity membership follows retained chunk holders, so it
        // starts at Empty rather than waiting for full LevelChunk readiness.
        // Durable entity persistence still starts at StructureStarts; entities
        // in lower-status chunks can be lost if those chunks unload first.
        let result = self.entity_manager.on_chunk_loaded(pos);
        if result.needs_save {
            self.mark_chunk_dirty(pos);
        }
        for entity in result.restored {
            self.attach_managed_entity_callback(&entity);
        }
        self.apply_entity_lifecycle_changes(EntityLifecycleChanges {
            tracking_started: result.tracking_started,
            tracking_stopped: Vec::new(),
            ticking_started: result.ticking_started,
            ticking_stopped: Vec::new(),
        });
    }

    pub(crate) fn update_entity_chunk_visibility(
        self: &Arc<Self>,
        pos: ChunkPos,
        visibility: EntityVisibility,
    ) {
        let changes = self.entity_manager.update_chunk_visibility(pos, visibility);
        self.apply_entity_lifecycle_changes(changes);
    }

    pub(crate) fn on_entity_chunk_unload_start(self: &Arc<Self>, pos: ChunkPos) {
        let result = self.entity_manager.begin_chunk_unload(pos);
        self.apply_entity_lifecycle_changes(EntityLifecycleChanges {
            tracking_started: Vec::new(),
            tracking_stopped: result.tracking_stopped,
            ticking_started: Vec::new(),
            ticking_stopped: result.ticking_stopped,
        });
        for entity in result.retained {
            let entity_id = entity.id();
            entity.set_level_callback(Arc::new(InactiveEntityCallback::new(entity_id)));
        }
    }

    pub(crate) fn on_entity_chunk_unload_finalized(&self, pos: ChunkPos) {
        self.entity_manager.finalize_chunk_unload(pos);
    }

    /// Spawns an item entity at the given position.
    ///
    /// This is a convenience method for dropping items in the world.
    ///
    /// Returns `None` if the item stack is empty.
    pub fn spawn_item(self: &Arc<Self>, pos: DVec3, item: ItemStack) -> Option<Arc<ItemEntity>> {
        // Default ItemEntity velocity: random horizontal scatter + upward pop
        let vx = rand::random::<f64>() * 0.2 - 0.1;
        let vy = 0.2;
        let vz = rand::random::<f64>() * 0.2 - 0.1;
        self.spawn_item_with_velocity(pos, item, DVec3::new(vx, vy, vz))
    }

    /// Spawns an item entity at the given position with initial velocity.
    ///
    /// Returns `None` if the item stack is empty.
    pub fn spawn_item_with_velocity(
        self: &Arc<Self>,
        pos: DVec3,
        item: ItemStack,
        velocity: DVec3,
    ) -> Option<Arc<ItemEntity>> {
        use crate::entity::next_entity_id;

        if item.is_empty() {
            return None;
        }

        let entity_id = next_entity_id();
        let entity = Arc::new(ItemEntity::with_item_and_velocity(
            &vanilla_entities::ITEM,
            entity_id,
            pos,
            item,
            velocity,
            Arc::downgrade(self),
        ));
        if let Err(error) = self.try_add_entity(entity.clone()) {
            log::warn!("Failed to spawn item entity: {error}");
            return None;
        }
        Some(entity)
    }

    /// Drops an item at a block position with random offset and velocity.
    ///
    /// Mirrors vanilla's `Block.popResource()`. Used for block drops.
    /// The item spawns near the center of the block with slight random offset
    /// and small random velocity.
    pub fn pop_resource(
        self: &Arc<Self>,
        pos: BlockPos,
        item: ItemStack,
    ) -> Option<Arc<ItemEntity>> {
        use steel_registry::vanilla_entities;

        if item.is_empty() {
            return None;
        }

        // Respect doTileDrops gamerule
        if !self.get_game_rule(&BLOCK_DROPS) {
            return None;
        }

        // Vanilla uses EntityType.ITEM dimensions for offset calculation
        let half_height = f64::from(vanilla_entities::ITEM.dimensions.height) / 2.0;

        // Random offset within block (vanilla: nextDouble(-0.25, 0.25))
        let x = f64::from(pos.x()) + 0.5 + (rand::random::<f64>() - 0.5) * 0.5;
        let y = f64::from(pos.y()) + 0.5 + (rand::random::<f64>() - 0.5) * 0.5 - half_height;
        let z = f64::from(pos.z()) + 0.5 + (rand::random::<f64>() - 0.5) * 0.5;

        let entity = self.spawn_item(DVec3::new(x, y, z), item)?;
        entity.set_default_pickup_delay();
        Some(entity)
    }

    /// Spawns experience at a block position when block drops are enabled.
    ///
    /// Mirrors Vanilla's `Block.popExperience`.
    pub fn pop_experience(self: &Arc<Self>, pos: BlockPos, amount: i32) {
        if amount <= 0 || !self.get_game_rule(&BLOCK_DROPS) {
            return;
        }

        ExperienceOrbEntity::award(
            self,
            DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()) + 0.5,
                f64::from(pos.z()) + 0.5,
            ),
            amount,
        );
    }

    /// Drops an item from a block face with directional velocity.
    ///
    /// Mirrors vanilla's `Block.popResourceFromFace()`. Used for items ejected
    /// from a specific side of a block.
    pub fn pop_resource_from_face(
        self: &Arc<Self>,
        pos: BlockPos,
        face: Direction,
        item: ItemStack,
    ) -> Option<Arc<ItemEntity>> {
        use steel_registry::vanilla_entities;

        if item.is_empty() {
            return None;
        }

        let half_width = f64::from(vanilla_entities::ITEM.dimensions.width) / 2.0;
        let half_height = f64::from(vanilla_entities::ITEM.dimensions.height) / 2.0;

        let (step_x, step_y, step_z) = face.offset();

        // Position calculation (vanilla logic)
        let x = f64::from(pos.x())
            + 0.5
            + if step_x == 0 {
                (rand::random::<f64>() - 0.5) * 0.5
            } else {
                f64::from(step_x) * (0.5 + half_width)
            };
        let y = f64::from(pos.y())
            + 0.5
            + if step_y == 0 {
                (rand::random::<f64>() - 0.5) * 0.5
            } else {
                f64::from(step_y) * (0.5 + half_height)
            }
            - half_height;
        let z = f64::from(pos.z())
            + 0.5
            + if step_z == 0 {
                (rand::random::<f64>() - 0.5) * 0.5
            } else {
                f64::from(step_z) * (0.5 + half_width)
            };

        // Velocity in direction of face
        let delta_x = if step_x == 0 {
            (rand::random::<f64>() - 0.5) * 0.2
        } else {
            f64::from(step_x) * 0.1
        };
        let delta_y = if step_y == 0 {
            rand::random::<f64>() * 0.1
        } else {
            f64::from(step_y) * 0.1 + 0.1
        };
        let delta_z = if step_z == 0 {
            (rand::random::<f64>() - 0.5) * 0.2
        } else {
            f64::from(step_z) * 0.1
        };

        let entity = self.spawn_item_with_velocity(
            DVec3::new(x, y, z),
            item,
            DVec3::new(delta_x, delta_y, delta_z),
        )?;
        entity.set_default_pickup_delay();
        Some(entity)
    }

    /// Gets an entity by its network ID.
    ///
    /// Returns `None` if the entity is not live in the world.
    #[must_use]
    pub fn get_entity_by_id(&self, id: i32) -> Option<SharedEntity> {
        self.entity_manager.get_by_id(id)
    }

    /// Returns true if this exact entity is live or retained for chunk-unload recovery.
    pub(crate) fn contains_live_or_unloading_entity(&self, entity: &SharedEntity) -> bool {
        self.entity_manager
            .contains_live_or_unloading_entity(entity)
    }

    /// Queues a world change from world-local code for server safe-point processing.
    pub fn queue_world_change(&self, entity: SharedEntity, request: WorldChangeRequest) {
        self.pending_world_changes.lock().push((entity, request));
    }

    pub(crate) fn drain_world_changes(&self) -> Vec<(SharedEntity, WorldChangeRequest)> {
        mem::take(&mut *self.pending_world_changes.lock())
    }

    /// Gets an entity by its network ID if it is visible to vanilla gameplay lookups.
    ///
    /// Returns `None` if the entity is not live or is hidden in an inaccessible chunk.
    #[must_use]
    pub fn get_accessible_entity_by_id(&self, id: i32) -> Option<SharedEntity> {
        self.entity_manager.get_accessible_by_id(id)
    }

    /// Gets an entity by its UUID.
    ///
    /// Returns `None` if the entity is not live in the world.
    #[must_use]
    pub fn get_entity_by_uuid(&self, uuid: &uuid::Uuid) -> Option<SharedEntity> {
        self.entity_manager.get_by_uuid(uuid)
    }

    /// Gets all entities intersecting the given bounding box.
    ///
    /// Only returns entities in loaded chunks.
    #[must_use]
    pub fn get_entities_in_aabb(&self, aabb: &WorldAabb) -> Vec<SharedEntity> {
        self.entity_manager.get_entities_in_aabb(aabb)
    }

    /// Gets entities intersecting the given bounding box and matching `predicate`.
    ///
    /// Only returns entities in loaded chunks.
    #[must_use]
    pub fn get_entities_in_aabb_matching(
        &self,
        aabb: &WorldAabb,
        predicate: impl FnMut(&dyn Entity) -> bool,
    ) -> Vec<SharedEntity> {
        self.entity_manager
            .get_entities_in_aabb_matching(aabb, predicate)
    }

    /// Returns whether any entity intersects the given bounding box and matches `predicate`.
    ///
    /// Only checks entities in loaded chunks.
    #[must_use]
    pub fn has_entity_in_aabb_matching(
        &self,
        aabb: &WorldAabb,
        predicate: impl FnMut(&dyn Entity) -> bool,
    ) -> bool {
        self.entity_manager
            .has_entity_in_aabb_matching(aabb, predicate)
    }

    /// Gets matching entity bounding boxes intersecting the given bounding box.
    ///
    /// Only checks entities in loaded chunks.
    #[must_use]
    pub fn get_entity_bounding_boxes_in_aabb_matching(
        &self,
        aabb: &WorldAabb,
        predicate: impl FnMut(&dyn Entity) -> bool,
    ) -> Vec<WorldAabb> {
        self.entity_manager
            .get_entity_bounding_boxes_in_aabb_matching(aabb, predicate)
    }

    /// Gets the nearest entity intersecting the given bounding box and matching `predicate`.
    ///
    /// Only returns entities in loaded chunks.
    #[must_use]
    pub fn nearest_entity_in_aabb_matching(
        &self,
        aabb: &WorldAabb,
        origin: DVec3,
        predicate: impl FnMut(&dyn Entity) -> bool,
    ) -> Option<SharedEntity> {
        self.entity_manager
            .nearest_entity_in_aabb_matching(aabb, origin, predicate)
    }

    /// Gets the nearest player to `position` within `max_distance`.
    #[must_use]
    pub fn nearest_player(
        &self,
        position: DVec3,
        max_distance: f64,
        mut predicate: impl FnMut(&Player) -> bool,
    ) -> Option<Arc<Player>> {
        let max_distance_sqr = max_distance * max_distance;
        let mut nearest: Option<(Arc<Player>, f64)> = None;
        self.players.iter_players(|_, player| {
            if predicate(player) {
                let distance_sqr = player.position().distance_squared(position);
                if nearest_player_distance_in_range(distance_sqr, max_distance, max_distance_sqr)
                    && nearest
                        .as_ref()
                        .is_none_or(|(_, current)| distance_sqr < *current)
                {
                    nearest = Some((player.clone(), distance_sqr));
                }
            }
            true
        });
        nearest.map(|(player, _)| player)
    }

    /// Gets the squared distance to the nearest player, if any player is present.
    #[must_use]
    pub fn nearest_player_distance_sqr(&self, position: DVec3) -> Option<f64> {
        let mut nearest = None;
        self.players.iter_players(|_, player| {
            if player.is_spectator() {
                return true;
            }
            let distance_sqr = player.position().distance_squared(position);
            if nearest.is_none_or(|current| distance_sqr < current) {
                nearest = Some(distance_sqr);
            }
            true
        });
        nearest
    }

    /// Gets entities matching vanilla's pushable entity selector for `pusher`.
    ///
    /// Vanilla also checks team collision rules; Steel has no teams yet, so this
    /// currently matches the null-team path where collision is allowed.
    #[must_use]
    pub fn get_pushable_entities(
        &self,
        pusher: &dyn Entity,
        aabb: &WorldAabb,
    ) -> Vec<SharedEntity> {
        self.get_entities_in_aabb(aabb)
            .into_iter()
            .filter(|entity| entity.id() != pusher.id())
            .filter(|entity| !entity.is_spectator())
            .filter(|entity| entity.is_pushable())
            .collect()
    }

    /// Registers a game event listener in a chunk section.
    pub fn register_game_event_listener(
        &self,
        section_pos: SectionPos,
        listener: SharedGameEventListener,
    ) {
        let chunk_pos = ChunkPos::new(section_pos.x(), section_pos.z());
        let registry = self.game_event_listener_storage(chunk_pos);
        if let Some(registry) = registry {
            registry.register(section_pos.y(), listener);
        }
    }

    /// Unregisters a game event listener from a chunk section.
    pub fn unregister_game_event_listener(
        &self,
        section_pos: SectionPos,
        listener: &SharedGameEventListener,
    ) -> bool {
        let chunk_pos = ChunkPos::new(section_pos.x(), section_pos.z());
        let registry = self.game_event_listener_storage(chunk_pos);
        registry.is_some_and(|registry| registry.unregister(section_pos.y(), listener))
    }

    /// Returns a stable registry handle without retaining the chunk holder read guard.
    pub(super) fn game_event_listener_storage(
        &self,
        chunk_pos: ChunkPos,
    ) -> Option<Arc<GameEventListenerStorage>> {
        self.chunk_map.with_full_chunk(chunk_pos, |chunk| {
            Arc::clone(&chunk.game_event_listeners().registry)
        })
    }

    /// Dispatches a game event to all listeners in range.
    pub fn game_event(
        self: &Arc<Self>,
        event: GameEventRef,
        pos: BlockPos,
        context: &GameEventContext,
    ) {
        self.game_event_at(
            event,
            DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()) + 0.5,
                f64::from(pos.z()) + 0.5,
            ),
            context,
        );
    }

    /// Dispatches a game event from an exact world position.
    pub fn game_event_at(
        self: &Arc<Self>,
        event: GameEventRef,
        source_pos: DVec3,
        context: &GameEventContext,
    ) {
        if !self.game_event_listener_count.has_any() {
            return;
        }
        let radius = event.notification_radius.max(0);
        let center = BlockPos::from(source_pos);
        let section_min_x = SectionPos::block_to_section_coord(center.x() - radius);
        let section_min_y = SectionPos::block_to_section_coord(center.y() - radius);
        let section_min_z = SectionPos::block_to_section_coord(center.z() - radius);
        let section_max_x = SectionPos::block_to_section_coord(center.x() + radius);
        let section_max_y = SectionPos::block_to_section_coord(center.y() + radius);
        let section_max_z = SectionPos::block_to_section_coord(center.z() + radius);
        let mut dispatcher = GameEventDispatcher::new(self, event, source_pos, context);

        for section_x in section_min_x..=section_max_x {
            for section_z in section_min_z..=section_max_z {
                let registry =
                    self.game_event_listener_storage(ChunkPos::new(section_x, section_z));
                if let Some(registry) = registry {
                    dispatcher.visit_chunk(&registry, section_min_y, section_max_y);
                }
            }
        }

        dispatcher.finish();
    }
}
