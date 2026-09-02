//! Full-chunk behavior over the common owning [`Chunk`].
use std::{
    fmt,
    io::Cursor,
    mem,
    sync::{Arc, Weak, atomic::Ordering},
};

use rustc_hash::FxHashSet;
use steel_protocol::packets::game::{
    BlockEntityInfo, ChunkPacketData, HeightmapType as ProtocolHeightmapType, Heightmaps,
    LightUpdatePacketData,
};
use steel_registry::{
    REGISTRY, RegistryEntry,
    blocks::{BlockRef, block_state_ext::BlockStateExt},
    fluid::FluidRef,
    vanilla_blocks,
};
use steel_utils::{
    BlockPos, BlockStateId, ChunkPos, Direction, PackedChunkLocalXZ, SectionPos, types::UpdateFlags,
};

use steel_utils::locks::SyncMutex;

use crate::behavior::{BLOCK_BEHAVIORS, BlockEntityCreation, FLUID_BEHAVIORS};
use crate::block_entity::{
    BlockEntity, BlockEntityInsert, BlockEntityLifecycleExt as _, BlockEntityLookup,
    ClearedBlockEntities, DetachedBlockEntity, LifecycleDispatchers, SharedBlockEntity,
};
use crate::chunk::{
    Chunk,
    block_entity_listener::{FullChunkGameEventListeners, ListenerSelectionCommit},
    chunk_holder::ChunkHolder,
    data::empty_postprocessing,
    heightmap::{ChunkHeightmaps, HeightmapType},
    light::{
        ChunkLightData, LightSectionEmptinessChange, build_chunk_light_update_packet,
        has_different_light_properties,
    },
    section::Sections,
    status::ChunkStatus,
};
use crate::entity::SharedEntity;
use crate::world::tick_scheduler::{
    BlockTickList, ChunkTickContainer, FluidTickList, ScheduledTickSnapshot, TickPriority,
    TickSchedulerError,
};
use crate::world::{World, game_event::GameEventListenerCount};
use steel_worldgen::structure::{StructureReferenceMap, StructureStartMap};

/// Borrowed capability for Full-only live world access.
///
/// Similar to Java's `LevelChunk`, this holds a weak reference to the world
/// (called `level` in Java) for callbacks during block state changes. Ticking
/// and initial sending additionally require the corresponding neighborhood
/// readiness confirmation from `ChunkMap`.
#[derive(Clone, Copy)]
pub struct FullChunkRef<'a> {
    chunk: &'a Chunk,
}

/// State that only exists once a common chunk crosses the Full boundary.
pub(crate) struct FullChunkRuntime {
    /// Section registries and exact listener selections owned by this retained chunk.
    game_event_listeners: FullChunkGameEventListeners,
    /// Main-boundary activation state and callbacks staged by background loading.
    block_entity_activation: SyncMutex<BlockEntityActivation>,
}

impl FullChunkRuntime {
    fn new(listener_count: Arc<GameEventListenerCount>) -> Self {
        Self {
            game_event_listeners: FullChunkGameEventListeners::new(listener_count),
            block_entity_activation: SyncMutex::new(BlockEntityActivation::default()),
        }
    }
}

impl fmt::Debug for FullChunkRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FullChunkRuntime").finish_non_exhaustive()
    }
}

#[derive(Default)]
struct BlockEntityActivation {
    holder: Weak<ChunkHolder>,
    pending_lifecycle_dispatchers: Vec<SharedBlockEntity>,
}

pub(crate) struct BlockEntityActivationBatch {
    pub(crate) lifecycle_dispatchers: Vec<SharedBlockEntity>,
    pub(crate) positions: Vec<BlockPos>,
}

/// Result of promoting a proto chunk to a full chunk.
pub struct FullChunkPromotion<'a> {
    /// Full capability initialized by this promotion.
    pub chunk: FullChunkRef<'a>,
    /// Entities that should be registered after the full chunk is published.
    pub pending_entities: Vec<SharedEntity>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FullChunkBlockSetResult {
    Changed(BlockStateId),
    Unchanged,
    Stale(BlockStateId),
}

enum PendingPromotionCommit {
    Retry,
    Complete {
        block_entity: Option<SharedBlockEntity>,
        lifecycle_dispatchers: LifecycleDispatchers,
    },
}

enum ProtoBlockEntityAdoption {
    Retry,
    Gone,
    Discarded,
    Adopted(LifecycleDispatchers),
}

fn random_tick_kinds(state: BlockStateId) -> Option<(bool, Option<FluidRef>)> {
    let metadata = state.get_ticking_metadata();
    let tick_block = metadata.randomly_ticking_block();
    let tick_fluid = metadata
        .randomly_ticking_fluid()
        .then_some(metadata.fluid_state().fluid_id);
    (tick_block || tick_fluid.is_some()).then_some((tick_block, tick_fluid))
}

/// Generates section-local random-tick positions with Vanilla's LCG and bit layout.
pub(crate) struct BlockRandomPositionGenerator {
    value: i32,
}

impl BlockRandomPositionGenerator {
    pub(crate) fn from_runtime_rng() -> Self {
        Self::from_seed(rand::random())
    }

    const fn from_seed(value: i32) -> Self {
        Self { value }
    }

    const fn next_local(&mut self) -> (usize, usize, usize) {
        self.value = self.value.wrapping_mul(3).wrapping_add(1_013_904_223);
        let value = self.value >> 2;
        (
            (value & 15) as usize,
            ((value >> 16) & 15) as usize,
            ((value >> 8) & 15) as usize,
        )
    }
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "FullChunkRef is a borrowed capability with a conventional shared-receiver API"
)]
impl FullChunkRef<'_> {
    pub(crate) const fn from_full_context(chunk: &Chunk) -> FullChunkRef<'_> {
        FullChunkRef { chunk }
    }

    fn runtime(&self) -> &FullChunkRuntime {
        let Some(runtime) = self.chunk.full_runtime() else {
            panic!("Full chunk view was exposed without initialized runtime state");
        };
        runtime
    }

    pub(crate) fn game_event_listeners(&self) -> &FullChunkGameEventListeners {
        &self.runtime().game_event_listeners
    }

    /// Returns the data retained across generation and Full runtime access.
    #[must_use]
    pub(crate) const fn common(&self) -> &Chunk {
        self.chunk
    }

    /// Returns the sections shared with the generation phase.
    #[must_use]
    pub const fn sections(&self) -> &Sections {
        self.chunk.sections()
    }

    /// Runs random block and fluid ticks for this chunk.
    pub(crate) fn tick_random_blocks(
        &self,
        world: &Arc<World>,
        random_tick_speed: u32,
        random_positions: &mut BlockRandomPositionGenerator,
    ) {
        if random_tick_speed == 0 {
            return;
        }

        let block_behaviors = &*BLOCK_BEHAVIORS;
        let fluid_behaviors = &*FLUID_BEHAVIORS;
        let chunk_base_x = self.chunk.pos.0.x * 16;
        let chunk_base_z = self.chunk.pos.0.y * 16;
        let random_tick_sections = self.chunk.sections.random_tick_sections();
        let mut next_section_index = 0;

        while let Some(section_index) = random_tick_sections.next(next_section_index) {
            next_section_index = section_index + 1;
            let section = &self.chunk.sections.sections[section_index];
            let section_base_y = self.min_y() + (section_index as i32 * 16);
            // Keep the guard across no-op samples. Extreme random tick speeds may
            // delay concurrent writers, but avoid lock churn in normal ticking.
            let mut section_guard = None;

            for _ in 0..random_tick_speed {
                let (local_x, local_y, local_z) = random_positions.next_local();
                let state = section_guard
                    .get_or_insert_with(|| section.read())
                    .states
                    .get(local_x, local_y, local_z);
                let Some((tick_block, tick_fluid)) = random_tick_kinds(state) else {
                    continue;
                };
                // Either callback may write this section. Reacquire lazily so
                // the next sample observes all changes made by this tick.
                drop(section_guard.take());
                let pos = BlockPos::new(
                    chunk_base_x + local_x as i32,
                    section_base_y + local_y as i32,
                    chunk_base_z + local_z as i32,
                );

                if tick_block {
                    block_behaviors
                        .get_behavior(state.get_block())
                        .random_tick(state, world, pos);
                }
                if let Some(fluid) = tick_fluid {
                    fluid_behaviors.get_behavior(fluid).random_tick(world, pos);
                }
            }
        }
    }
}

