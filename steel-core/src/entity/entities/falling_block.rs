//! Falling block entity implementation.
//!
//! `FallingBlockEntity` is the physical entity spawned when a block with gravity
//! (sand, gravel, anvil, concrete powder) detects that the block below it is free.
//! It falls with gravity, handles landing logic (place block or drop item), and
//! optionally damages entities on impact (anvil).

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Weak};

use crossbeam::atomic::AtomicCell;
use glam::DVec3;
use simdnbt::borrow::BaseNbtCompound as BorrowedNbtCompound;
use simdnbt::borrow::NbtCompound as NbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::blocks::shapes::AABBd;
use steel_registry::entity_data::DataValue;
use steel_registry::entity_types::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_tags;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::FallingBlockEntityData;
use steel_registry::vanilla_game_rules::ENTITY_DROPS;
use steel_registry::{REGISTRY, RegistryExt, TaggedRegistryExt};
use steel_utils::locks::SyncMutex;
use steel_utils::types::{GameType, UpdateFlags};
use steel_utils::{BlockPos, BlockStateId};
use uuid::Uuid;

use crate::behavior::BLOCK_BEHAVIORS;
use crate::entity::{Entity, EntityBase, RemovalReason};
use crate::fluid::state::{fluid_state_to_block, get_fluid_state_from_block};
use crate::physics::MoverType;
use crate::world::RaytraceAction;
use crate::world::World;
use steel_protocol::packets::game::{
    CBlockUpdate, CEntityPositionSync, CMoveEntityPos, CSetEntityMotion, calc_delta,
};

/// Gravity applied per tick (blocks/tick²). Vanilla: `FallingBlockEntity.getDefaultGravity()`
const DEFAULT_GRAVITY: f64 = 0.04;

/// Ticks before an out-of-bounds falling block is discarded.
const TIME_BEFORE_OOB_DESPAWN: i32 = 100;

/// Absolute tick limit before any falling block is discarded.
const MAX_LIFETIME: i32 = 600;

/// A block entity that falls due to gravity.
///
/// Spawned by `FallingBlock.tick()` when the block below is free. Falls until it
/// lands on a solid surface, then places the block or drops it as an item.
pub struct FallingBlockEntity {
    base: EntityBase,

    // Physics
    velocity: SyncMutex<DVec3>,
    on_ground: AtomicBool,

    // Network sync
    entity_data: SyncMutex<FallingBlockEntityData>,

    // Block data
    /// The block state being carried by this entity.
    block_state: AtomicCell<BlockStateId>,

    // Timers / flags
    /// Ticks this entity has been alive. Vanilla: `FallingBlockEntity.time`.
    time: AtomicI32,
    /// Whether to drop the block as an item when it can't land. Vanilla: `dropItem`.
    drop_item: AtomicBool,
    /// Set to true when the anvil breaks on landing to cancel the item drop.
    cancel_drop: AtomicBool,
    /// Whether this entity damages living entities on impact. Vanilla: `hurtEntities`.
    hurt_entities: AtomicBool,
    /// Maximum impact damage cap. Vanilla: `fallDamageMax`.
    fall_damage_max: AtomicI32,
    /// Damage per block fallen. Vanilla: `fallDamagePerDistance`.
    fall_damage_per_distance: AtomicCell<f32>,
    /// Total distance fallen in blocks, accumulated across ticks. Vanilla: `Entity.fallDistance`.
    fall_distance: AtomicCell<f64>,
    /// Optional NBT for a block entity carried by this falling block (e.g. a chest).
    block_data: SyncMutex<Option<NbtCompound>>,

    // Network sync tracking
    last_sent_velocity: SyncMutex<DVec3>,
    last_sent_position: SyncMutex<DVec3>,
    last_sent_on_ground: AtomicBool,
    needs_sync: AtomicBool,
    /// Internal tick counter (always increments, unlike `time` which can stop).
    tick_count: AtomicI32,
}

