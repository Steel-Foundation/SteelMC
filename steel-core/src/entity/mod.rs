//! This module contains entity-related traits and types.

use std::sync::{Arc, LazyLock, Weak};

use glam::DVec3;
use rustc_hash::FxHashSet;
use simdnbt::borrow::BaseNbtCompound;
use simdnbt::owned::NbtCompound;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_data::DataValue;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_attributes;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_entity_type_tags::EntityTypeTag;
use steel_registry::{REGISTRY, TaggedRegistryExt, vanilla_game_events};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, BlockStateId, WorldAabb, axis::Axis};
use uuid::Uuid;

use crate::behavior::{BLOCK_BEHAVIORS, EntityFallOnContext, EntityLandingContext};
use crate::entity::attribute::AttributeMap;
use crate::physics::{
    EntityPhysicsState, MoveResult, MoverType, WorldCollisionProvider,
    move_entity as resolve_entity_movement,
};
use crate::world::World;
use crate::world::game_event_context::GameEventContext;
use crate::{entity::damage::DamageSource, player::Player};

use entities::ItemEntity;

/// Global counter for allocating unique entity IDs.
///
/// Mirrors vanilla's `Entity.ENTITY_COUNTER`. Each new entity increments this
/// counter to get a unique network ID. Starts at 1 (0 is reserved).
static ENTITY_COUNTER: LazyLock<SyncMutex<i32>> = LazyLock::new(|| SyncMutex::new(1));
const MOVEMENT_RECORD_EPSILON: f64 = 1.0e-7;

enum BlockEffectSegmentResult {
    Complete(i32),
    IterationLimit,
    Removed,
}

/// Allocates a new unique entity ID.
///
/// This is the primary way to get entity IDs for spawning entities.
/// Thread-safe through the shared counter lock.
#[must_use]
pub fn next_entity_id() -> i32 {
    let mut counter = ENTITY_COUNTER.lock();
    let id = *counter;
    *counter = counter.wrapping_add(1);
    id
}

fn apply_block_effect_segment(
    entity: &dyn Entity,
    world: &Arc<World>,
    from: DVec3,
    to: DVec3,
    max_iterations: i32,
    visited_blocks: &mut FxHashSet<BlockPos>,
) -> BlockEffectSegmentResult {
    let aabb = entity.make_bounding_box_at(to).deflate(1.0E-5);
    if aabb.is_empty() {
        return BlockEffectSegmentResult::Complete(0);
    }

    let mut hit_iteration_limit = false;
    let Some(iterations) =
        block_effects::for_each_block_intersected_between(from, to, aabb, |pos, iteration| {
            if entity.is_removed() {
                return false;
            }
            if iteration >= max_iterations {
                hit_iteration_limit = true;
                return false;
            }

            let state = world.get_block_state(pos);
            if state.is_air() || !visited_blocks.insert(pos) {
                return true;
            }

            let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
            behavior.entity_inside(state, world, pos, entity);
            !entity.is_removed()
        })
    else {
        if entity.is_removed() {
            return BlockEffectSegmentResult::Removed;
        }
        return if hit_iteration_limit {
            BlockEffectSegmentResult::IterationLimit
        } else {
            BlockEffectSegmentResult::Complete(0)
        };
    };

    if entity.is_removed() {
        BlockEffectSegmentResult::Removed
    } else {
        BlockEffectSegmentResult::Complete(iterations)
    }
}

fn relative_on_axis(position: DVec3, axis: Axis, amount: f64) -> DVec3 {
    match axis {
        Axis::X => DVec3::new(position.x + amount, position.y, position.z),
        Axis::Y => DVec3::new(position.x, position.y + amount, position.z),
        Axis::Z => DVec3::new(position.x, position.y, position.z + amount),
    }
}

fn record_movement_for_block_effects(
    entity: &dyn Entity,
    from: DVec3,
    to: DVec3,
    requested_movement: DVec3,
    actual_movement: DVec3,
) {
    let movement_length = actual_movement.length_squared();
    if movement_length > MOVEMENT_RECORD_EPSILON
        || requested_movement.length_squared() - movement_length < MOVEMENT_RECORD_EPSILON
    {
        entity.base().record_movement_this_tick(
            EntityMovement::with_axis_dependent_original_movement(from, to, requested_movement),
        );
    }
}

pub mod attribute;
mod base;
mod block_effects;
mod cache;
mod callback;
pub mod damage;
pub mod entities;
mod fluid_contact;
mod living_base;
mod movement_sync;
mod registry;
mod shared_flags;
mod storage;
mod synced_data;
mod tracker;