impl Chunk {
    fn initialize_full_runtime_state(&self, listener_count: Arc<GameEventListenerCount>) {
        let runtime = FullChunkRuntime::new(listener_count);
        assert!(
            self.initialize_full_runtime(runtime).is_ok(),
            "Full chunk runtime was initialized more than once"
        );
    }

    /// Promotes this chunk to Full status and initializes its runtime state.
    ///
    /// Transfers the chunk's heightmaps after ensuring every final map is primed.
    /// Recalculates section block counts for random tick optimization.
    ///
    /// # Panics
    /// Panics if this chunk's light-section count does not match its world height.
    ///
    #[must_use]
    pub(crate) fn promote_to_full(&self) -> FullChunkPromotion<'_> {
        let proto_chunk = self;
        // Generation-only caches are never retained by a Full chunk. Carvers normally
        // consume them earlier; promotion is the defensive lifecycle boundary.
        proto_chunk.clear_transient_generation_state();
        let min_y = proto_chunk.min_y();
        let height = proto_chunk.height();
        let level = proto_chunk.level_weak();
        // Ensure full chunks always have populated final heightmaps. Some stages
        // may not touch blocks (carvers are currently empty), so lazy final
        // heightmaps are not guaranteed to exist before promotion.
        {
            let mut heightmaps = proto_chunk.heightmaps.write();
            heightmaps.prime_from_sections(
                HeightmapType::final_types(),
                min_y,
                height,
                &proto_chunk.sections.sections,
            );
            for &heightmap_type in HeightmapType::final_types() {
                let _ = heightmaps.get_final(heightmap_type);
            }
        }

        // Recalculate section counts for random tick optimization
        for section in &proto_chunk.sections.sections {
            section.write().recalculate_counts();
        }

        // Vanilla keeps proto ticks pending through Full promotion. Retaining
        // the same container also makes the promotion linearizable with any
        // concurrent scheduling against the chunk.
        assert!(
            proto_chunk.scheduled_tick_container().promote_to_full(),
            "Proto chunk scheduled-tick container was already promoted"
        );
        let pending_entities = proto_chunk.entities.close_and_drain();
        if let Err(error) = proto_chunk
            .light
            .write()
            .refresh_emptiness_maps_from_sections(&proto_chunk.sections)
        {
            panic!("invalid proto chunk light emptiness map length: {error:?}");
        }

        FullChunkRef::populate_poi(&level, &proto_chunk.sections, proto_chunk.pos, min_y);
        let game_event_listener_count = level
            .upgrade()
            .map_or_else(GameEventListenerCount::shared, |world| {
                world.game_event_listener_count()
            });