impl FallingBlockEntity {
    /// Creates a new falling block entity with the given block state.
    #[must_use]
    pub fn new(id: i32, position: DVec3, world: Weak<World>, block_state: BlockStateId) -> Self {
        let mut entity_data = FallingBlockEntityData::new();
        entity_data
            .start_pos
            .set(steel_registry::entity_data::BlockPos::new(
                position.x as i32,
                position.y as i32,
                position.z as i32,
            ));

        Self {
            base: EntityBase::new(id, position, world),
            velocity: SyncMutex::new(DVec3::ZERO),
            on_ground: AtomicBool::new(false),
            entity_data: SyncMutex::new(entity_data),
            block_state: AtomicCell::new(block_state),
            time: AtomicI32::new(0),
            drop_item: AtomicBool::new(true),
            cancel_drop: AtomicBool::new(false),
            hurt_entities: AtomicBool::new(false),
            fall_damage_max: AtomicI32::new(40),
            fall_damage_per_distance: AtomicCell::new(0.0),
            fall_distance: AtomicCell::new(0.0),
            block_data: SyncMutex::new(None),
            last_sent_velocity: SyncMutex::new(DVec3::ZERO),
            last_sent_position: SyncMutex::new(position),
            last_sent_on_ground: AtomicBool::new(false),
            needs_sync: AtomicBool::new(false),
            tick_count: AtomicI32::new(0),
        }
    }

    /// Creates a falling block entity from saved data.
    ///
    /// Used when loading entities from disk. Type-specific data is restored via
    /// `load_additional()` after this constructor.
    #[must_use]
    pub fn from_saved(
        id: i32,
        position: DVec3,
        uuid: Uuid,
        velocity: DVec3,
        _rotation: (f32, f32),
        on_ground: bool,
        world: Weak<World>,
    ) -> Self {
        use steel_registry::vanilla_blocks;
        let block_state = vanilla_blocks::SAND.default_state();

        Self {
            base: EntityBase::with_uuid(id, uuid, position, world),
            velocity: SyncMutex::new(velocity),
            on_ground: AtomicBool::new(on_ground),
            entity_data: SyncMutex::new(FallingBlockEntityData::new()),
            block_state: AtomicCell::new(block_state),
            time: AtomicI32::new(0),
            drop_item: AtomicBool::new(true),
            cancel_drop: AtomicBool::new(false),
            hurt_entities: AtomicBool::new(false),
            fall_damage_max: AtomicI32::new(40),
            fall_damage_per_distance: AtomicCell::new(0.0),
            fall_distance: AtomicCell::new(0.0),
            block_data: SyncMutex::new(None),
            last_sent_velocity: SyncMutex::new(velocity),
            last_sent_position: SyncMutex::new(position),
            last_sent_on_ground: AtomicBool::new(on_ground),
            needs_sync: AtomicBool::new(false),
            tick_count: AtomicI32::new(0),
        }
    }

    /// Spawns a `FallingBlockEntity` for the given block position and state.
    ///
    /// Strips WATERLOGGED from the state, replaces the block with its fluid state
    /// (water source or air), and returns the entity ready to be added to the world.
    ///
    /// Vanilla: `FallingBlockEntity.fall(Level, BlockPos, BlockState)`.
    pub fn fall(id: i32, world: &Arc<World>, pos: BlockPos, state: BlockStateId) -> Self {
        // Strip WATERLOGGED — the entity itself is never waterlogged
        let falling_state = state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .map(|_| state.set_value(&BlockStateProperties::WATERLOGGED, false))
            .unwrap_or(state);

        // Replace the block with its fluid state (air, or water source if waterlogged)
        let fluid_replacement = fluid_state_to_block(get_fluid_state_from_block(state));
        world.set_block(pos, fluid_replacement, UpdateFlags::UPDATE_ALL);

        // Spawn at block center X/Z, bottom Y (vanilla uses pos.getX() + 0.5, pos.getY())
        let spawn_pos = DVec3::new(pos.x() as f64 + 0.5, pos.y() as f64, pos.z() as f64 + 0.5);

        let entity = Self::new(id, spawn_pos, Arc::downgrade(world), falling_state);
        entity
            .entity_data
            .lock()
            .start_pos
            .set(steel_registry::entity_data::BlockPos::new(pos.x(), pos.y(), pos.z()));
        entity
    }

    /// Gets the current block state being carried.
    pub fn block_state(&self) -> BlockStateId {
        self.block_state.load()
    }