use crate::portal::TeleportTransition;
pub use base::{
    EntityBase, EntityBaseLoad, EntityBaseState, EntityGroundContact, EntityMovement,
    EntityMovementFlags,
};
pub use cache::EntityCache;
pub use callback::{
    EntityChunkCallback, EntityLevelCallback, NullEntityCallback, PlayerEntityCallback,
    RemovalReason,
};
pub use fluid_contact::EntityFluidContact;
pub use living_base::{DEATH_DURATION, LivingEntityBase};
pub use movement_sync::{
    EntityPositionSyncDecision, EntityPositionSyncState, POSITION_SYNC_THRESHOLD,
};
pub use registry::{ENTITIES, EntityLoadRequest, EntityRegistry, init_entities};
pub(crate) use shared_flags::EntitySharedFlags;
pub use storage::EntityStorage;
pub use synced_data::EntitySyncedData;
pub use tracker::EntityTracker;

/// Type alias for a shared entity reference.
pub type SharedEntity = Arc<dyn Entity>;

/// Type alias for a weak entity reference.
pub type WeakEntity = Weak<dyn Entity>;

/// Object-safe access to an entity trait object from default `Entity` methods.
pub trait EntityEventSource {
    /// Returns this entity as a game-event source.
    fn as_entity_event_source(&self) -> &dyn Entity;
}

impl<T: Entity> EntityEventSource for T {
    fn as_entity_event_source(&self) -> &dyn Entity {
        self
    }
}

/// A trait for entities.
///
/// This trait provides the core functionality for entities.
/// It's based on Minecraft's `Entity` class.
///
/// # Using `EntityBase`
///
/// Entities expose [`EntityBase`] to get default implementations for common
/// methods like `id()`, `uuid()`, `position()`, etc.
///
/// ```ignore
/// impl Entity for MyEntity {
///     fn base(&self) -> &EntityBase { &self.base }
///     fn entity_type(&self) -> EntityTypeRef { vanilla_entities::MY_ENTITY }
///     fn bounding_box(&self) -> WorldAabb { /* ... */ }
///     // All other common methods use defaults from EntityBase!
/// }
/// ```
pub trait Entity: EntityEventSource + Send + Sync {
    /// Returns a reference to the entity's shared vanilla base fields.
    fn base(&self) -> &EntityBase;

    /// Gets the entity type containing tracking range, dimensions, etc.
    fn entity_type(&self) -> EntityTypeRef;

    /// Gets the entity's unique network ID (session-local).
    fn id(&self) -> i32 {
        self.base().id()
    }

    /// Gets the UUID of the entity (persistent identifier).
    fn uuid(&self) -> Uuid {
        self.base().uuid()
    }

    /// Gets the entity's current position.
    fn position(&self) -> DVec3 {
        self.base().position()
    }

    /// Gets the entity position used by vanilla movement traces.
    fn old_position(&self) -> DVec3 {
        self.base().old_position()
    }

    /// Gets the entity's bounding box for collision queries.
    fn bounding_box(&self) -> WorldAabb {
        self.base().bounding_box()
    }

    /// Builds this entity's default bounding box at `position`.
    fn make_bounding_box_at(&self, position: DVec3) -> WorldAabb {
        let dimensions = self.base().dimensions();
        WorldAabb::entity_box(
            position.x,
            position.y,
            position.z,
            f64::from(dimensions.half_width()),
            f64::from(dimensions.height),
        )
    }

    /// Called every game tick when the entity is in a ticked chunk.
    ///
    /// Use `self.level()` to access the world for physics, block queries, etc.
    /// The caller (`EntityStorage`) handles base tick logic like dirty data sync.
    fn tick(&self) {}

    /// Sends position/velocity changes to tracking players.
    ///
    /// Called every tick by `EntityStorage` after `tick()`, mirrors vanilla's
    /// `ServerEntity.sendChanges()`. Handles position sync based on `updateInterval`,
    /// velocity sync when `needsSync` is set, and on-ground state changes.
    ///
    /// Default implementation does nothing. Override for entities that need
    /// position/velocity synchronization (items, projectiles, etc.).
    fn send_changes(&self, _tick_count: i32) {}

    /// Gets the world this entity is in.
    ///
    /// Returns `None` if the entity is not in a world or the world was dropped.
    /// Mirrors vanilla's `Entity.level()`.
    fn level(&self) -> Option<Arc<World>> {
        self.base().level()
    }

    /// Packs dirty entity data for network synchronization.
    ///
    /// Returns `Some(values)` if there are dirty values to sync, `None` otherwise.
    /// Clears the dirty flags after packing.
    fn pack_dirty_entity_data(&self) -> Option<Vec<DataValue>> {
        self.synced_data().and_then(EntitySyncedData::pack_dirty)
    }

    /// Packs all non-default entity data for initial spawn.
    ///
    /// Used when sending entity data to a player who just started tracking this entity.
    fn pack_all_entity_data(&self) -> Vec<DataValue> {
        self.synced_data()
            .map_or_else(Vec::new, EntitySyncedData::pack_all)
    }