        proto_chunk.initialize_full_runtime_state(game_event_listener_count);
        let full = FullChunkRef::from_full_context(proto_chunk);
        full.adopt_proto_block_entities();
        FullChunkPromotion {
            chunk: full,
            pending_entities,
        }
    }

    /// Creates an owning Full chunk loaded from disk (not dirty).
    ///
    /// Recalculates section block counts for random tick optimization.
    ///
    /// # Arguments
    /// * `sections` - The chunk sections
    /// * `pos` - The chunk position
    /// * `min_y` - The minimum Y coordinate of the world
    /// * `height` - The total height of the world
    /// * `level` - Weak reference to the world (mirrors Java's `LevelChunk.level`)
    /// * `block_ticks` - Scheduled block ticks loaded from disk
    /// * `fluid_ticks` - Scheduled fluid ticks loaded from disk
    /// * `heightmaps` - Heightmaps loaded from disk
    /// * `postprocessing` - Pending postprocessing offsets loaded from disk
    /// * `light` - Chunk-owned light data loaded from disk
    ///
    /// # Panics
    /// Panics if the loaded light-section count does not match the chunk's world height.
    ///
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "all parameters are required to fully restore a chunk from disk"
    )]
    pub(crate) fn from_full_disk(
        sections: Sections,
        pos: ChunkPos,
        min_y: i32,
        height: i32,
        level: Weak<World>,
        block_ticks: BlockTickList,
        fluid_ticks: FluidTickList,
        mut heightmaps: ChunkHeightmaps,
        postprocessing: Vec<Vec<u16>>,
        structure_starts: StructureStartMap,
        structure_references: StructureReferenceMap,
        light: ChunkLightData,
    ) -> Chunk {
        // Disk payloads may omit maps that are derivable from section data.
        // Full construction owns the invariant that every final map exists;
        // callers cannot publish a partially initialized runtime chunk.
        heightmaps.prime_from_sections(
            HeightmapType::final_types(),
            min_y,
            height,
            &sections.sections,
        );

        let chunk = Chunk::from_disk(
            sections,
            pos,
            ChunkStatus::Full,
            min_y,
            height,
            heightmaps,
            structure_starts,
            structure_references,
            None,
            postprocessing,
            block_ticks,
            fluid_ticks,
            level.clone(),
            light,
        );

        FullChunkRef::populate_poi(&level, &chunk.sections, pos, min_y);
        let game_event_listener_count = level
            .upgrade()
            .map_or_else(GameEventListenerCount::shared, |world| {
                world.game_event_listener_count()
            });

        chunk.initialize_full_runtime_state(game_event_listener_count);
        chunk
    }
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "FullChunkRef is a borrowed capability with a conventional shared-receiver API"
)]
impl FullChunkRef<'_> {
    /// Revalidates proto block entities while retaining the same storage instance.
    fn adopt_proto_block_entities(&self) {
        // Vanilla's source is a HashMap. Preserve the storage's native order at
        // this promotion-only boundary rather than imposing a new ordering.
        let block_entities = self
            .chunk
            .block_entity_storage()
            .get_all_without_lifecycle_filter();
        for block_entity in block_entities {
            let pos = block_entity.get_block_pos();
            if ChunkPos::from_block_pos(pos) != self.chunk.pos {
                log::warn!(
                    "Trying to promote block entity {} at {pos:?} in chunk {:?}",
                    block_entity.get_type().key,
                    self.chunk.pos,
                );
                self.chunk
                    .block_entity_storage()
                    .discard_if_same_without_lifecycle(pos, &block_entity);
                continue;
            }

            loop {
                let state = self.chunk.get_block_state(pos);
                let valid = state.has_block_entity() && block_entity.is_valid_block_state(state);
                let adoption = self.with_locked_block_state(pos, |live_state| {
                    if live_state != state {
                        return ProtoBlockEntityAdoption::Retry;
                    }
                    if !valid {
                        return if self
                            .chunk
                            .block_entity_storage()
                            .discard_if_same_without_lifecycle(pos, &block_entity)
                        {
                            ProtoBlockEntityAdoption::Discarded
                        } else {
                            ProtoBlockEntityAdoption::Gone
                        };
                    }
                    self.chunk
                        .block_entity_storage()
                        .adopt_if_same_staged(pos, &block_entity, state)
                        .map_or(
                            ProtoBlockEntityAdoption::Gone,
                            ProtoBlockEntityAdoption::Adopted,
                        )
                });

                match adoption {
                    ProtoBlockEntityAdoption::Retry => {}
                    ProtoBlockEntityAdoption::Gone => break,
                    ProtoBlockEntityAdoption::Discarded => {
                        log::warn!(
                            "Discarding promoted block entity {} at {pos:?}: block {} does not accept that type",
                            block_entity.get_type().key,
                            state.get_block().key,
                        );
                        break;
                    }
                    ProtoBlockEntityAdoption::Adopted(lifecycle_dispatchers) => {
                        let cached_state = block_entity.get_block_state();
                        if state.get_block() != cached_state.get_block() {
                            log::warn!(
                                "Updating mismatched block entity {} state at {pos:?}: {} != {}",
                                block_entity.get_type().key,
                                state.get_block().key,
                                cached_state.get_block().key,
                            );
                        }
                        self.finish_block_entity_change(pos, lifecycle_dispatchers);
                        self.mark_unsaved();
                        break;
                    }
                }
            }
        }
    }

    /// Returns a reference to the world if it's still alive.
    ///
    /// This mirrors Java's `LevelChunk.getLevel()`.
    #[must_use]
    pub fn get_level(&self) -> Option<Arc<World>> {
        self.chunk.get_level()
    }

    /// Returns the weak reference to the world.
    ///
    /// Use this when you need to pass the world reference to block entities
    /// at construction time.
    #[must_use]
    pub fn level_weak(&self) -> Weak<World> {
        self.chunk.level_weak()
    }

    /// Activates load-staged block entities at the serialized chunk lifecycle boundary.
    ///
    /// The holder is installed before callbacks so reentrant changes register directly
    /// into the world ticker. Existing storage order is retained for this load-only step.
    pub(crate) fn prepare_block_entity_activation(
        &self,
        holder: &Arc<ChunkHolder>,
    ) -> Option<BlockEntityActivationBatch> {
        let lifecycle_dispatchers = {
            let mut activation = self.runtime().block_entity_activation.lock();
            if let Some(current) = activation.holder.upgrade() {
                debug_assert!(Arc::ptr_eq(&current, holder));
                if Arc::ptr_eq(&current, holder) {
                    return None;
                }
            }
            activation.holder = Arc::downgrade(holder);
            mem::take(&mut activation.pending_lifecycle_dispatchers)
        };

        let mut known_positions = FxHashSet::default();
        let mut positions = self
            .chunk
            .block_entity_storage()
            .get_all_without_lifecycle_filter()
            .into_iter()
            .map(|block_entity| block_entity.get_block_pos())
            .inspect(|pos| {
                known_positions.insert(*pos);
            })
            .collect::<Vec<_>>();
        positions.extend(
            self.runtime()
                .game_event_listeners
                .block_entity_positions()
                .into_iter()
                .filter(|pos| known_positions.insert(*pos)),
        );
        Some(BlockEntityActivationBatch {
            lifecycle_dispatchers,
            positions,
        })
    }

    /// Deactivates one finalized holder and returns callbacks staged before activation.
    #[must_use]
    pub(crate) fn deactivate_block_entities(
        &self,
        holder: &Arc<ChunkHolder>,
    ) -> Vec<SharedBlockEntity> {
        let mut activation = self.runtime().block_entity_activation.lock();
        let belongs = activation
            .holder
            .upgrade()
            .is_some_and(|current| Arc::ptr_eq(&current, holder));
        if belongs {
            activation.holder = Weak::new();
        }
        mem::take(&mut activation.pending_lifecycle_dispatchers)
    }

    /// Makes an unloading holder dormant without discarding wrapper identity.
    pub(crate) fn suspend_block_entities(&self, holder: &Arc<ChunkHolder>) {
        let mut activation = self.runtime().block_entity_activation.lock();
        let belongs = activation
            .holder
            .upgrade()
            .is_some_and(|current| Arc::ptr_eq(&current, holder));
        if belongs {
            activation.holder = Weak::new();
        }
    }

    /// Captures a live state only while `expected` remains the exact storage owner.
    ///
    /// The section read precedes the storage read, matching block-state writers. Both
    /// guards are dropped before the caller selects or invokes behavior.
    #[must_use]
    pub(crate) fn block_entity_tick_state_if_owned(
        &self,
        pos: BlockPos,
        expected: &SharedBlockEntity,
    ) -> Option<BlockStateId> {
        let (state, owner_matches) = self.with_locked_block_state(pos, |state| {
            (
                state,
                self.chunk
                    .block_entity_storage()
                    .contains_same(pos, expected),
            )
        });
        owner_matches.then_some(state)
    }

    pub(crate) fn block_entity_tick_target(
        &self,
        pos: BlockPos,
    ) -> Option<(BlockStateId, SharedBlockEntity)> {
        self.with_locked_block_state(pos, |state| {
            self.chunk
                .block_entity_storage()
                .get(pos)
                .map(|entity| (state, entity))
        })
    }

    fn finish_block_entity_change(
        &self,
        pos: BlockPos,
        lifecycle_dispatchers: LifecycleDispatchers,
    ) {
        {
            let mut activation = self.runtime().block_entity_activation.lock();
            if activation.holder.upgrade().is_none() {
                activation
                    .pending_lifecycle_dispatchers
                    .extend(lifecycle_dispatchers);
                return;
            }
        }
        let current = self
            .chunk
            .block_entity_storage()
            .get(pos)
            .filter(|block_entity| !block_entity.is_removed());
        self.runtime()
            .game_event_listeners
            .remove_obsolete(pos, current.as_ref());
        for block_entity in lifecycle_dispatchers {
            block_entity.dispatch_lifecycle_events();
        }
        self.reconcile_block_entity_game_event_listener(pos);
        self.refresh_block_entity_ticker(pos);
    }

    /// Re-selects one listener without retaining section, storage, or binding locks across the
    /// block/provider callback.
    pub(crate) fn reconcile_block_entity_game_event_listener(&self, pos: BlockPos) {
        loop {
            let current = self
                .chunk
                .block_entity_storage()
                .get(pos)
                .filter(|block_entity| !block_entity.is_removed());
            self.runtime()
                .game_event_listeners
                .remove_obsolete(pos, current.as_ref());
            let Some(block_entity) = current else {
                return;
            };
            if self
                .runtime()
                .game_event_listeners
                .is_selected(&block_entity)
            {
                return;
            }

            let Some((state, live_entity)) = self.block_entity_tick_target(pos) else {
                continue;
            };
            if !Arc::ptr_eq(&block_entity, &live_entity) || live_entity.is_removed() {
                continue;
            }
            let Some(world) = self.get_level() else {
                return;
            };
            let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
            let listener = behavior.get_game_event_listener(&world, live_entity.as_ref());

            let still_owned = self.with_locked_block_state(pos, |live_state| {
                live_state == state
                    && !live_entity.is_removed()
                    && self
                        .chunk
                        .block_entity_storage()
                        .contains_same(pos, &live_entity)
            });
            if !still_owned {
                continue;
            }
            if self
                .runtime()
                .block_entity_activation
                .lock()
                .holder
                .upgrade()
                .is_none()
            {
                return;
            }

            match self
                .runtime()
                .game_event_listeners
                .commit_selection(live_entity, listener)
            {
                ListenerSelectionCommit::Committed | ListenerSelectionCommit::AlreadySelected => {
                    return;
                }
                ListenerSelectionCommit::Occupied => {}
            }
        }
    }

    fn refresh_block_entity_ticker(&self, pos: BlockPos) {
        let holder = {
            let activation = self.runtime().block_entity_activation.lock();
            activation.holder.upgrade()
        };
        let Some(holder) = holder else {
            return;
        };
        let Some(world) = self.get_level() else {
            return;
        };
        let Some((state, block_entity)) = self.block_entity_tick_target(pos) else {
            world.block_entity_tickers().remove(&holder, pos);
            return;
        };
        if block_entity.is_removed() {
            world.block_entity_tickers().remove(&holder, pos);
            return;
        }

        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        let ticker = behavior.get_block_entity_ticker(&world, state, block_entity.get_type());
        let ticker = ticker.filter(|ticker| {
            let valid = ticker.accepts(block_entity.get_type());
            if !valid {
                tracing::error!(
                    block = %state.get_block().key,
                    block_entity_type = %block_entity.get_type().key,
                    ?pos,
                    "Block behavior returned a ticker for the wrong block-entity type"
                );
            }
            valid
        });
        world
            .block_entity_tickers()
            .reconcile(&holder, block_entity, ticker);
    }

    /// Returns this chunk's stable scheduled-tick container.
    pub(crate) const fn scheduled_tick_container(&self) -> &Arc<ChunkTickContainer> {
        self.chunk.scheduled_tick_container()
    }

    pub(crate) fn schedule_unregistered_block_tick(
        &self,
        block: BlockRef,
        pos: BlockPos,
        trigger_tick: i64,
        priority: TickPriority,
        sub_tick_order: i64,
    ) -> Option<bool> {
        self.chunk
            .scheduled_tick_container()
            .schedule_unregistered_block(block, pos, trigger_tick, priority, sub_tick_order)
    }

    pub(crate) fn schedule_unregistered_fluid_tick(
        &self,
        fluid: FluidRef,
        pos: BlockPos,
        trigger_tick: i64,
        priority: TickPriority,
        sub_tick_order: i64,
    ) -> Option<bool> {
        self.chunk
            .scheduled_tick_container()
            .schedule_unregistered_fluid(fluid, pos, trigger_tick, priority, sub_tick_order)
    }

    pub(crate) fn has_scheduled_block_tick(
        &self,
        pos: BlockPos,
        block: BlockRef,
    ) -> Result<bool, TickSchedulerError> {
        self.chunk
            .scheduled_tick_container()
            .has_block(pos, block)
            .ok_or(TickSchedulerError::MissingContainer(self.chunk.pos))
    }

    pub(crate) fn has_scheduled_fluid_tick(
        &self,
        pos: BlockPos,
        fluid: FluidRef,
    ) -> Result<bool, TickSchedulerError> {
        self.chunk
            .scheduled_tick_container()
            .has_fluid(pos, fluid)
            .ok_or(TickSchedulerError::MissingContainer(self.chunk.pos))
    }

    /// Schedules through the world index, or through local pre-publication
    /// storage when this chunk has no live world (as in focused unit tests).
    pub(crate) fn schedule_block_tick(
        &self,
        pos: BlockPos,
        block: BlockRef,
        trigger_tick: i64,
        priority: TickPriority,
        sub_tick_order: i64,
    ) {
        let result = if let Some(world) = self.get_level() {
            world.schedule_block_tick_for_chunk(
                *self,
                pos,
                block,
                trigger_tick,
                priority,
                sub_tick_order,
            )
        } else {
            self.schedule_unregistered_block_tick(
                block,
                pos,
                trigger_tick,
                priority,
                sub_tick_order,
            )
            .ok_or(TickSchedulerError::MissingContainer(self.chunk.pos))
        };

        match result {
            Ok(true) => self.chunk.dirty.store(true, Ordering::Release),
            Ok(false) => {}
            Err(error) => panic!("Full chunk scheduled-tick ownership invariant failed: {error:?}"),
        }
    }

    pub(crate) fn schedule_fluid_tick(
        &self,
        pos: BlockPos,
        fluid: FluidRef,
        trigger_tick: i64,
        priority: TickPriority,
        sub_tick_order: i64,
    ) {
        let result = if let Some(world) = self.get_level() {
            world.schedule_fluid_tick_for_chunk(
                *self,
                pos,
                fluid,
                trigger_tick,
                priority,
                sub_tick_order,
            )
        } else {
            self.schedule_unregistered_fluid_tick(
                fluid,
                pos,
                trigger_tick,
                priority,
                sub_tick_order,
            )
            .ok_or(TickSchedulerError::MissingContainer(self.chunk.pos))
        };

        match result {
            Ok(true) => self.chunk.dirty.store(true, Ordering::Release),
            Ok(false) => {}
            Err(error) => panic!("Full chunk scheduled-tick ownership invariant failed: {error:?}"),
        }
    }

    /// Takes an owned persistence snapshot without exposing live scheduler data.
    pub(crate) fn scheduled_tick_snapshot(&self) -> ScheduledTickSnapshot {
        let current_tick = self.get_level().map_or(0, |world| world.game_time());
        let result = self
            .chunk
            .scheduled_tick_container()
            .snapshot(current_tick)
            .ok_or(TickSchedulerError::MissingContainer(self.chunk.pos));
        match result {
            Ok(snapshot) => snapshot,
            Err(error) => panic!("Full chunk scheduled-tick ownership invariant failed: {error:?}"),
        }
    }

    /// Fills the vanilla skylight-source cache from current section contents.
    pub fn initialize_light_sources(&self) {
        self.refresh_light_emptiness_maps();
        self.chunk
            .sky_light_sources
            .write()
            .fill_from_sections(&self.chunk.sections);
    }

    /// Drains pending vanilla generation postprocessing offsets.
    pub(crate) fn take_postprocessing(&self) -> Option<Box<[Vec<u16>]>> {
        let mut postprocessing = self.chunk.postprocessing.lock();
        if postprocessing.iter().all(Vec::is_empty) {
            return None;
        }

        let pending = mem::replace(&mut *postprocessing, empty_postprocessing(self.height()));
        self.chunk.dirty.store(true, Ordering::Release);
        Some(pending)
    }

    /// Snapshots pending postprocessing offsets for chunk persistence.
    pub(crate) fn postprocessing_for_serialization(&self) -> Vec<Vec<u16>> {
        self.chunk
            .postprocessing
            .lock()
            .iter()
            .map(Vec::clone)
            .collect()
    }

    /// Runs pending vanilla generation postprocessing at the r1 readiness transition.
    pub(crate) fn post_process_generation(
        world: &Arc<World>,
        chunk_pos: ChunkPos,
        min_y: i32,
        postprocessing: Box<[Vec<u16>]>,
    ) {
        for (section_index, packed_offsets) in postprocessing.into_vec().into_iter().enumerate() {
            if packed_offsets.is_empty() {
                continue;
            }

            let section_y = Self::section_y_from_section_index(min_y, section_index);
            for packed in packed_offsets {
                let pos = Chunk::unpack_postprocessing_offset(packed, section_y, chunk_pos);
                let state = world.get_block_state(pos);
                let fluid_state = state.get_fluid_state();

                if !fluid_state.is_empty() {
                    FLUID_BEHAVIORS.get_behavior(fluid_state.fluid_id).tick(
                        world,
                        pos,
                        state,
                        fluid_state,
                    );
                }

                if state.get_block().config.liquid {
                    BLOCK_BEHAVIORS
                        .get_behavior(state.get_block())
                        .tick(state, world, pos);
                } else {
                    let new_state = Self::update_from_neighbor_shapes(world, state, pos);
                    if new_state != state {
                        let flags = UpdateFlags::UPDATE_INVISIBLE
                            | UpdateFlags::UPDATE_KNOWN_SHAPE
                            | UpdateFlags::UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS;
                        world.set_block(pos, new_state, flags);
                    }
                }
            }
        }
    }

    fn update_from_neighbor_shapes(
        world: &Arc<World>,
        state: BlockStateId,
        pos: BlockPos,
    ) -> BlockStateId {
        let mut updated = state;
        for direction in Direction::UPDATE_SHAPE_ORDER {
            let neighbor_pos = pos.relative(direction);
            let neighbor_state = world.get_block_state(neighbor_pos);
            let behavior = BLOCK_BEHAVIORS.get_behavior(updated.get_block());
            updated =
                behavior.update_shape(updated, world, pos, direction, neighbor_pos, neighbor_state);
        }
        updated
    }

    /// Scans chunk sections for POI block states and populates world POI storage.
    fn populate_poi(level: &Weak<World>, sections: &Sections, pos: ChunkPos, min_y: i32) {
        let Some(world) = level.upgrade() else {
            return;
        };

        // Palette pre-check WITHOUT the global POI lock: collect only the
        // sections that actually contain POI blocks. The vast majority of
        // worldgen chunks have none, so they never touch the (heavily
        // contended) `poi_storage` mutex and never do a per-block scan.
        // `Vec::new()` doesn't allocate until the first push, so the common
        // empty case is allocation-free.
        let mut poi_sections: Vec<(usize, SectionPos)> = Vec::new();
        for (i, section) in sections.sections.iter().enumerate() {
            if section.read().contains_poi() {
                let section_y = min_y / 16 + i as i32;
                poi_sections.push((i, SectionPos::new(pos.0.x, section_y, pos.0.y)));
            }
        }
        if poi_sections.is_empty() {
            return;
        }

        let mut poi_storage = world.poi_storage.lock();
        for (i, section_pos) in poi_sections {
            let guard = sections.sections[i].read();
            poi_storage.scan_and_populate(&guard, section_pos);
        }
    }

    /// Returns the minimum Y coordinate of the world.
    #[must_use]
    pub const fn min_y(&self) -> i32 {
        self.chunk.min_y()
    }

    /// Returns the total height of the world.
    #[must_use]
    pub const fn height(&self) -> i32 {
        self.chunk.height()
    }

    /// Gets the first available Y coordinate for a heightmap column.
    #[must_use]
    pub fn get_height(&self, heightmap_type: HeightmapType, local_x: usize, local_z: usize) -> i32 {
        self.chunk
            .heightmaps
            .read()
            .get_final(heightmap_type)
            .get_first_available(local_x, local_z)
    }

    /// Gets the section index for a given Y coordinate.
    #[must_use]
    const fn get_section_index(&self, y: i32) -> usize {
        ((y - self.min_y()) / 16) as usize
    }

    #[must_use]
    const fn section_y_from_section_index(min_y: i32, index: usize) -> i32 {
        min_y.div_euclid(16) + index as i32
    }

    /// Marks the chunk as unsaved.
    fn mark_unsaved(&self) {
        self.chunk.dirty.store(true, Ordering::Release);
    }

    /// Gets a block entity at the given position.
    ///
    /// Returns `None` if no block entity exists at the position.
    #[must_use]
    pub fn get_block_entity(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        loop {
            match self.chunk.block_entity_storage().lookup(pos) {
                BlockEntityLookup::Concrete(block_entity) => {
                    if block_entity.is_removed() {
                        if self
                            .chunk
                            .block_entity_storage()
                            .remove_if_same_and_removed(pos, &block_entity)
                        {
                            self.finish_block_entity_change(pos, LifecycleDispatchers::new());
                            return None;
                        }
                        continue;
                    }
                    return Some(block_entity);
                }
                BlockEntityLookup::Pending => return self.promote_pending_block_entity(pos),
                BlockEntityLookup::Absent => return None,
            }
        }
    }

    /// Gets a block entity, creating the live block's implementation when storage is missing.
    ///
    /// This is Vanilla's `EntityCreationType.IMMEDIATE` path used by `Level.getBlockEntity`.
    #[must_use]
    pub(crate) fn get_block_entity_immediate(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        if ChunkPos::from_block_pos(pos) != self.chunk.pos {
            return None;
        }
        loop {
            match self.chunk.block_entity_storage().lookup(pos) {
                BlockEntityLookup::Concrete(block_entity) => {
                    if block_entity.is_removed() {
                        if self
                            .chunk
                            .block_entity_storage()
                            .remove_if_same_and_removed(pos, &block_entity)
                        {
                            self.finish_block_entity_change(pos, LifecycleDispatchers::new());
                        }
                        continue;
                    }
                    return Some(block_entity);
                }
                BlockEntityLookup::Pending => return self.promote_pending_block_entity(pos),
                BlockEntityLookup::Absent => {}
            }

            let state = self.get_block_state(pos);
            if !state.has_block_entity() {
                let state_unchanged =
                    self.with_locked_block_state(pos, |live_state| live_state == state);
                if state_unchanged {
                    return None;
                }
                continue;
            }

            let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
            match behavior.new_block_entity(self.chunk.level_weak(), pos, state) {
                BlockEntityCreation::Created(block_entity) => {
                    let valid = block_entity.get_block_pos() == pos
                        && block_entity.is_valid_block_state(state);
                    let inserted = self.with_locked_block_state(pos, |live_state| {
                        if live_state != state {
                            return None;
                        }
                        if !valid {
                            return Some(None);
                        }
                        Some(Some(
                            self.chunk
                                .block_entity_storage()
                                .insert_if_absent_staged(&block_entity, state),
                        ))
                    });
                    let Some(inserted) = inserted else {
                        continue;
                    };
                    let inserted = inserted?;
                    match inserted {
                        BlockEntityInsert::Existing(existing) => return Some(existing),
                        BlockEntityInsert::Inserted(lifecycle_dispatchers) => {
                            self.finish_block_entity_change(pos, lifecycle_dispatchers);
                            self.mark_unsaved();
                            return Some(block_entity);
                        }
                    }
                }
                BlockEntityCreation::NoEntity => {
                    let state_unchanged =
                        self.with_locked_block_state(pos, |live_state| live_state == state);
                    if state_unchanged {
                        return None;
                    }
                }
                BlockEntityCreation::Unimplemented => {
                    let inserted = self.with_locked_block_state(pos, |live_state| {
                        live_state == state && self.chunk.block_entity_storage().set_pending(pos)
                    });
                    if inserted {
                        self.mark_unsaved();
                    }
                    return None;
                }
            }
        }
    }

    fn promote_pending_block_entity(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        loop {
            match self.chunk.block_entity_storage().lookup(pos) {
                BlockEntityLookup::Concrete(block_entity) => {
                    if block_entity.is_removed() {
                        if self
                            .chunk
                            .block_entity_storage()
                            .remove_if_same_and_removed(pos, &block_entity)
                        {
                            self.finish_block_entity_change(pos, LifecycleDispatchers::new());
                        }
                        continue;
                    }
                    return Some(block_entity);
                }
                BlockEntityLookup::Pending => {}
                BlockEntityLookup::Absent => return None,
            }

            let state = self.get_block_state(pos);
            if !state.has_block_entity() {
                let state_unchanged = self.with_locked_block_state(pos, |live_state| {
                    if live_state != state {
                        return false;
                    }
                    self.chunk.block_entity_storage().remove_pending(pos);
                    true
                });
                if !state_unchanged {
                    continue;
                }
                log::warn!(
                    "Tried to promote a pending block entity at {pos:?}, but block {} does not allow one",
                    state.get_block().key,
                );
                return None;
            }

            let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
            let creation = behavior.new_block_entity(self.chunk.level_weak(), pos, state);
            match self.commit_pending_creation(pos, state, creation) {
                PendingPromotionCommit::Retry => {}
                PendingPromotionCommit::Complete {
                    block_entity,
                    lifecycle_dispatchers,
                } => {
                    self.finish_block_entity_change(pos, lifecycle_dispatchers);
                    return block_entity;
                }
            }
        }
    }

    fn commit_pending_creation(
        &self,
        pos: BlockPos,
        expected_state: BlockStateId,
        creation: BlockEntityCreation,
    ) -> PendingPromotionCommit {
        match creation {
            BlockEntityCreation::Created(block_entity) => {
                let valid = block_entity.get_block_pos() == pos
                    && ChunkPos::from_block_pos(pos) == self.chunk.pos
                    && block_entity.is_valid_block_state(expected_state);
                self.with_locked_block_state(pos, |live_state| {
                    // The block behavior owns its factory. A result created from an obsolete
                    // state must never consume a marker installed by the replacement block, even
                    // when both states accept the same block-entity type.
                    if live_state != expected_state {
                        return PendingPromotionCommit::Retry;
                    }
                    if !valid {
                        return PendingPromotionCommit::Complete {
                            block_entity: None,
                            lifecycle_dispatchers: LifecycleDispatchers::new(),
                        };
                    }
                    let (block_entity, lifecycle_dispatchers) = self
                        .chunk
                        .block_entity_storage()
                        .promote_staged(pos, expected_state, block_entity);
                    PendingPromotionCommit::Complete {
                        block_entity,
                        lifecycle_dispatchers,
                    }
                })
            }
            BlockEntityCreation::NoEntity => self.with_locked_block_state(pos, |live_state| {
                if live_state != expected_state {
                    return PendingPromotionCommit::Retry;
                }
                self.chunk.block_entity_storage().remove_pending(pos);
                PendingPromotionCommit::Complete {
                    block_entity: None,
                    lifecycle_dispatchers: LifecycleDispatchers::new(),
                }
            }),
            // Keep Steel's marker until this block gains a factory. Vanilla consumes it because
            // every Vanilla EntityBlock factory is implemented; retaining it prevents permanent
            // data loss for intentionally deferred block implementations.
            BlockEntityCreation::Unimplemented => self.with_locked_block_state(pos, |live_state| {
                if live_state == expected_state {
                    PendingPromotionCommit::Complete {
                        block_entity: None,
                        lifecycle_dispatchers: LifecycleDispatchers::new(),
                    }
                } else {
                    PendingPromotionCommit::Retry
                }
            }),
        }
    }

    /// Attempts every packed promotion after generation postprocessing.
    ///
    /// Intentional `Unimplemented` markers remain packed until Steel gains their block factory.
    pub(crate) fn promote_pending_block_entities(&self) {
        let positions = self.pending_block_entity_positions();
        for pos in positions {
            let _ = self.promote_pending_block_entity(pos);
        }
    }

    /// Returns packed block-entity positions without causing promotion.
    #[must_use]
    pub fn pending_block_entity_positions(&self) -> Vec<BlockPos> {
        self.chunk.block_entity_storage().pending_positions()
    }

    /// Retains a Vanilla `DUMMY` marker for lazy promotion if no concrete entity exists.
    pub fn set_pending_block_entity(&self, pos: BlockPos) {
        if ChunkPos::from_block_pos(pos) != self.chunk.pos {
            log::warn!(
                "Trying to set a pending block entity at {pos:?} in chunk {:?}",
                self.chunk.pos,
            );
            return;
        }
        if self.chunk.block_entity_storage().set_pending(pos) {
            self.mark_unsaved();
        }
    }

    /// Removes a block entity at the given position.
    ///
    /// Marks the entity as removed and unbinds its world ticker.
    #[must_use]
    pub fn remove_block_entity(&self, pos: BlockPos) -> bool {
        let (removed, lifecycle_dispatchers) = self.chunk.block_entity_storage().remove_staged(pos);
        self.finish_block_entity_change(pos, lifecycle_dispatchers);
        self.mark_unsaved();
        removed
    }

    /// Removes only the entity that still owns its position.
    pub(crate) fn remove_block_entity_if_same(&self, expected: &dyn BlockEntity) -> bool {
        let pos = expected.get_block_pos();
        let (removed, lifecycle_dispatchers) = self
            .chunk
            .block_entity_storage()
            .remove_if_same_staged(pos, expected);
        self.finish_block_entity_change(pos, lifecycle_dispatchers);
        if removed {
            self.mark_unsaved();
        }
        removed
    }

    /// Adds a block entity and reconciles its state-selected world ticker.
    ///
    /// Note: The world reference should be passed at block entity construction time.
    /// Returns false when the entity's position or type does not match the live state.
    #[must_use]
    pub fn add_and_register_block_entity(&self, block_entity: SharedBlockEntity) -> bool {
        let pos = block_entity.get_block_pos();
        let (valid, lifecycle_dispatchers) =
            self.add_and_register_block_entity_staged(block_entity);
        self.finish_block_entity_change(pos, lifecycle_dispatchers);
        valid
    }

    fn add_and_register_block_entity_staged(
        &self,
        block_entity: SharedBlockEntity,
    ) -> (bool, LifecycleDispatchers) {
        let pos = block_entity.get_block_pos();
        if ChunkPos::from_block_pos(pos) != self.chunk.pos {
            log::warn!(
                "Trying to set block entity {} at {pos:?} in chunk {:?}",
                block_entity.get_type().key,
                self.chunk.pos,
            );
            return (false, LifecycleDispatchers::new());
        }

        loop {
            let state = self.get_block_state(pos);
            let valid = state.has_block_entity() && block_entity.is_valid_block_state(state);
            if !valid {
                let state_unchanged =
                    self.with_locked_block_state(pos, |live_state| live_state == state);
                if !state_unchanged {
                    continue;
                }
                log::warn!(
                    "Trying to set block entity {} at {pos:?}, but block {} does not accept that type",
                    block_entity.get_type().key,
                    state.get_block().key,
                );
                return (false, LifecycleDispatchers::new());
            }

            let cached_state = block_entity.get_block_state();
            let committed = self.with_locked_block_state(pos, |live_state| {
                if live_state != state {
                    return None;
                }
                Some(
                    self.chunk
                        .block_entity_storage()
                        .add_staged(&block_entity, state),
                )
            });
            let Some((_, lifecycle_dispatchers)) = committed else {
                continue;
            };
            if state.get_block() != cached_state.get_block() {
                log::warn!(
                    "Updating mismatched block entity {} state at {pos:?}: {} != {}",
                    block_entity.get_type().key,
                    state.get_block().key,
                    cached_state.get_block().key,
                );
            }

            self.mark_unsaved();
            return (true, lifecycle_dispatchers);
        }
    }

    fn with_locked_block_state<R>(&self, pos: BlockPos, f: impl FnOnce(BlockStateId) -> R) -> R {
        let y = pos.y();
        if y < self.min_y() || y >= self.min_y() + self.height() {
            return f(REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR));
        }

        let section_index = self.get_section_index(y);
        if section_index >= self.chunk.sections.sections.len() {
            return f(REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR));
        }

        let section = self.chunk.sections.sections[section_index].read();
        let state = if section.is_empty() {
            REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR)
        } else {
            section.states.get(
                (pos.x() & 15) as usize,
                (y & 15) as usize,
                (pos.z() & 15) as usize,
            )
        };
        f(state)
    }

    fn reconcile_block_entity_after_set(&self, pos: BlockPos, state: BlockStateId) {
        loop {
            if self.get_block_state(pos) != state {
                return;
            }

            if let Some(block_entity) = self.get_block_entity(pos) {
                if block_entity.is_valid_block_state(state) {
                    let committed = self.with_locked_block_state(pos, |live_state| {
                        if live_state != state {
                            return None;
                        }
                        Some(self.chunk.block_entity_storage().update_if_same_staged(
                            pos,
                            &block_entity,
                            state,
                        ))
                    });
                    let Some((updated, lifecycle_dispatchers)) = committed else {
                        return;
                    };
                    self.finish_block_entity_change(pos, lifecycle_dispatchers);
                    if updated {
                        return;
                    }
                    continue;
                }

                let removed = self.with_locked_block_state(pos, |live_state| {
                    if live_state != state {
                        return None;
                    }
                    Some(
                        self.chunk
                            .block_entity_storage()
                            .remove_if_same_staged(pos, block_entity.as_ref()),
                    )
                });
                let Some((removed, lifecycle_dispatchers)) = removed else {
                    return;
                };
                self.finish_block_entity_change(pos, lifecycle_dispatchers);
                if !removed {
                    continue;
                }
                log::warn!(
                    "Removed mismatched block entity at {pos:?}: type = {}, state = {}",
                    block_entity.get_type().key,
                    state.get_block().key,
                );
                // Removal hooks may synchronously replace either the block or its entity.
                continue;
            }

            let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
            match behavior.new_block_entity(self.chunk.level_weak(), pos, state) {
                BlockEntityCreation::Created(block_entity) => {
                    let valid = block_entity.get_block_pos() == pos
                        && block_entity.is_valid_block_state(state);
                    let inserted = self.with_locked_block_state(pos, |live_state| {
                        if live_state != state {
                            return None;
                        }
                        if !valid {
                            return Some(None);
                        }
                        Some(Some(
                            self.chunk
                                .block_entity_storage()
                                .insert_if_absent_staged(&block_entity, state),
                        ))
                    });
                    let Some(inserted) = inserted else {
                        return;
                    };
                    let Some(inserted) = inserted else {
                        debug_assert!(false, "block-entity factory returned an invalid entity");
                        return;
                    };
                    if let BlockEntityInsert::Inserted(lifecycle_dispatchers) = inserted {
                        self.finish_block_entity_change(pos, lifecycle_dispatchers);
                        return;
                    }
                }
                BlockEntityCreation::NoEntity => return,
                BlockEntityCreation::Unimplemented => {
                    let inserted = self.with_locked_block_state(pos, |live_state| {
                        live_state == state && self.chunk.block_entity_storage().set_pending(pos)
                    });
                    if inserted {
                        self.mark_unsaved();
                    }
                    return;
                }
            }
        }
    }

    /// Returns all block entities in this chunk.
    #[must_use]
    pub fn get_block_entities(&self) -> Vec<SharedBlockEntity> {
        self.chunk.block_entity_storage().get_all()
    }

    /// Clears entity ownership while deferring lifecycle callbacks to an outer-lock-free caller.
    #[must_use]
    pub(crate) fn clear_all_block_entities_staged(&self) -> ClearedBlockEntities {
        self.chunk
            .block_entity_storage()
            .clear_and_stage_lifecycle_callbacks()
    }

    /// Sets a block state at the given position.
    ///
    /// Returns the old block state, or `None` if nothing changed.
    ///
    /// The world scheduler serializes gameplay mutations. This method makes the palette and
    /// block-entity ownership transition atomic, but its later Vanilla-ordered callbacks and
    /// derived-cache updates are not a general concurrent transaction for the same position.
    ///
    /// # Arguments
    /// * `pos` - The absolute block position
    /// * `state` - The new block state to set
    /// * `flags` - Update flags controlling behavior
    ///
    /// # Panics
    ///
    /// Panics if the behavior registry has not been initialized.
    #[must_use]
    pub fn set_block_state(
        &self,
        pos: BlockPos,
        state: BlockStateId,
        flags: UpdateFlags,
    ) -> Option<BlockStateId> {
        match self.set_block_state_inner(pos, None, state, flags)? {
            FullChunkBlockSetResult::Changed(old_state) => Some(old_state),
            FullChunkBlockSetResult::Unchanged | FullChunkBlockSetResult::Stale(_) => None,
        }
    }

    pub(crate) fn set_block_state_if_unchanged(
        &self,
        pos: BlockPos,
        expected_state: BlockStateId,
        new_state: BlockStateId,
        flags: UpdateFlags,
    ) -> Option<FullChunkBlockSetResult> {
        self.set_block_state_inner(pos, Some(expected_state), new_state, flags)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "block mutation keeps vanilla side effects in one ordered transaction"
    )]
    fn set_block_state_inner(
        &self,
        pos: BlockPos,
        expected_state: Option<BlockStateId>,
        state: BlockStateId,
        flags: UpdateFlags,
    ) -> Option<FullChunkBlockSetResult> {
        let y = pos.0.y;

        if y < self.min_y() || y >= self.min_y() + self.height() {
            return None;
        }

        let section_index = self.get_section_index(y);

        if section_index >= self.chunk.sections.sections.len() {
            return None;
        }

        let section = &self.chunk.sections.sections[section_index];

        let local_x = (pos.0.x & 15) as usize;
        let local_y = (y & 15) as usize;
        let local_z = (pos.0.z & 15) as usize;

        let mut keep_block_entity_decision = None;
        let (old_state, was_empty, is_empty, detached_block_entity) = loop {
            let mut section_guard = section.write();
            let observed_state = section_guard.states.get(local_x, local_y, local_z);
            if expected_state.is_some_and(|expected| observed_state != expected) {
                return Some(FullChunkBlockSetResult::Stale(observed_state));
            }
            if observed_state == state {
                return Some(FullChunkBlockSetResult::Unchanged);
            }

            // Behavior decisions run without section/storage locks. The following palette write
            // verifies the exact observed state before using the decision.
            let old_block = observed_state.get_block();
            let new_block = state.get_block();
            let block_changed = old_block != new_block;
            let detach_block_entity = if block_changed && observed_state.has_block_entity() {
                let Some((decision_state, should_keep)) = keep_block_entity_decision else {
                    drop(section_guard);
                    let should_keep = BLOCK_BEHAVIORS
                        .get_behavior(new_block)
                        .should_keep_block_entity(observed_state, state);
                    keep_block_entity_decision = Some((observed_state, should_keep));
                    continue;
                };
                if decision_state != observed_state {
                    drop(section_guard);
                    keep_block_entity_decision = None;
                    continue;
                }
                !should_keep
            } else {
                false
            };

            let was_empty = section_guard.is_empty();
            let old_state = section_guard.set_block_state(local_x, local_y, local_z, state);
            debug_assert_eq!(old_state, observed_state);
            let detached_block_entity = detach_block_entity.then(|| {
                self.chunk
                    .block_entity_storage()
                    .detach_and_queue_removal(pos)
            });
            let is_empty = section_guard.is_empty();
            break (old_state, was_empty, is_empty, detached_block_entity);
        };

        let min_y = self.min_y();
        let sections = &self.chunk.sections;
        self.chunk
            .heightmaps
            .write()
            .update_final(local_x, y, local_z, state, |lx, scan_y, lz| {
                let scan_section_index = ((scan_y - min_y) / 16) as usize;
                let scan_local_y = ((scan_y - min_y) % 16) as usize;
                sections.sections[scan_section_index]
                    .read()
                    .states
                    .get(lx, scan_local_y, lz)
            });

        let old_block = old_state.get_block();
        let new_block = state.get_block();

        let empty_section_change = if was_empty == is_empty {
            None
        } else {
            self.update_light_section_emptiness(y, is_empty);
            Some(LightSectionEmptinessChange {
                section_pos: SectionPos::new(
                    self.chunk.pos.0.x,
                    SectionPos::block_to_section_coord(y),
                    self.chunk.pos.0.y,
                ),
                empty: is_empty,
            })
        };

        let light_properties_changed = has_different_light_properties(old_state, state);
        if light_properties_changed {
            self.update_sky_light_sources(local_x, y, local_z);
        }

        let block_changed = old_block != new_block;
        let moved_by_piston = flags.contains(UpdateFlags::UPDATE_MOVE_BY_PISTON);
        let side_effects = !flags.contains(UpdateFlags::UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS);

        let block_behaviors = &*BLOCK_BEHAVIORS;
        let old_behavior = block_behaviors.get_behavior(old_block);
        let new_behavior = block_behaviors.get_behavior(new_block);

        let level = self.get_level();
        if let Some(level) = &level
            && (light_properties_changed || empty_section_change.is_some())
        {
            level.queue_light_change_after_block_set(pos, old_state, state, empty_section_change);
        }

        if let Some(DetachedBlockEntity {
            entity,
            dispatch_removed,
        }) = detached_block_entity
            && let Some(block_entity) = entity
        {
            // The exact old owner was detached with the palette write, so a concurrent/reentrant
            // replacement cannot be drained or removed by this operation. Unlike Vanilla's
            // single-threaded map, the entity is no longer discoverable during this callback.
            if side_effects && level.is_some() {
                block_entity.pre_remove_side_effects(pos, old_state);
            }
            let mut lifecycle_dispatchers = LifecycleDispatchers::new();
            if dispatch_removed {
                lifecycle_dispatchers.push(block_entity);
            }
            self.finish_block_entity_change(pos, lifecycle_dispatchers);
        }

        if let Some(level) = level {
            // Notify neighbors that we were removed (for rails, etc.)
            if (block_changed || new_behavior.is_rail())
                && (flags.contains(UpdateFlags::UPDATE_NEIGHBORS) || moved_by_piston)
            {
                old_behavior.affect_neighbors_after_removal(
                    old_state,
                    &level,
                    pos,
                    moved_by_piston,
                );
            }

            // Removal callbacks may synchronously replace this position. Vanilla does not run
            // placement callbacks for the stale request.
            let current_state = section.read().states.get(local_x, local_y, local_z);
            if current_state.get_block() != new_block {
                return Some(FullChunkBlockSetResult::Stale(current_state));
            }

            // Call on_place for the new block
            if !flags.contains(UpdateFlags::UPDATE_SKIP_ON_PLACE) {
                new_behavior.on_place(state, &level, pos, old_state, moved_by_piston);
            }
        }

        // Block-entity reconciliation is an exact-state transaction. Placement callbacks or
        // concurrent writers that replace this request own the resulting entity instead.
        if state.has_block_entity() {
            self.reconcile_block_entity_after_set(pos, state);
        }

        self.mark_unsaved();
        Some(FullChunkBlockSetResult::Changed(old_state))
    }

    fn update_light_section_emptiness(&self, y: i32, is_empty: bool) {
        let section_y = SectionPos::block_to_section_coord(y);
        self.chunk
            .light
            .write()
            .set_section_empty(section_y, is_empty);
    }

    fn update_sky_light_sources(&self, local_x: usize, y: i32, local_z: usize) {
        let chunk_min_x = self.chunk.pos.0.x * 16;
        let chunk_min_z = self.chunk.pos.0.y * 16;
        self.chunk.sky_light_sources.write().update(
            local_x,
            y,
            local_z,
            |scan_x, scan_y, scan_z| {
                self.get_block_state(BlockPos::new(
                    chunk_min_x + scan_x as i32,
                    scan_y,
                    chunk_min_z + scan_z as i32,
                ))
            },
        );
    }

    pub(crate) fn refresh_light_emptiness_maps(&self) {
        if let Err(error) = self
            .chunk
            .light
            .write()
            .refresh_emptiness_maps_from_sections(&self.chunk.sections)
        {
            panic!("invalid chunk light emptiness map length: {error:?}");
        }
    }

    /// Gets a block state at the given position.
    #[must_use]
    pub fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
        let y = pos.0.y;
        if y < self.min_y() || y >= self.min_y() + self.height() {
            return REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR);
        }

        let section_index = self.get_section_index(y);

        // `LevelChunk` returns air outside its section array; `World` handles void air.
        if section_index >= self.chunk.sections.sections.len() {
            return REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR);
        }

        let section = &self.chunk.sections.sections[section_index];
        let section_guard = section.read();

        if section_guard.is_empty() {
            return REGISTRY.blocks.get_base_state_id(&vanilla_blocks::AIR);
        }

        let local_x = (pos.0.x & 15) as usize;
        let local_y = (y & 15) as usize;
        let local_z = (pos.0.z & 15) as usize;

        section_guard.states.get(local_x, local_y, local_z)
    }

    /// Mirrors vanilla `ChunkAccess.getHighestFilledSectionIndex`.
    #[must_use]
    pub fn highest_filled_section_index(&self) -> Option<usize> {
        self.chunk
            .sections
            .sections
            .iter()
            .rposition(|section| !section.read().is_empty())
    }

    /// Mirrors vanilla `ChunkAccess.getHighestSectionPosition`.
    #[must_use]
    pub fn highest_section_position(&self) -> i32 {
        self.highest_filled_section_index()
            .map_or(self.min_y(), |index| self.min_y() + index as i32 * 16)
    }

    /// Extracts the chunk data for sending to the client.
    #[must_use]
    pub fn extract_chunk_data(&self) -> ChunkPacketData {
        let data = Vec::new();

        let mut cursor = Cursor::new(data);
        self.chunk.sections.sections.iter().for_each(|section| {
            section.read().write(&mut cursor);
        });

        let heightmaps = {
            let heightmaps = self.chunk.heightmaps.read();
            vec![
                (
                    ProtocolHeightmapType::WorldSurface,
                    heightmaps
                        .get_final(HeightmapType::WorldSurface)
                        .get_raw_data(),
                ),
                (
                    ProtocolHeightmapType::MotionBlocking,
                    heightmaps
                        .get_final(HeightmapType::MotionBlocking)
                        .get_raw_data(),
                ),
                (
                    ProtocolHeightmapType::MotionBlockingNoLeaves,
                    heightmaps
                        .get_final(HeightmapType::MotionBlockingNoLeaves)
                        .get_raw_data(),
                ),
            ]
        };

        // Collect block entity data for client sync
        let block_entities: Vec<BlockEntityInfo> = self
            .chunk
            .block_entity_storage()
            .get_all()
            .iter()
            .map(|entity| {
                let pos = entity.get_block_pos();
                let type_id = entity.get_type().id() as i32;
                let update_tag = entity.get_update_tag();

                BlockEntityInfo {
                    packed_xz: PackedChunkLocalXZ::from_block_pos(pos),
                    y: pos.0.y as i16,
                    type_id,
                    data: update_tag.into(),
                }
            })
            .collect();

        ChunkPacketData {
            heightmaps: Heightmaps { heightmaps },
            data: cursor.into_inner(),
            block_entities,
        }
    }

    /// Extracts the light data for sending to the client.
    #[must_use]
    pub fn extract_light_data(&self, has_skylight: bool) -> LightUpdatePacketData {
        let light = self.chunk.light.read();
        build_chunk_light_update_packet(&light, has_skylight)
    }
}

#[cfg(test)]
mod game_event_tests;

#[cfg(test)]
mod tests;