    /// Enables entity damage on impact.
    ///
    /// Called by blocks like anvils via `BlockBehavior::falling_entity_config()`.
    /// Vanilla: `FallingBlockEntity.setHurtsEntities(float, int)`.
    pub fn set_hurts_entities(&self, damage_per_distance: f32, damage_max: i32) {
        self.hurt_entities.store(true, Ordering::Relaxed);
        self.fall_damage_per_distance.store(damage_per_distance);
        self.fall_damage_max.store(damage_max, Ordering::Relaxed);
    }

    /// Prevents the block from being dropped as an item on impact.
    pub fn disable_drop(&self) {
        self.cancel_drop.store(true, Ordering::Relaxed);
    }

    // === Private helpers ===

    fn entity_drops_enabled(&self, world: &Arc<World>) -> bool {
        world
            .get_game_rule(&ENTITY_DROPS)
            .as_bool()
            .unwrap_or(true)
    }

    /// Applies impact damage to entities in the bounding box.
    ///
    /// Also handles anvil degradation on impact.
    /// Vanilla: `FallingBlockEntity.causeFallDamage()`.
    fn cause_fall_damage(&self, fall_distance: f64, world: &Arc<World>) {
        if !self.hurt_entities.load(Ordering::Relaxed) {
            return;
        }

        let fall_dist_int = (fall_distance - 1.0).ceil() as i32;
        if fall_dist_int < 0 {
            return;
        }

        let per_dist = self.fall_damage_per_distance.load();
        let damage_max = self.fall_damage_max.load(Ordering::Relaxed);
        let damage = ((fall_dist_int as f32 * per_dist).floor() as i32).min(damage_max) as f32;

        let block_state = self.block_state.load();
        let block_ref = block_state.get_block();

        let source = BLOCK_BEHAVIORS
            .get_behavior(block_ref)
            .fall_damage_source(self.base.id());

        for entity in world.get_entities_in_aabb(&self.bounding_box()) {
            if entity.id() == self.base.id() {
                continue;
            }
            // Vanilla: EntitySelector.NO_CREATIVE_OR_SPECTATOR
            if let Some(player) = entity.clone().as_player() {
                let gm = player.game_mode.load();
                if gm == GameType::Creative || gm == GameType::Spectator {
                    continue;
                }
            }
            // TODO: EntitySelector.LIVING_ENTITY_STILL_ALIVE — requires is_living() + health check
            entity.hurt(&source, damage);
        }

        // Anvil degradation: chance to degrade state on impact
        if damage > 0.0
            && REGISTRY
                .blocks
                .is_in_tag(block_ref, &vanilla_block_tags::ANVIL_TAG)
        {
            let degrade_chance = 0.05 + fall_dist_int as f32 * 0.05;
            if rand::random::<f32>() < degrade_chance {
                match degrade_anvil(block_state) {
                    Some(new_state) => self.block_state.store(new_state),
                    None => self.cancel_drop.store(true, Ordering::Relaxed),
                }
            }
        }
    }

    fn try_drop_block_item(&self, world: &Arc<World>) {
        if !self.drop_item.load(Ordering::Relaxed) || !self.entity_drops_enabled(world) {
            return;
        }
        let block_state = self.block_state.load();
        let block_ref = block_state.get_block();
        if let Some(item_ref) = REGISTRY.items.by_key(&block_ref.key) {
            let _ = self.spawn_at_location(ItemStack::new(item_ref), 0.0);
        }
    }

    fn call_on_broken_after_fall(&self, world: &Arc<World>, pos: BlockPos) {
        let block_ref = self.block_state.load().get_block();
        BLOCK_BEHAVIORS
            .get_behavior(block_ref)
            .on_broken_after_fall(world, pos, self);
    }

    /// Checks velocity sync and returns a packet if needed.
    fn check_velocity_sync(&self) -> Option<CSetEntityMotion> {
        let current = *self.velocity.lock();
        let last_sent = *self.last_sent_velocity.lock();

        let diff_sq = (current.x - last_sent.x).powi(2)
            + (current.y - last_sent.y).powi(2)
            + (current.z - last_sent.z).powi(2);

        let should_sync = diff_sq > 1.0e-7
            || (diff_sq > 0.0 && current.x == 0.0 && current.y == 0.0 && current.z == 0.0);

        if should_sync {
            *self.last_sent_velocity.lock() = current;
            Some(CSetEntityMotion::new(self.id(), current.x, current.y, current.z))
        } else {
            None
        }
    }