    /// Returns the synchronized entity-data container for entities with vanilla data accessors.
    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        None
    }

    /// Returns true if the entity has been marked for removal.
    fn is_removed(&self) -> bool {
        self.base().is_removed()
    }

    /// Marks the entity as removed with the given reason.
    fn set_removed(&self, reason: RemovalReason) {
        self.base().set_removed(reason);
    }

    /// Sets the level callback for lifecycle events (movement, removal).
    fn set_level_callback(&self, callback: Arc<dyn EntityLevelCallback>) {
        self.base().set_level_callback(callback);
    }

    /// Gets the entity as a Player if it is one.
    fn as_player(self: Arc<Self>) -> Option<Arc<Player>> {
        None
    }

    /// Gets the entity as an `ItemEntity` if it is one.
    fn as_item_entity(self: Arc<Self>) -> Option<Arc<ItemEntity>> {
        None
    }

    /// Returns true for entities that implement vanilla living-entity behavior.
    fn is_living_entity(&self) -> bool {
        false
    }

    /// Returns true when movement is authored by a remote client.
    fn is_client_authoritative(&self) -> bool {
        false
    }

    /// Returns true when this server instance owns movement side effects.
    fn is_local_instance_authoritative(&self) -> bool {
        !self.is_client_authoritative()
    }

    /// Returns true when vanilla allows this side to apply movement simulation side effects.
    fn can_simulate_movement(&self) -> bool {
        self.is_local_instance_authoritative()
    }

    /// Returns true when vanilla landing bounce should be suppressed.
    fn is_suppressing_bounce(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_shift_key_down)
    }

    /// Returns true when vanilla collision context should treat the entity as descending.
    fn is_descending(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_shift_key_down)
    }

    /// Returns the movement vector vanilla exposes for block-contact logic.
    fn known_movement(&self) -> DVec3 {
        self.velocity()
    }

    /// Gets the entity's rotation as (yaw, pitch) in degrees.
    ///
    /// Yaw is horizontal rotation (0-360), pitch is vertical (-90 to 90).
    fn rotation(&self) -> (f32, f32) {
        self.base().rotation()
    }

    /// Sets the entity's rotation as (yaw, pitch) in degrees.
    fn set_rotation(&self, rotation: (f32, f32)) {
        self.base().set_rotation(rotation);
    }

    /// Extra spawn-packet data used by vanilla for entity-specific construction.
    fn spawn_data(&self) -> i32 {
        0
    }

    /// Gets the eye height for this entity.
    ///
    /// Default implementation returns the eye height from the entity type dimensions.
    /// Override for entities with pose-dependent eye heights (e.g., players).
    fn get_eye_height(&self) -> f64 {
        f64::from(self.base().dimensions().eye_height)
    }

    /// Gets the Y coordinate of the entity's eyes.
    ///
    /// Equivalent to vanilla's `Entity.getEyeY()`.
    fn get_eye_y(&self) -> f64 {
        self.position().y + self.get_eye_height()
    }

    /// Gets the entity's velocity in blocks per tick.
    fn velocity(&self) -> DVec3 {
        self.base().velocity()
    }

    /// Sets the entity's velocity.
    fn set_velocity(&self, velocity: DVec3) {
        self.base().set_velocity(velocity);
    }

    /// Returns accumulated vanilla fall distance.
    fn fall_distance(&self) -> f64 {
        self.base().fall_distance()
    }

    /// Sets accumulated vanilla fall distance.
    fn set_fall_distance(&self, fall_distance: f64) {
        self.base().set_fall_distance(fall_distance);
    }

    /// Resets accumulated vanilla fall distance.
    fn reset_fall_distance(&self) {
        self.base().reset_fall_distance();
    }

    /// Returns true if this entity is currently touching water.
    fn is_in_water(&self) -> bool {
        self.fluid_contact().water_height() > 0.0
    }

    /// Returns true if this entity is currently touching lava.
    fn is_in_lava(&self) -> bool {
        self.fluid_contact().lava_height() > 0.0
    }

    /// Returns cached fluid contact from the last entity fluid refresh.
    fn fluid_contact(&self) -> EntityFluidContact {
        self.base().fluid_contact()
    }

    /// Refreshes cached fluid contact from this entity's current bounding box.
    fn refresh_fluid_contact(&self) -> EntityFluidContact {
        let Some(world) = self.level() else {
            let contact = EntityFluidContact::default();
            self.base().set_fluid_contact(contact);
            return contact;
        };

        let contact = EntityFluidContact::scan(&world, self.bounding_box());
        self.base().set_fluid_contact(contact);
        contact
    }

    /// Returns true if this entity type ignores vanilla fall damage.
    fn is_fall_damage_immune(&self) -> bool {
        REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::FALL_DAMAGE_IMMUNE)
    }

    /// Applies vanilla fall damage. Base entities only propagate to passengers.
    #[expect(
        unused_variables,
        reason = "base entity fall damage is a no-op until passengers are implemented"
    )]
    fn cause_fall_damage(
        &self,
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
    ) -> bool {
        // TODO: Propagate fall damage to passengers once passenger state exists.
        false
    }

    /// Returns true if the entity is on the ground.
    fn on_ground(&self) -> bool {
        self.base().on_ground()
    }

    /// Returns true if the last movement was clipped horizontally.
    fn horizontal_collision(&self) -> bool {
        self.base().horizontal_collision()
    }

    /// Returns true if the last movement was clipped vertically.
    fn vertical_collision(&self) -> bool {
        self.base().vertical_collision()
    }

    /// Returns true if the last vertical collision was below the entity.
    fn vertical_collision_below(&self) -> bool {
        self.base().vertical_collision_below()
    }

    /// Returns true when movement bypasses collision physics.
    fn no_physics(&self) -> bool {
        self.base().no_physics()
    }

    /// Returns true when vanilla block-contact effects may run for this entity.
    fn is_affected_by_blocks(&self) -> bool {
        !self.is_removed() && !self.no_physics()
    }

    /// Sets whether this entity bypasses collision physics.
    fn set_no_physics(&self, no_physics: bool) {
        self.base().set_no_physics(no_physics);
    }

    /// Applies vanilla stuck-in-block movement for the next movement pass.
    fn make_stuck_in_block(&self, speed_multiplier: DVec3) {
        self.base().make_stuck_in_block(speed_multiplier);
    }

    /// Applies current block-contact effects to this entity.
    ///
    /// Mirrors the shared ownership boundary of vanilla `Entity.applyEffectsFromBlocks`.
    /// TODO: Extend the block behavior API with vanilla's entity-inside collision
    /// shape, fluid inside effects, and `InsideBlockEffectApplier` once those
    /// effect systems exist.
    fn apply_effects_from_blocks(&self) {
        if !self.is_affected_by_blocks() {
            return;
        }

        let Some(world) = self.level() else {
            return;
        };

        let entity = self.as_entity_event_source();
        let movements = self.base().take_movements_for_block_effects();
        let mut visited_blocks = FxHashSet::default();
        for movement in movements {
            let mut remaining_iterations = 16;
            let delta = movement.to() - movement.from();
            if let Some(original_movement) = movement.axis_dependent_original_movement()
                && delta.length_squared() > 0.0
            {
                let mut segment_from = movement.from();
                for axis in block_effects::axis_step_order(original_movement) {
                    let axis_move = block_effects::component(delta, axis);
                    if axis_move == 0.0 {
                        continue;
                    }

                    let segment_to = relative_on_axis(segment_from, axis, axis_move);
                    match apply_block_effect_segment(
                        entity,
                        &world,
                        segment_from,
                        segment_to,
                        remaining_iterations,
                        &mut visited_blocks,
                    ) {
                        BlockEffectSegmentResult::Complete(iterations) => {
                            remaining_iterations -= iterations;
                        }
                        BlockEffectSegmentResult::IterationLimit => {
                            apply_block_effect_segment(
                                entity,
                                &world,
                                movement.to(),
                                movement.to(),
                                1,
                                &mut visited_blocks,
                            );
                            return;
                        }
                        BlockEffectSegmentResult::Removed => return,
                    }
                    segment_from = segment_to;
                }
            } else {
                match apply_block_effect_segment(
                    entity,
                    &world,
                    movement.from(),
                    movement.to(),
                    remaining_iterations,
                    &mut visited_blocks,
                ) {
                    BlockEffectSegmentResult::Complete(iterations) => {
                        remaining_iterations -= iterations;
                    }
                    BlockEffectSegmentResult::IterationLimit => {
                        apply_block_effect_segment(
                            entity,
                            &world,
                            movement.to(),
                            movement.to(),
                            1,
                            &mut visited_blocks,
                        );
                        return;
                    }
                    BlockEffectSegmentResult::Removed => return,
                }
            }

            if remaining_iterations <= 0 {
                apply_block_effect_segment(
                    entity,
                    &world,
                    movement.to(),
                    movement.to(),
                    1,
                    &mut visited_blocks,
                );
                return;
            }
        }
    }

    /// Sets whether the entity is on the ground.
    fn set_on_ground(&self, on_ground: bool) {
        let ground_contact = self.ground_contact_after_movement(on_ground, None);
        let movement_flags = self.base().movement_flags().with_on_ground(on_ground);
        self.base()
            .set_movement_flags(movement_flags, ground_contact);
    }

    /// Sets ground and horizontal collision flags from accepted movement.
    fn set_on_ground_with_movement(
        &self,
        on_ground: bool,
        horizontal_collision: bool,
        movement: DVec3,
    ) {
        let ground_contact = self.ground_contact_after_movement(on_ground, Some(movement));
        self.base()
            .set_on_ground_with_movement(on_ground, horizontal_collision, ground_contact);
    }

    /// Sets the entity's position.
    fn set_position(&self, pos: DVec3) {
        self.base().set_position(pos);
    }

    /// Sets the vanilla movement-trace old position to the current position.
    fn set_old_position_to_current(&self) {
        self.base().set_old_position_to_current();
    }

    /// Sets the vanilla movement-trace old position explicitly.
    fn set_old_position(&self, old_position: DVec3) {
        self.base().set_old_position(old_position);
    }

    /// Returns the block position this entity is standing on.
    fn on_pos(&self, offset: f32) -> Option<BlockPos> {
        let world = self.level()?;

        if let Some(supporting_block) = self.base().supporting_block() {
            if offset <= 1.0e-5 {
                return Some(supporting_block);
            }

            let below_state = world.get_block_state(supporting_block);
            let below_block = below_state.get_block();
            if (offset <= 0.5 && below_block.has_tag(&BlockTag::FENCES))
                || below_block.has_tag(&BlockTag::WALLS)
                || below_block.has_tag(&BlockTag::FENCE_GATES)
            {
                return Some(supporting_block);
            }

            return Some(BlockPos::new(
                supporting_block.x(),
                (self.position().y - f64::from(offset)).floor() as i32,
                supporting_block.z(),
            ));
        }

        let position = self.position();
        Some(BlockPos::new(
            position.x.floor() as i32,
            (position.y - f64::from(offset)).floor() as i32,
            position.z.floor() as i32,
        ))
    }

    /// Returns the block position used for movement-affecting block properties.
    fn block_pos_below_that_affects_movement(&self) -> Option<BlockPos> {
        self.on_pos(0.500_001)
    }

    /// Returns vanilla `getOnPosLegacy()`, used by fall/step block hooks.
    fn on_pos_legacy(&self) -> Option<BlockPos> {
        self.on_pos(0.2)
    }

    /// Returns the vanilla block speed factor applied after movement.
    #[expect(
        clippy::float_cmp,
        reason = "intentional: vanilla checks static block speed factors against 1.0"
    )]
    fn block_speed_factor(&self) -> f32 {
        let Some(world) = self.level() else {
            return 1.0;
        };

        let position = self.position();
        let current_state = world.get_block_state(BlockPos::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        ));
        let current_block = current_state.get_block();
        let speed_factor_here = current_block.config.speed_factor;
        if current_block == &vanilla_blocks::WATER
            || current_block == &vanilla_blocks::BUBBLE_COLUMN
        {
            return speed_factor_here;
        }

        if speed_factor_here != 1.0 {
            return speed_factor_here;
        }

        let Some(below_pos) = self.block_pos_below_that_affects_movement() else {
            return 1.0;
        };
        world
            .get_block_state(below_pos)
            .get_block()
            .config
            .speed_factor
    }

    /// Maximum height this entity can step up during normal movement.
    fn max_up_step(&self) -> f32 {
        0.0
    }

    /// Whether movement should apply player-style sneak edge prevention.
    fn backs_off_from_edge(&self) -> bool {
        false
    }

    // === Physics Helper Methods ===
    // These mirror vanilla's Entity class methods.

    /// Gets the default gravity for this entity type.
    ///
    /// Override this to specify entity-specific gravity.
    /// Vanilla values: `ItemEntity` = 0.04, `LivingEntity` = 0.08
    fn get_default_gravity(&self) -> f64 {
        0.0
    }

    /// Returns true if gravity is disabled for this entity.
    fn is_no_gravity(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_no_gravity)
    }

    /// Gets the current gravity value.
    ///
    /// Returns 0 if `no_gravity` is set, otherwise returns `get_default_gravity()`.
    fn get_gravity(&self) -> f64 {
        if self.is_no_gravity() {
            0.0
        } else {
            self.get_default_gravity()
        }
    }

    /// Applies gravity to the entity's velocity.
    ///
    /// Mirrors vanilla's `Entity.applyGravity()`.
    fn apply_gravity(&self) {
        let gravity = self.get_gravity();
        if gravity != 0.0 {
            let mut vel = self.velocity();
            vel.y -= gravity;
            self.set_velocity(vel);
        }
    }

    /// Moves the entity with collision detection.
    ///
    /// Mirrors vanilla's `Entity.move(MoverType, Vec3)`.
    /// Updates position, `on_ground`, velocity (on collision), and returns collision info.
    #[expect(
        clippy::too_many_lines,
        reason = "mirrors vanilla Entity.move control flow in one auditable path"
    )]
    fn move_entity(&self, mover_type: MoverType, delta: DVec3) -> Option<MoveResult> {
        let world = self.level()?;
        if self.no_physics() {
            let final_position = self.position() + delta;
            self.set_position(final_position);
            self.base().clear_collision_flags();
            self.refresh_fluid_contact();

            return Some(MoveResult {
                final_position,
                actual_movement: delta,
                on_ground: self.on_ground(),
                horizontal_collision: false,
                vertical_collision: false,
                x_collision: false,
                z_collision: false,
                final_aabb: self.bounding_box(),
            });
        }

        let mut movement = delta;
        if mover_type == MoverType::Piston {
            let game_time = world.level_data.read().game_time();
            movement = self.base().limit_piston_movement(movement, game_time);
            if movement == DVec3::ZERO {
                return None;
            }
        }
        movement = self
            .base()
            .consume_stuck_speed_multiplier(movement, mover_type != MoverType::Piston);

        let start_position = self.position();

        // Build physics state
        let physics_state = EntityPhysicsState::with_dimensions(
            start_position,
            self.base().dimensions(),
            self.max_up_step(),
        )
        .with_on_ground(self.on_ground())
        .with_backs_off_from_edge(self.backs_off_from_edge())
        .with_fall_distance(self.fall_distance())
        .with_descending(self.is_descending());

        // Perform collision detection and movement
        let collision_world = WorldCollisionProvider::new(&world);
        let result =
            resolve_entity_movement(&physics_state, movement, mover_type, &collision_world);

        record_movement_for_block_effects(
            self.as_entity_event_source(),
            start_position,
            result.final_position,
            movement,
            result.actual_movement,
        );

        // Update entity state
        self.set_position(result.final_position);
        let movement_flags = EntityMovementFlags::after_move(
            result.on_ground,
            result.horizontal_collision,
            result.vertical_collision,
            movement,
        );
        let ground_contact =
            self.ground_contact_after_movement(result.on_ground, Some(result.actual_movement));
        self.base()
            .set_movement_flags(movement_flags, ground_contact);
        self.refresh_fluid_contact();

        if self.is_local_instance_authoritative()
            && self.apply_fall_damage_after_move(&result, &world)
        {
            return Some(result);
        }

        // Vanilla: Entity.move() zeros velocity components on collision.
        // Horizontal collision zeros X/Z individually based on which axis collided.
        // Vertical collision calls Block.updateEntityMovementAfterFallOn.
        // The default block behavior zeros Y velocity; block-specific behavior
        // can override this for slime, beds, and similar landing surfaces.
        if result.horizontal_collision {
            let vel = self.velocity();
            self.set_velocity(DVec3::new(
                if result.x_collision { 0.0 } else { vel.x },
                vel.y,
                if result.z_collision { 0.0 } else { vel.z },
            ));
        }
        if result.vertical_collision && self.can_simulate_movement() {
            let velocity = self.velocity();
            let landing_context = EntityLandingContext::new(
                velocity,
                self.is_living_entity(),
                self.is_suppressing_bounce(),
            );
            let next_velocity =
                if let Some(effect_pos) = self.block_pos_below_that_affects_movement() {
                    let effect_state = world.get_block_state(effect_pos);
                    let behavior = BLOCK_BEHAVIORS.get_behavior(effect_state.get_block());
                    behavior.update_entity_movement_after_fall_on(
                        effect_state,
                        &world,
                        effect_pos,
                        landing_context,
                    )
                } else {
                    landing_context.default_velocity_after_fall_on()
                };
            self.set_velocity(next_velocity);
        }

        let speed_factor = f64::from(self.block_speed_factor());
        let vel = self.velocity();
        self.set_velocity(DVec3::new(
            vel.x * speed_factor,
            vel.y,
            vel.z * speed_factor,
        ));

        Some(result)
    }

    /// Applies vanilla fall-distance bookkeeping after accepted movement.
    fn apply_fall_damage_after_move(&self, result: &MoveResult, world: &Arc<World>) -> bool {
        self.do_check_fall_damage(result.actual_movement, result.on_ground, world)
    }

    /// Mirrors vanilla `Entity.doCheckFallDamage`.
    ///
    /// Callers update on-ground/supporting-block state before this method.
    fn do_check_fall_damage(&self, movement: DVec3, on_ground: bool, world: &Arc<World>) -> bool {
        let Some(effect_pos) = self.on_pos_legacy() else {
            return false;
        };
        let effect_state = world.get_block_state(effect_pos);
        self.check_fall_damage(movement.y, on_ground, effect_state, effect_pos, world);
        self.is_removed()
    }

    /// Mirrors vanilla `Entity.checkFallDamage`.
    fn check_fall_damage(
        &self,
        vertical_movement: f64,
        on_ground: bool,
        on_state: BlockStateId,
        pos: BlockPos,
        world: &Arc<World>,
    ) {
        if !self.is_in_water() && vertical_movement < 0.0 {
            self.base().accumulate_fall_distance(vertical_movement);
        }

        if !on_ground {
            return;
        }

        let fall_distance = self.fall_distance();
        if fall_distance > 0.0 {
            let behavior = BLOCK_BEHAVIORS.get_behavior(on_state.get_block());
            let fall_context =
                EntityFallOnContext::new(fall_distance, self.is_suppressing_bounce());
            if let Some(fall_damage) = behavior.fall_on(on_state, world, pos, fall_context) {
                self.cause_fall_damage(
                    fall_damage.fall_distance,
                    fall_damage.damage_modifier,
                    &fall_damage.source,
                );
            }

            let supporting_state = self
                .base()
                .supporting_block()
                .map_or(on_state, |supporting_pos| {
                    world.get_block_state(supporting_pos)
                });
            world.game_event(
                &vanilla_game_events::HIT_GROUND,
                BlockPos::new(
                    self.position().x.floor() as i32,
                    self.position().y.floor() as i32,
                    self.position().z.floor() as i32,
                ),
                &GameEventContext::new(Some(self.as_entity_event_source()), Some(supporting_state)),
            );
        }

        self.reset_fall_distance();
    }

    /// Computes vanilla support state for an on-ground update.
    fn ground_contact_after_movement(
        &self,
        on_ground: bool,
        movement: Option<DVec3>,
    ) -> EntityGroundContact {
        let Some(world) = self.level() else {
            return if on_ground {
                EntityGroundContact::on_ground(None)
            } else {
                EntityGroundContact::airborne()
            };
        };

        self.check_supporting_block(on_ground, movement, &world)
    }

    /// Mirrors vanilla `Entity.checkSupportingBlock`.
    fn check_supporting_block(
        &self,
        on_ground: bool,
        movement: Option<DVec3>,
        world: &Arc<World>,
    ) -> EntityGroundContact {
        if !on_ground {
            return EntityGroundContact::airborne();
        }

        let bounding_box = self.bounding_box();
        let test_area = WorldAabb::new(
            bounding_box.min_x(),
            bounding_box.min_y() - 1.0e-6,
            bounding_box.min_z(),
            bounding_box.max_x(),
            bounding_box.min_y(),
            bounding_box.max_z(),
        );
        let collision_world = WorldCollisionProvider::new(world);
        let descending = self.is_descending();
        let mut supporting_block =
            collision_world.find_supporting_block(self.position(), &test_area, descending);

        if supporting_block.is_none()
            && !self.base().on_ground_no_blocks()
            && let Some(movement) = movement
        {
            let previous_test_area = test_area.move_by(-movement.x, 0.0, -movement.z);
            supporting_block = collision_world.find_supporting_block(
                self.position(),
                &previous_test_area,
                descending,
            );
        }

        EntityGroundContact::on_ground(supporting_block)
    }

    /// Spawns an item at this entity's location.
    ///
    /// Mirrors vanilla's `Entity.spawnAtLocation()`. The item spawns at the
    /// entity's position with the given Y offset and has a default pickup delay.
    ///
    /// Returns `None` if the item stack is empty or the entity has no world.
    fn spawn_at_location(
        &self,
        item: ItemStack,
        y_offset: f64,
    ) -> Option<Arc<entities::ItemEntity>> {
        let world = self.level()?;
        let pos = self.position();
        world.spawn_item(DVec3::new(pos.x, pos.y + y_offset, pos.z), item)
    }

    // === Persistence Methods ===
    // These mirror vanilla's Entity.addAdditionalSaveData/readAdditionalSaveData.

    /// Saves type-specific entity data to NBT.
    ///
    /// Called during chunk serialization. Implementors should save all data
    /// needed to restore entity state on load. Base fields (pos, motion,
    /// rotation, uuid, `on_ground`) are handled by the serialization layer.
    ///
    /// Mirrors vanilla's `Entity.addAdditionalSaveData()`.
    fn save_additional(&self, _nbt: &mut NbtCompound) {}

    /// Loads type-specific entity data from NBT.
    ///
    /// Called after entity creation during chunk deserialization. Base fields
    /// are already restored; this handles type-specific data.
    ///
    /// Mirrors vanilla's `Entity.readAdditionalSaveData()`.
    fn load_additional(&self, _nbt: &BaseNbtCompound<'_>) {}

    // === Tick Tracking ===
    // These methods prevent double-ticking when an entity moves between chunks
    // during the same server tick.

    /// Checks if this entity was already ticked during the given server tick.
    ///
    /// This prevents double-ticking when an entity moves to a different chunk
    /// during its tick, and that chunk gets ticked later in the same server tick.
    ///
    /// Returns `true` if already ticked this tick, `false` otherwise.
    fn was_ticked_this_tick(&self, server_tick: i32) -> bool {
        self.base().was_ticked_this_tick(server_tick)
    }

    /// Marks this entity as ticked for the given server tick.
    ///
    /// Called by `EntityStorage::tick()` before ticking an entity.
    fn mark_ticked(&self, server_tick: i32) {
        self.base().mark_ticked(server_tick);
    }

    /// Applies damage to this entity.
    ///
    /// Vanilla: `Entity.hurtServer()` — overridden by `LivingEntity` (complex
    /// armor/effects/invulnerability logic) and `ItemEntity` (health decrement
    /// and discard). Default returns `false` (entity ignores damage).
    #[expect(
        unused_variables,
        reason = "default trait impl; parameters used by overrides"
    )]
    fn hurt(&self, source: &DamageSource, amount: f32) -> bool {
        false
    }

    /// Teleports an entity from one loaded world to another.
    ///
    /// The default implementation logs a warning — non-player entity teleportation
    /// is not yet implemented. Override in entity types that support it.
    fn change_world(self: Arc<Self>, _teleport_transition: &TeleportTransition) {
        log::warn!(
            "change_world called on entity {} which does not implement world changes",
            self.id(),
        );
    }
}