    /// Checks position sync and returns the appropriate packet if needed.
    fn check_position_sync(&self, tick_count: i32) -> Option<PositionSyncPacket> {
        let current_pos = self.position();
        let last_sent = *self.last_sent_position.lock();
        let current_on_ground = self.on_ground();
        let last_on_ground = self.last_sent_on_ground.load(Ordering::Relaxed);

        let diff_sq = (current_pos.x - last_sent.x).powi(2)
            + (current_pos.y - last_sent.y).powi(2)
            + (current_pos.z - last_sent.z).powi(2);

        let position_changed = diff_sq >= 7.629_394_5e-6;
        let on_ground_changed = current_on_ground != last_on_ground;
        let force_periodic_sync = tick_count % 60 == 0;

        if !position_changed && !on_ground_changed && !force_periodic_sync {
            return None;
        }

        let dx = calc_delta(current_pos.x, last_sent.x);
        let dy = calc_delta(current_pos.y, last_sent.y);
        let dz = calc_delta(current_pos.z, last_sent.z);

        let use_full_sync =
            on_ground_changed || force_periodic_sync || dx.is_none() || dy.is_none() || dz.is_none();

        self.last_sent_on_ground
            .store(current_on_ground, Ordering::Relaxed);
        *self.last_sent_position.lock() = current_pos;

        if use_full_sync {
            let vel = *self.velocity.lock();
            Some(PositionSyncPacket::Full(CEntityPositionSync {
                entity_id: self.id(),
                x: current_pos.x,
                y: current_pos.y,
                z: current_pos.z,
                velocity_x: vel.x,
                velocity_y: vel.y,
                velocity_z: vel.z,
                yaw: 0.0,
                pitch: 0.0,
                on_ground: current_on_ground,
            }))
        } else {
            Some(PositionSyncPacket::Delta(CMoveEntityPos {
                entity_id: self.id(),
                dx: dx.unwrap(),
                dy: dy.unwrap(),
                dz: dz.unwrap(),
                on_ground: current_on_ground,
            }))
        }
    }
}

enum PositionSyncPacket {
    Delta(CMoveEntityPos),
    Full(CEntityPositionSync),
}

/// Degrades an anvil state one step: ANVIL → CHIPPED → DAMAGED → None (destroyed).
///
/// Returns `None` when the anvil would be destroyed.
/// Vanilla: `AnvilBlock.damage(BlockState)`.
fn degrade_anvil(state: BlockStateId) -> Option<BlockStateId> {
    use steel_registry::vanilla_blocks;
    let block = state.get_block();
    let facing = state.try_get_value(&BlockStateProperties::HORIZONTAL_FACING);
    let with_facing = |new_state: BlockStateId| match facing {
        Some(f) => new_state.set_value(&BlockStateProperties::HORIZONTAL_FACING, f),
        None => new_state,
    };
    if block == &vanilla_blocks::ANVIL {
        Some(with_facing(vanilla_blocks::CHIPPED_ANVIL.default_state()))
    } else if block == &vanilla_blocks::CHIPPED_ANVIL {
        Some(with_facing(vanilla_blocks::DAMAGED_ANVIL.default_state()))
    } else {
        None
    }
}

/// Returns true if a block state can be replaced by a falling block landing on it.
///
/// Vanilla: `FallingBlock.isFree(BlockState)`.
pub fn is_free(state: BlockStateId) -> bool {
    state.is_air()
        || REGISTRY
            .blocks
            .is_in_tag(state.get_block(), &vanilla_block_tags::FIRE_TAG)
        || state.get_block().config.liquid
        || state.is_replaceable()
}

impl Entity for FallingBlockEntity {
    fn base(&self) -> Option<&EntityBase> {
        Some(&self.base)
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::FALLING_BLOCK
    }

    fn bounding_box(&self) -> AABBd {
        let pos = self.position();
        let dims = self.entity_type().dimensions;
        let half_width = f64::from(dims.width) / 2.0;
        let height = f64::from(dims.height);
        AABBd {
            min_x: pos.x - half_width,
            min_y: pos.y,
            min_z: pos.z - half_width,
            max_x: pos.x + half_width,
            max_y: pos.y + height,
            max_z: pos.z + half_width,
        }
    }

    fn velocity(&self) -> DVec3 {
        *self.velocity.lock()
    }

    fn set_velocity(&self, velocity: DVec3) {
        *self.velocity.lock() = velocity;
    }

    fn on_ground(&self) -> bool {
        self.on_ground.load(Ordering::Relaxed)
    }

    fn set_on_ground(&self, on_ground: bool) {
        self.on_ground.store(on_ground, Ordering::Relaxed);
    }

    fn get_default_gravity(&self) -> f64 {
        DEFAULT_GRAVITY
    }

    fn blocks_building(&self) -> bool {
        true
    }

    fn spawn_data(&self) -> i32 {
        self.block_state.load().0 as i32
    }

    fn pack_dirty_entity_data(&self) -> Option<Vec<DataValue>> {
        self.entity_data.lock().pack_dirty()
    }

    fn pack_all_entity_data(&self) -> Vec<DataValue> {
        self.entity_data.lock().pack_all()
    }

    fn tick(&self) {
        let block_state = self.block_state.load();
        if block_state.is_air() {
            self.set_removed(RemovalReason::Discarded);
            return;
        }

        let tick_count = self.tick_count.fetch_add(1, Ordering::Relaxed) + 1;
        self.time.fetch_add(1, Ordering::Relaxed);

        // Store pre-move state for concrete powder detection and needs_sync tracking
        let prev_on_ground = self.on_ground();
        let prev_pos = self.position();
        let prev_vel = *self.velocity.lock();

        self.apply_gravity();

        // Capture velocity after gravity but before collision — mirrors vanilla's `vec3` in
        // Entity.move(), which is what accumulates into `fallDistance`.
        let vel_for_fall = *self.velocity.lock();

        let Some(_move_result) = self.do_move(MoverType::SelfMovement) else {
            return;
        };

        // TODO: portals
        // TODO: applyEffectsFromBlocks() — requires movement tracking on EntityBase,
        //       BlockBehavior::entity_inside()/step_on(), and AABB block iteration along path

        let Some(world) = self.level() else {
            return;
        };

        let time = self.time.load(Ordering::Relaxed);
        let cur = self.position();
        let pos = BlockPos::containing(cur.x, cur.y, cur.z);
        let block_ref = block_state.get_block();

        // Concrete powder: if moving fast enough, raycast to detect water traversal.
        // Vanilla: checks if concrete powder passed through a water source in one tick.
        let is_concrete_powder = block_ref.key.path.ends_with("_concrete_powder");

        let mut is_stuck_in_water = false;
        if is_concrete_powder {
            let cur_pos = self.position();
            let fluid = crate::fluid::state::get_fluid_state(&world, pos);
            is_stuck_in_water = is_water_source_fluid(fluid);

            // High-speed water detection via raycast (vanilla: ClipContext.Fluid.SOURCE_ONLY)
            if !is_stuck_in_water && prev_vel.length_squared() > 1.0 {
                let (hit_pos, _) = world.raytrace(prev_pos, cur_pos, |check_pos, w| {
                    let state = w.get_block_state(check_pos);
                    if is_water_source_block(state) {
                        RaytraceAction::ImmediateHit
                    } else {
                        RaytraceAction::Pass
                    }
                });
                if let Some(water_pos) = hit_pos {
                    let fluid_at_hit =
                        crate::fluid::state::get_fluid_state(&world, water_pos);
                    if is_water_source_fluid(fluid_at_hit) {
                        is_stuck_in_water = true;
                    }
                }
            }
        }

        let on_ground = self.on_ground();

        // Update accumulated fall distance (mirrors Entity.move() internals in vanilla).
        // Uses vel_for_fall (after gravity, before collision) matching vanilla's `vec3.y`.
        let current_fall_distance = self.fall_distance.load();
        if on_ground || is_stuck_in_water {
            self.fall_distance.store(0.0);
        } else if vel_for_fall.y < 0.0 {
            self.fall_distance.store(current_fall_distance + (-vel_for_fall.y));
        }

        if !on_ground && !is_stuck_in_water {
            // Airborne: check despawn conditions
            let out_of_bounds = pos.y() <= world.get_min_y() || pos.y() > world.get_max_y();
            if (time > TIME_BEFORE_OOB_DESPAWN && out_of_bounds) || time > MAX_LIFETIME {
                self.try_drop_block_item(&world);
                self.set_removed(RemovalReason::Discarded);
            }
        } else {
            // Landed (or concrete powder touching water): apply fall damage on first landing frame.
            // Uses the accumulated fall_distance, matching vanilla's Entity.causeFallDamage().
            if !prev_on_ground {
                self.cause_fall_damage(current_fall_distance, &world);
            }

            // Bounce damping (vanilla: multiply by (0.7, -0.5, 0.7))
            {
                let mut vel = self.velocity.lock();
                vel.x *= 0.7;
                vel.y *= -0.5;
                vel.z *= 0.7;
            }

            let current_state = world.get_block_state(pos);

            // Skip landing logic if we're on a moving piston
            use steel_registry::vanilla_blocks;
            if current_state.get_block() != &vanilla_blocks::MOVING_PISTON {
                if !self.cancel_drop.load(Ordering::Relaxed) {
                    // TODO: should use canBeReplaced(DirectionalPlaceContext(DOWN, EMPTY, UP)) —
                    //       in practice equivalent for most blocks since the item is always empty
                    let may_replace = current_state.is_replaceable();
                    let below_state = world.get_block_state(pos.below());
                    let would_continue = is_free(below_state) && !(is_concrete_powder && is_stuck_in_water);
                    let would_survive = BLOCK_BEHAVIORS
                        .get_behavior(block_state.get_block())
                        .can_survive(block_state, &world, pos)
                        && !would_continue;

                    if may_replace && would_survive {
                        // Restore WATERLOGGED if landing in water
                        let place_state =
                            if block_state.try_get_value(&BlockStateProperties::WATERLOGGED).is_some()
                                && crate::fluid::state::get_fluid_state(&world, pos)
                                    .fluid_id
                                    == crate::fluid::water_id()
                            {
                                block_state.set_value(&BlockStateProperties::WATERLOGGED, true)
                            } else {
                                block_state
                            };

                        if world.set_block(pos, place_state, UpdateFlags::UPDATE_ALL) {
                            // Send immediate block update to entity-tracking players so the
                            // client sees the block placed in the same frame as the entity
                            // removal. Vanilla: sendToTrackingPlayers(ClientboundBlockUpdatePacket)
                            let chunk_pos = steel_utils::ChunkPos::new(
                                pos.x() >> 4,
                                pos.z() >> 4,
                            );
                            world.broadcast_to_nearby(
                                chunk_pos,
                                CBlockUpdate { pos, block_state: world.get_block_state(pos) },
                                None,
                            );
                            self.set_removed(RemovalReason::Killed);
                            let behavior = BLOCK_BEHAVIORS.get_behavior(block_ref);
                            behavior.on_land(&world, pos, place_state, current_state, self);
                            // TODO: load block_data into block entity when has_block_entity() works
                        } else if self.drop_item.load(Ordering::Relaxed) && self.entity_drops_enabled(&world) {
                            self.call_on_broken_after_fall(&world, pos);
                            self.try_drop_block_item(&world);
                            self.set_removed(RemovalReason::Killed);
                        } else {
                            self.set_removed(RemovalReason::Killed);
                        }
                    } else {
                        self.set_removed(RemovalReason::Killed);
                        if self.drop_item.load(Ordering::Relaxed) && self.entity_drops_enabled(&world) {
                            self.call_on_broken_after_fall(&world, pos);
                            self.try_drop_block_item(&world);
                        }
                    }
                } else {
                    self.call_on_broken_after_fall(&world, pos);
                    self.set_removed(RemovalReason::Killed);
                }
            }
        }

        // Horizontal drag applied every tick regardless of ground state
        // Vanilla: getDeltaMovement().scale(0.98) at end of tick
        {
            let mut vel = self.velocity.lock();
            *vel *= 0.98;
        }

        // Track velocity changes for sync
        let new_vel = *self.velocity.lock();
        let diff = new_vel - prev_vel;
        if diff.length_squared() > 0.01 {
            self.needs_sync.store(true, Ordering::Relaxed);
        }
        if on_ground != prev_on_ground {
            self.needs_sync.store(true, Ordering::Relaxed);
        }

        let _ = tick_count;
    }