/// A trait for living entities that can take damage, heal, and die.
///
/// This trait provides the core functionality for entities that have health,
/// can be damaged, and can die. It's based on Minecraft's `LivingEntity` class.
///
/// **Note:** All methods take `&self` (not `&mut self`) because living entities
/// are shared via `Arc` and use interior mutability (atomics, `SyncMutex`, etc.).
pub trait LivingEntity: Entity {
    /// Returns a reference to the shared [`LivingEntityBase`] that holds
    /// living runtime state such as attributes, cached movement speed,
    /// damage cooldown, and death animation counters.
    fn living_base(&self) -> &LivingEntityBase;

    /// Returns a reference to this entity's attribute map.
    fn attributes(&self) -> &SyncMutex<AttributeMap> {
        self.living_base().attributes()
    }

    /// Gets the current health of the entity.
    fn get_health(&self) -> f32;

    /// Sets the health of the entity, clamped between 0 and max health.
    fn set_health(&self, health: f32);

    /// Gets the maximum health from the attribute system.
    fn get_max_health(&self) -> f32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::MAX_HEALTH)
            .unwrap_or(20.0) as f32
    }

    /// Heals the entity by the specified amount.
    fn heal(&self, amount: f32) {
        let current_health = self.get_health();
        if current_health > 0.0 {
            self.set_health(current_health + amount);
        }
    }

    /// Returns true if the entity is dead or dying (health <= 0).
    fn is_dead_or_dying(&self) -> bool {
        self.get_health() <= 0.0
    }

    /// Returns true if the entity is alive (health > 0).
    fn is_alive(&self) -> bool {
        !self.is_dead_or_dying()
    }

    /// Gets the absorption amount (extra health from effects like absorption).
    fn get_absorption_amount(&self) -> f32;

    /// Sets the absorption amount.
    fn set_absorption_amount(&self, amount: f32);

    /// Gets the entity's armor value from the attribute system.
    fn get_armor_value(&self) -> i32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::ARMOR)
            .unwrap_or(0.0) as i32
    }

    /// Gets the gravity value from the attribute system.
    fn get_attribute_gravity(&self) -> f64 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::GRAVITY)
            .unwrap_or(0.08)
    }

    /// Checks if the entity can be affected by potions.
    fn is_affected_by_potions(&self) -> bool {
        true
    }

    /// Checks if the entity is attackable.
    fn attackable(&self) -> bool {
        true
    }

    /// Checks if the entity is currently using an item.
    fn is_using_item(&self) -> bool {
        false
    }

    /// Checks if the entity is blocking with a shield or similar item.
    fn is_blocking(&self) -> bool {
        false
    }

    /// Checks if the entity is fall flying (using elytra).
    fn is_fall_flying(&self) -> bool {
        false
    }

    /// Checks if the entity is sleeping.
    fn is_sleeping(&self) -> bool {
        false
    }

    /// Stops the entity from sleeping.
    fn stop_sleeping(&self) {}

    /// Checks if the entity is sprinting.
    fn is_sprinting(&self) -> bool {
        false
    }

    /// Sets whether the entity is sprinting.
    fn set_sprinting(&self, sprinting: bool);

    /// Gets the entity's cached movement speed.
    fn get_speed(&self) -> f32 {
        self.living_base().speed()
    }

    /// Sets the entity's cached movement speed.
    fn set_speed(&self, speed: f32) {
        self.living_base().set_speed(speed);
    }

    /// Drains dirty attributes and applies server-side effects.
    fn refresh_dirty_attributes(&self) {
        let dirty = self.attributes().lock().drain_dirty_updates();
        for attr in dirty {
            if attr.key == vanilla_attributes::MAX_HEALTH.key {
                let max = self.get_max_health();
                if self.get_health() > max {
                    self.set_health(max);
                }
            } else if attr.key == vanilla_attributes::MAX_ABSORPTION.key {
                let max = self
                    .attributes()
                    .lock()
                    .get_value(vanilla_attributes::MAX_ABSORPTION)
                    .unwrap_or(0.0) as f32;
                if self.get_absorption_amount() > max {
                    self.set_absorption_amount(max);
                }
            }
            // TODO: SCALE → refreshDimensions()
            // TODO: WAYPOINT_TRANSMIT_RANGE → waypoint manager
        }
    }
}