    fn send_changes(&self, tick_count: i32) {
        let Some(world) = self.level() else { return };

        let update_interval = self.entity_type().update_interval;
        let needs_sync = self.needs_sync.load(Ordering::Relaxed);

        if tick_count % update_interval != 0 && !needs_sync {
            return;
        }

        let pos = self.position();
        let chunk_pos = steel_utils::ChunkPos::new((pos.x as i32) >> 4, (pos.z as i32) >> 4);

        if let Some(vel_packet) = self.check_velocity_sync() {
            world.broadcast_to_nearby(chunk_pos, vel_packet, None);
        }

        if let Some(packet) = self.check_position_sync(tick_count) {
            match packet {
                PositionSyncPacket::Delta(p) => {
                    world.broadcast_to_nearby(chunk_pos, p, None);
                }
                PositionSyncPacket::Full(p) => {
                    world.broadcast_to_nearby(chunk_pos, p, None);
                }
            }
        }

        self.needs_sync.store(false, Ordering::Relaxed);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let block_state = self.block_state.load();
        // Block state: store as numeric ID (vanilla uses codec; we store raw ID for now)
        // TODO: use proper BlockState codec when available
        nbt.insert("BlockStateId", NbtTag::Int(block_state.0 as i32));
        nbt.insert("Time", NbtTag::Int(self.time.load(Ordering::Relaxed)));
        nbt.insert(
            "DropItem",
            NbtTag::Byte(self.drop_item.load(Ordering::Relaxed) as i8),
        );
        nbt.insert(
            "HurtEntities",
            NbtTag::Byte(self.hurt_entities.load(Ordering::Relaxed) as i8),
        );
        nbt.insert(
            "FallHurtAmount",
            NbtTag::Float(self.fall_damage_per_distance.load()),
        );
        nbt.insert(
            "FallHurtMax",
            NbtTag::Int(self.fall_damage_max.load(Ordering::Relaxed)),
        );
        if let Some(ref data) = *self.block_data.lock() {
            nbt.insert("TileEntityData", NbtTag::Compound(data.clone()));
        }
        nbt.insert(
            "CancelDrop",
            NbtTag::Byte(self.cancel_drop.load(Ordering::Relaxed) as i8),
        );
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt: NbtCompoundView<'_, '_> = nbt.into();

        if let Some(id) = nbt.int("BlockStateId") {
            self.block_state.store(BlockStateId(id as u16));
        }

        if let Some(time) = nbt.int("Time") {
            self.time.store(time, Ordering::Relaxed);
        }

        if let Some(drop_item) = nbt.byte("DropItem") {
            self.drop_item.store(drop_item != 0, Ordering::Relaxed);
        }

        let default_hurt = REGISTRY
            .blocks
            .is_in_tag(self.block_state.load().get_block(), &vanilla_block_tags::ANVIL_TAG);
        let hurt = nbt.byte("HurtEntities").map_or(default_hurt, |b| b != 0);
        self.hurt_entities.store(hurt, Ordering::Relaxed);

        if let Some(amount) = nbt.float("FallHurtAmount") {
            self.fall_damage_per_distance.store(amount);
        }

        if let Some(max) = nbt.int("FallHurtMax") {
            self.fall_damage_max.store(max, Ordering::Relaxed);
        }

        if let Some(cancel) = nbt.byte("CancelDrop") {
            self.cancel_drop.store(cancel != 0, Ordering::Relaxed);
        }

        if let Some(data) = nbt.compound("TileEntityData") {
            *self.block_data.lock() = Some(data.to_owned());
        }
    }
}

/// Returns true if this fluid state is a water source.
fn is_water_source_fluid(fluid: crate::fluid::FluidState) -> bool {
    fluid.fluid_id == crate::fluid::water_id() && fluid.is_source()
}

/// Returns true if this block state is a water source block or waterlogged.
fn is_water_source_block(state: BlockStateId) -> bool {
    use steel_registry::vanilla_blocks;
    if state.get_block() == &vanilla_blocks::WATER {
        return true;
    }
    matches!(
        state.try_get_value(&BlockStateProperties::WATERLOGGED),
        Some(true)
    )
}
