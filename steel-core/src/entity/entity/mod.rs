//! Vanilla `Entity`: the trait every entity implements, and one function per
//! method subclasses override holding `Entity`'s own body (`entity::tick`).

use std::collections::BTreeSet;

use super::*;
use crate::entity::shearable::Shearable;
use crate::entity::{leash::Leashable, pushing::EntityPush};
use rand::{RngExt, rng};
use steel_math::DEGREE_90;
use steel_utils::DowncastTypeKey;
use steel_utils::nbt::{NbtNumeric, nbt_collection_values};

mod block_contact;
mod body;
mod combat;
mod fire;
mod fluids;
mod interaction;
mod motion;
mod persistence;
mod portals;
mod props;
mod riding;
mod sounds;
pub use {
    block_contact::EntityBlockContact, body::EntityBody, combat::EntityCombat,
    fire::EntityFireAndFreeze, fluids::EntityFluids, interaction::EntityInteraction,
    motion::EntityMotion, persistence::EntityPersistence, portals::EntityPortals,
    riding::EntityRiding, sounds::EntitySounds,
};

pub use block_contact::{
    apply_inside_block_effect, block_pos_below_that_affects_movement, block_speed_factor,
    is_ignoring_block_triggers, is_on_rails, is_stepping_carefully, make_stuck_in_block,
    movement_emission, on_climbable,
};
pub use body::{
    dimensions_for_pose, is_crouching, is_in_wall, look_at, look_at_entity, motion_direction,
    refresh_dimensions,
};
pub use combat::{
    attackable, award_kill_score, block_explosion_resistance, can_be_hit_by_projectile,
    explosion_damage_origin, explosion_indirect_source, ignore_explosion, kill, killed_entity,
    on_explosion_hit, should_block_explode, skip_attack_interaction,
};
pub use fire::{
    can_freeze, fire_ignite_extra_ticks, fire_immune, fire_immune_ticks, remaining_fire_ticks_cap,
    should_play_lava_hurt_sound,
};
pub use fluids::{
    do_water_splash_effect, is_pushed_by_fluid, is_swimming, on_above_bubble_column,
    on_inside_bubble_column, update_swimming,
};
pub use interaction::{
    interact, may_interact, notify_leash_holder, notify_leashee_removed, player_touch,
};
pub use motion::{
    air_drag, backs_off_from_edge, can_be_collided_with, can_collide_with, can_simulate_movement,
    cause_fall_damage, check_fall_damage, default_gravity, effective_gravity,
    is_hard_collision_relevant, is_suppressing_bounce, known_movement, known_speed, max_up_step,
    move_entity, push_entity, push_impulse, teleport_to, try_set_position,
    uses_client_movement_packets,
};
pub use persistence::apply_implicit_item_components;
pub use portals::{
    can_use_portal, dimension_changing_delay, on_teleported, relative_portal_position,
};
pub use props::{EntityMethods, EntityProps, EntityTable};
pub use riding::{
    can_add_passenger, could_accept_passenger, dismount_location_for_passenger,
    passenger_attachment_point, remove_vehicle, ride_tick, start_riding,
};
pub use sounds::{
    fall_sounds, is_flapping, on_flap, play_sound, play_step_sound, sound_source,
    swim_high_speed_splash_sound, swim_sound, swim_splash_sound,
};

/// Vanilla `Entity.refreshDimensions` small-entity limit: only entities at most
/// this wide and tall (in blocks) get their position fudged after growing.
const FUDGE_SMALL_DIMENSION_LIMIT: f32 = 4.0;
/// Vanilla `Entity.fudgePositionAfterSizeChange` epsilon padding (vanilla `1.0E-6`).
const FUDGE_POSITION_EPSILON: f64 = 1.0e-6;

pub(super) fn landing_particle_count(fall_distance: f64, safe_fall_distance: f64) -> Option<i32> {
    let power = (fall_distance + 1.0e-6 - safe_fall_distance)
        .floor()
        .max(0.0);
    (power > 0.0).then(|| (150.0 * (f64::from(0.2_f32) + power / 15.0).min(2.5)) as i32)
}

pub(super) fn landing_particle_position(entity_position: DVec3, landing_pos: BlockPos) -> DVec3 {
    let entity_pos = BlockPos::new(
        entity_position.x.floor() as i32,
        entity_position.y.floor() as i32,
        entity_position.z.floor() as i32,
    );
    if landing_pos.x() == entity_pos.x() && landing_pos.z() == entity_pos.z() {
        return entity_position;
    }

    let x_difference = entity_position.x - f64::from(landing_pos.x()) - 0.5;
    let z_difference = entity_position.z - f64::from(landing_pos.z()) - 0.5;
    let maximum_difference = x_difference.abs().max(z_difference.abs());
    DVec3::new(
        f64::from(landing_pos.x()) + 0.5 + x_difference / maximum_difference * 0.5,
        entity_position.y,
        f64::from(landing_pos.z()) + 0.5 + z_difference / maximum_difference * 0.5,
    )
}

pub(crate) fn nbt_dvec3(tag: &NbtTag) -> Option<DVec3> {
    let values = nbt_collection_values(tag)?;
    let values = values
        .iter()
        .filter_map(NbtNumeric::codec_f64)
        .collect::<Vec<_>>();
    let &[x, y, z, ..] = values.as_slice() else {
        return None;
    };
    Some(DVec3::new(x, y, z))
}

pub(crate) fn read_nbt_dvec3(nbt: &BorrowedNbtCompoundView<'_, '_>, key: &str) -> Option<DVec3> {
    nbt_dvec3(&nbt.get(key)?.to_owned())
}

pub(crate) fn read_nbt_rotation(
    nbt: &BorrowedNbtCompoundView<'_, '_>,
    key: &str,
) -> Option<(f32, f32)> {
    let values = nbt_collection_values(&nbt.get(key)?.to_owned())?;
    let values = values
        .iter()
        .filter_map(NbtNumeric::codec_f32)
        .collect::<Vec<_>>();
    let &[yaw, pitch, ..] = values.as_slice() else {
        return None;
    };
    Some((yaw, pitch))
}

pub(crate) fn sanitize_nbt_motion(motion: DVec3) -> DVec3 {
    let sanitize = |value: f64| {
        if value.abs() > 10.0 { 0.0 } else { value }
    };
    DVec3::new(sanitize(motion.x), sanitize(motion.y), sanitize(motion.z))
}

/// Final state accepted from a client-authored movement packet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcceptedClientMovement {
    /// Optional accepted packet position. Rotation-only packets leave this unset.
    pub position: Option<DVec3>,
    /// Accepted yaw and pitch in degrees.
    pub rotation: (f32, f32),
    /// Accepted on-ground flag.
    pub on_ground: bool,
    /// Accepted horizontal-collision flag.
    pub horizontal_collision: bool,
    /// Movement delta from the server position before processing the packet.
    pub movement: DVec3,
    /// Whether vanilla resets fall distance after the movement is applied.
    pub reset_fall_distance: bool,
}

/// Result of applying accepted client-authored movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptedClientMovementOutcome {
    /// Movement applied and regular post-processing should continue.
    Applied,
    /// Movement applied, but follow-up processing should stop because the
    /// entity handled a terminal side effect such as death.
    Handled,
}

/// Unsizes an entity reference where `Self` may not be `Sized`, as in the
/// `Entity` extension traits and the `entity::*` functions.
pub trait AsEntity {
    /// Returns this entity as `&dyn Entity`.
    fn as_entity(&self) -> &dyn Entity;
}

impl<T: Entity> AsEntity for T {
    fn as_entity(&self) -> &dyn Entity {
        self
    }
}

/// A trait for entities.
///
/// This trait provides the core functionality for entities.
/// It's based on Minecraft's `Entity` class.
/// Concrete implementations must also claim a unique [`steel_utils::DowncastTypeKey`]
/// through [`steel_utils::DowncastType`].
///
/// # Implementing
///
/// Entities expose [`EntityBase`] for the state every entity has, their
/// components, and one table per vanilla class they belong to for the methods
/// entity types change: [`EntityProps`] here, `LivingProps`, `MobProps`, ...
/// in the layers. A method goes in the table of the class that declares it,
/// and an entry that extends its parent calls the parent layer's function, as
/// below. Implementations don't override the other methods.
/// `#[derive(Entity)]` writes the impl; the layer tables are trait impls.
///
/// Behavior shared by every entity lives in extension traits per concern
/// ([`EntityMotion`], [`EntityRiding`], [`EntityFluids`], ...), implemented for
/// all entities; `use crate::entity::prelude::*` brings them into scope.
///
///
/// ```ignore
/// #[derive(Entity)]
/// #[entity(save = Self::save_my_entity, load = Self::load_my_entity)]
/// #[entity(key = "steel:entity/my_entity")]
/// pub struct MyEntity {
///     #[entity(base)]
///     base: EntityBase,
///     #[entity(entity_type)]
///     entity_type: EntityTypeRef,
///     #[entity(component)]
///     living_base: LivingEntityBase,
///     #[entity(synced)]
///     entity_data: CheckedMutex<MyEntityData>,
/// }
///
/// impl EntityTable for MyEntity {
///     const ENTITY: EntityProps<Self> = EntityProps {
///         is_pickable: |_| true,
///         ..living_entity::entity_props()
///     };
/// }
///
/// impl LivingTable for MyEntity {
///     const LIVING: LivingProps<Self> = LivingProps {
///         // `super.aiStep()`, then this type's part.
///         ai_step: |entity| {
///             let movement = living_entity::ai_step(entity);
///             entity.tick_my_state();
///             movement
///         },
///         ..LivingProps::new()
///     };
/// }
/// ```
pub trait Entity: EntityMethods + AsEntity + ErasedType + Send + Sync + 'static {
    /// Returns a reference to the entity's shared vanilla base fields.
    fn base(&self) -> &EntityBase;

    /// Gets the entity type containing tracking range, dimensions, etc.
    fn entity_type(&self) -> EntityTypeRef;

    /// Returns this entity's component whose downcast key is `key`; see
    /// [`Components::query`](crate::entity::component::Components::query).
    fn erased_component(&self, _key: DowncastTypeKey) -> Option<&dyn ErasedType> {
        None
    }

    /// Returns a shared handle to this entity, for code that keeps one beyond
    /// the current call. `None` only while the entity is being built or
    /// dropped.
    fn shared(&self) -> Option<SharedEntity> {
        self.base().shared()
    }

    /// Gets the entity's unique network ID (session-local).
    fn id(&self) -> i32 {
        self.base().id()
    }

    /// Gets the generation counter of this runtime construction of the entity.
    fn generation(&self) -> EntityGeneration {
        self.base().generation()
    }

    /// Gets the UUID of the entity (persistent identifier).
    fn uuid(&self) -> Uuid {
        self.base().uuid()
    }

    /// Gets the entity's current position.
    fn position(&self) -> DVec3 {
        self.base().position()
    }

    /// Gets the entity's current block position.
    fn block_position(&self) -> BlockPos {
        let position = self.position();
        BlockPos::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        )
    }

    /// Returns the block state this entity's origin currently overlaps.
    fn in_block_state(&self, world: &World) -> BlockStateId {
        self.base().in_block_state(world)
    }

    /// Gets the entity position used by vanilla movement traces.
    fn old_position(&self) -> DVec3 {
        self.base().old_position()
    }

    /// Gets the entity's bounding box for collision queries.
    fn bounding_box(&self) -> WorldAabb {
        self.base().bounding_box()
    }

    /// Returns whether this entity is invisible, per its synced entity data.
    fn is_invisible(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_base_invisible_flag)
    }

    /// Returns whether this entity is sneaking (shift key down), per its synced entity data.
    fn is_discrete(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_shift_key_down)
    }

    /// Returns whether this entity is allied to `other`.
    fn is_allied_to(&self, _other: &dyn Entity) -> bool {
        false
    }

    /// Returns whether this entity is a marker armor stand.
    fn is_marker_armor_stand(&self) -> bool {
        false
    }

    /// Returns whether this entity is a tameable animal owned by `owner`.
    fn is_tame_owned_by(&self, _owner: &Living<'_>) -> bool {
        false
    }

    /// Returns the owner UUID when this entity is a projectile.
    fn projectile_owner_uuid(&self) -> Option<Uuid> {
        self.as_projectile()?.owner_uuid()
    }

    /// Returns the live owner when this entity is a projectile.
    fn projectile_owner(&self) -> Option<SharedEntity> {
        self.as_projectile()?.owner()
    }

    /// Opts this entity into the [`DefaultEntityPush`] fast path. It is read under the
    /// entity manager lock, so it must give the same answer for the entity's whole
    /// lifetime and have no side effects.
    fn as_default_entity_push(&self) -> Option<&dyn DefaultEntityPush> {
        try_as_dyn::<Self, dyn DefaultEntityPush>(self)
    }

    /// Applies vanilla below-world handling.
    fn check_below_world(&self) {
        let Some(world) = self.level() else {
            return;
        };

        if self.position().y < f64::from(world.get_min_y() - 64) {
            self.on_below_world();
        }
    }

    /// Gets the world this entity is in.
    ///
    /// Returns `None` if the entity is not in a world or the world was dropped.
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

    /// Returns why this entity was removed, if it has been removed.
    fn removal_reason(&self) -> Option<RemovalReason> {
        self.base().removal_reason()
    }

    /// Marks the entity as removed with the given reason.
    ///
    /// Only the first removal takes effect. Like vanilla, the entity leaves
    /// its vehicle when destroyed and its passengers dismount through their
    /// own `stop_riding` before the world learns of the removal.
    fn set_removed(&self, reason: RemovalReason) {
        let Some(callback) = self.base().mark_removed(reason) else {
            return;
        };
        if reason.should_destroy() {
            self.stop_riding();
        }
        for passenger in self.passengers() {
            passenger.stop_riding();
        }
        self.base().finish_removal(callback, reason);
        self.on_removal(reason);
    }

    /// Emits a vanilla game event from this entity's exact position with an explicit source entity.
    fn game_event_with_source_entity(
        &self,
        event: GameEventRef,
        source_entity: Option<&dyn Entity>,
    ) {
        let Some(world) = self.level() else {
            return;
        };

        world.game_event_at(
            event,
            self.position(),
            &GameEventContext::new(source_entity, None),
        );
    }

    /// Emits a vanilla game event from this entity's exact position.
    fn game_event(&self, event: GameEventRef) {
        self.game_event_with_source_entity(event, Some(self.as_entity()));
    }

    /// Sets the level callback for lifecycle events (movement, removal).
    fn set_level_callback(&self, callback: Arc<dyn EntityLevelCallback>) {
        self.base().set_level_callback(callback);
    }

    /// Returns true for entities that implement vanilla living-entity behavior.
    fn is_living_entity(&self) -> bool {
        self.as_living_entity().is_some()
    }

    /// Returns whether this entity belongs to vanilla's `AbstractArrow` hierarchy.
    fn is_abstract_arrow(&self) -> bool {
        self.entity_type().is_abstract_arrow
    }

    /// Returns this entity as a projectile, if it is one.
    fn as_projectile(&self) -> Option<Projectile<'_>> {
        Projectile::of(self.as_entity())
    }

    /// Returns this entity as a living entity when it has living behavior.
    fn as_living_entity(&self) -> Option<Living<'_>> {
        Living::of(self.as_entity())
    }

    /// Returns this entity as an item frame when it has item-frame behavior.
    fn as_item_frame(&self) -> Option<&dyn ItemFrame> {
        try_as_dyn::<Self, dyn ItemFrame>(self)
    }

    /// Returns this entity as a player when it is the concrete server player.
    fn as_player(&self) -> Option<&Player> {
        self.downcast_ref::<Player>()
    }

    /// Returns true for mobs with pathfinding navigation.
    fn is_pathfinder_mob(&self) -> bool {
        self.as_pathfinder_mob().is_some()
    }

    /// Returns this entity as a pathfinder mob when it has pathfinding behavior.
    fn as_pathfinder_mob(&self) -> Option<Mob<'_>> {
        self.as_mob().filter(Mob::is_pathfinder)
    }

    /// Returns true for entities that implement vanilla mob behavior.
    fn is_mob(&self) -> bool {
        self.as_mob().is_some()
    }

    /// Returns this entity as a mob when it has mob behavior.
    fn as_mob(&self) -> Option<Mob<'_>> {
        Mob::of(self.as_entity())
    }

    /// Returns this entity as [`Shearable`] when shears act on it.
    fn as_shearable(&self) -> Option<&dyn Shearable> {
        try_as_dyn::<Self, dyn Shearable>(self)
    }

    /// Returns this entity as a `Leashable` if it has [`Leashable`] behavior.
    fn as_leashable(&self) -> Option<&dyn Leashable> {
        try_as_dyn::<Self, dyn Leashable>(self)
    }

    /// Returns this entity's `LivingEntity` properties when it is a living entity;
    /// [`Living::of`] builds the view from them.
    fn as_living_methods(&self) -> Option<&dyn LivingMethods> {
        try_as_dyn::<Self, dyn LivingMethods>(self)
    }

    /// Returns this entity's `Mob` properties when it is a mob.
    fn as_mob_methods(&self) -> Option<&dyn MobMethods> {
        try_as_dyn::<Self, dyn MobMethods>(self)
    }

    /// Returns this entity's `AgeableMob` properties when it is an ageable mob.
    fn as_ageable_methods(&self) -> Option<&dyn AgeableMethods> {
        try_as_dyn::<Self, dyn AgeableMethods>(self)
    }

    /// Returns this entity's `Animal` properties when it is an animal.
    fn as_animal_methods(&self) -> Option<&dyn AnimalMethods> {
        try_as_dyn::<Self, dyn AnimalMethods>(self)
    }

    /// Returns this entity's `Projectile` properties when it is a projectile.
    fn as_projectile_methods(&self) -> Option<&dyn ProjectileMethods> {
        try_as_dyn::<Self, dyn ProjectileMethods>(self)
    }

    /// Returns this entity's `AbstractMinecart` properties when it is a minecart.
    fn as_abstract_minecart_methods(&self) -> Option<&dyn AbstractMinecartMethods> {
        try_as_dyn::<Self, dyn AbstractMinecartMethods>(self)
    }

    /// Whether this entity implements the `Enemy` marker.
    fn is_enemy(&self) -> bool {
        self.as_enemy().is_some()
    }

    /// Returns the `Enemy` marker, equivalent to vanilla's `instanceof Enemy`.
    fn as_enemy(&self) -> Option<&dyn Enemy> {
        try_as_dyn::<Self, dyn Enemy>(self)
    }

    /// Returns true for entities that implement vanilla item-steered boosts.
    fn is_item_steerable(&self) -> bool {
        self.as_item_steerable().is_some()
    }

    /// Returns this entity as item steerable when it has item-steering behavior.
    fn as_item_steerable(&self) -> Option<&dyn ItemSteerable> {
        try_as_dyn::<Self, dyn ItemSteerable>(self)
    }

    /// Returns the number of ticks this entity has existed for.
    fn tick_count(&self) -> i32 {
        self.base().tick_count()
    }

    /// Returns whether this entity has not completed its first tick.
    fn is_first_tick(&self) -> bool {
        self.base().is_first_tick()
    }

    /// Sets whether this entity has not completed its first tick.
    fn set_first_tick(&self, first_tick: bool) {
        self.base().set_first_tick(first_tick);
    }

    /// Advances this entity's tick counter by one.
    fn advance_tick_count(&self) {
        self.base().advance_tick_count();
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

    /// Projects all base-owned synchronized fields into generated entity data.
    fn sync_base_entity_data(&self) {
        let Some(synced_data) = self.synced_data() else {
            return;
        };

        let save_data = self.base().save_data();
        synced_data.set_air_supply(save_data.air_supply);
        synced_data.set_custom_name(save_data.custom_name);
        synced_data.set_custom_name_visible(save_data.custom_name_visible);
        synced_data.set_silent(save_data.silent);
        synced_data.set_no_gravity(save_data.no_gravity);
        synced_data.set_base_glowing_flag(save_data.glowing);
        synced_data.set_base_ticks_frozen(self.ticks_frozen());
        synced_data.set_base_on_fire_flag(self.is_on_fire() || self.has_visual_fire());
    }

    /// Returns this entity's optional vanilla custom name.
    fn custom_name(&self) -> Option<TextComponent> {
        self.base().custom_name()
    }

    /// Sets this entity's optional vanilla custom name.
    fn set_custom_name(&self, custom_name: Option<TextComponent>) {
        self.base().set_custom_name(custom_name.clone());
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_custom_name(custom_name);
        }
    }

    /// Returns whether vanilla renders this entity's custom name.
    fn is_custom_name_visible(&self) -> bool {
        self.base().custom_name_visible()
    }

    /// Sets whether vanilla renders this entity's custom name.
    fn set_custom_name_visible(&self, visible: bool) {
        self.base().set_custom_name_visible(visible);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_custom_name_visible(visible);
        }
    }

    /// Returns whether this entity has the server-owned vanilla glowing tag.
    fn has_glowing_tag(&self) -> bool {
        self.base().glowing()
    }

    /// Sets this entity's server-owned vanilla glowing tag.
    fn set_glowing_tag(&self, glowing: bool) {
        self.base().set_glowing(glowing);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_base_glowing_flag(glowing);
        }
    }

    /// Returns a snapshot of this entity's vanilla scoreboard tags.
    fn tags(&self) -> Vec<String> {
        self.base().tags()
    }

    /// Adds a vanilla scoreboard tag.
    fn add_tag(&self, tag: String) -> bool {
        self.base().add_tag(tag)
    }

    /// Removes a vanilla scoreboard tag.
    fn remove_tag(&self, tag: &str) -> bool {
        self.base().remove_tag(tag)
    }

    /// Returns a snapshot of this entity's vanilla custom data.
    fn custom_data(&self) -> NbtCompound {
        self.base().custom_data()
    }

    /// Replaces this entity's vanilla custom data.
    fn set_custom_data(&self, custom_data: NbtCompound) {
        self.base().set_custom_data(custom_data);
    }

    /// Broadcasts a vanilla entity event/status packet near this entity.
    fn broadcast_entity_event(&self, event: EntityStatus) {
        let Some(world) = self.level() else {
            return;
        };

        world.broadcast_to_nearby(
            ChunkPos::from_entity_pos(self.position()),
            CEntityEvent {
                entity_id: self.id(),
                event,
            },
            None,
        );
    }

    /// Returns whether this entity is invulnerable to non-bypassing damage.
    fn is_invulnerable(&self) -> bool {
        self.base().invulnerable()
    }

    /// Sets whether this entity is invulnerable to non-bypassing damage.
    fn set_invulnerable(&self, invulnerable: bool) {
        self.base().set_invulnerable(invulnerable);
    }

    // These mirror vanilla's Entity.addAdditionalSaveData/readAdditionalSaveData.

    /// Saves type-specific entity data to NBT.
    ///
    /// Called during chunk serialization. Implementors should save all data
    /// needed to restore entity state on load. Base fields (pos, motion,
    /// rotation, uuid, `on_ground`) are handled by the serialization layer.
    fn save_additional(&self, _nbt: &mut NbtCompound) {}

    /// Loads type-specific entity data from NBT.
    ///
    /// Called after entity creation during chunk deserialization. Base fields
    /// are already restored; this handles type-specific data.
    fn load_additional(&self, _nbt: BorrowedNbtCompoundView<'_, '_>) {}

    // This already exists for structures and AABB whatever that might be, but I also need it for entities, I hope this is the right spot to put it.
    /// Calculates the squared Euclidean distance from this entity's position to the given position.
    fn distance_to_sqr(&self, pos: DVec3) -> f64 {
        let dx = self.position().x - pos.x;
        let dy = self.position().y - pos.y;
        let dz = self.position().z - pos.z;

        dx * dx + dy * dy + dz * dz
    }
}

/// `Entity`'s [`EntityProps::is_always_ticking`].
pub const fn is_always_ticking<E: Entity + ?Sized>(_entity: &E) -> bool {
    false
}

/// `Entity`'s [`EntityProps::broadcast_to_player`].
pub const fn broadcast_to_player<E: Entity + ?Sized>(_entity: &E, _player: &Player) -> bool {
    true
}

/// `Entity`'s [`EntityProps::scoreboard_name`].
pub fn scoreboard_name<E: Entity + ?Sized>(entity: &E) -> String {
    entity.uuid().to_string()
}

/// `Entity`'s [`EntityProps::name`].
pub fn name<E: Entity + ?Sized>(entity: &E) -> TextComponent {
    entity.custom_name().map_or_else(
        || entity_type_name(entity.entity_type()),
        remove_entity_name_actions,
    )
}

/// `Entity`'s [`EntityProps::display_name`].
pub fn display_name<E: Entity + ?Sized>(entity: &E) -> TextComponent {
    let name = entity.name();
    name.clone()
        .hover_event(HoverEvent::show_entity(
            entity.entity_type().key.to_string(),
            entity.uuid(),
            Some(name),
        ))
        .insertion(entity.uuid().to_string())
}

/// `Entity`'s [`EntityProps::plain_text_name`].
pub fn plain_text_name<E: Entity + ?Sized>(entity: &E) -> String {
    entity.name().to_plain(&DisplayResolutor)
}

/// `Entity`'s [`EntityProps::spawn_position`].
pub fn spawn_position<E: Entity + ?Sized>(entity: &E) -> DVec3 {
    entity.position()
}

/// `Entity`'s [`EntityProps::blocks_building`].
pub const fn blocks_building<E: Entity + ?Sized>(_entity: &E) -> bool {
    false
}

/// `Entity`'s [`EntityProps::is_pickable`].
pub const fn is_pickable<E: Entity + ?Sized>(_entity: &E) -> bool {
    false
}

/// `Entity`'s [`EntityProps::is_pushable`].
pub const fn is_pushable<E: Entity + ?Sized>(_entity: &E) -> bool {
    false
}

/// `Entity`'s [`EntityProps::is_spectator`].
pub const fn is_spectator<E: Entity + ?Sized>(_entity: &E) -> bool {
    false
}

/// `Entity`'s [`EntityProps::piston_push_reaction`].
pub fn piston_push_reaction<E: Entity + ?Sized>(entity: &E) -> PushReaction {
    if entity.is_marker_armor_stand() {
        PushReaction::IgnoreEntity
    } else {
        entity.entity_type().flags.piston_push_reaction
    }
}

/// `Entity`'s [`EntityProps::tick`]: vanilla `Entity.tick` is the base tick.
pub fn tick<E: Entity + ?Sized>(entity: &E) {
    entity.base_tick();
}

/// `Entity`'s [`EntityProps::hurt`]: vanilla `Entity.hurtServer` is
/// abstract, so an entity type without one takes no damage.
pub const fn hurt<E: Entity + ?Sized>(
    _entity: &E,
    _world: &World,
    _source: &DamageSource,
    _amount: f32,
) -> bool {
    false
}

/// `Entity`'s [`EntityProps::stop_riding`].
pub fn stop_riding<E: Entity + ?Sized>(entity: &E) {
    entity.remove_vehicle();
}

/// `Entity`'s [`EntityProps::set_y_body_rot`]: only living entities have
/// a body rotation.
pub const fn set_y_body_rot<E: Entity + ?Sized>(_entity: &E, _y_body_rot: f32) {}

/// `Entity`'s [`EntityProps::position_rider`]: repositions a direct
/// passenger from this vehicle's attachment point.
pub fn position_rider<E: Entity + ?Sized>(this: &E, passenger: &dyn Entity) {
    if !this.has_passenger(passenger) {
        return;
    }

    let riding_position = this.passenger_riding_position(passenger);
    let vehicle_attachment = passenger.vehicle_attachment_point(this.as_entity());
    if let Err(error) = passenger.try_set_position(riding_position - vehicle_attachment) {
        log::debug!(
            "Failed to position passenger {} riding entity {}: {error}",
            passenger.id(),
            this.id()
        );
    }
}

/// `Entity`'s [`EntityProps::controlling_passenger`]: entities have none.
pub fn controlling_passenger<E: Entity + ?Sized>(_entity: &E) -> Option<SharedEntity> {
    None
}

/// `Entity`'s [`EntityProps::base_tick`]: vanilla `Entity.baseTick`.
pub fn base_tick<E: Entity + ?Sized>(entity: &E) {
    entity.base().advance_base_tick_state();
    entity.handle_portal();
    entity.base().advance_powder_snow_contact_for_base_tick();
    entity.base().update_was_eye_in_water();
    entity.update_fluid_interaction();
    entity.update_swimming();
    if entity
        .base()
        .advance_fire_tick(entity.fire_immune(), entity.is_in_lava())
        && let Some(world) = entity.level()
    {
        entity.hurt(
            &world,
            &DamageSource::environment(&vanilla_damage_types::ON_FIRE),
            1.0,
        );
    }
    entity.base().dampen_fall_distance_in_lava();
    entity.check_below_world();
    entity.sync_base_fire_freeze_entity_data();
    entity.set_first_tick(false);
    // Vanilla checks `this instanceof Leashable` inside `Entity.baseTick`.
    if let Some(mob) = entity.as_leashable() {
        mob.tick_leash();
    }
    // VANILLA CLIENT-LOCAL: `Entity.spawnSprintParticle` creates sprint particles.
}

/// `Entity`'s [`EntityProps::on_below_world`].
pub fn on_below_world<E: Entity + ?Sized>(entity: &E) {
    entity.set_removed(RemovalReason::Discarded);
}

/// `Entity`'s [`EntityProps::check_despawn`].
pub const fn check_despawn<E: Entity + ?Sized>(_entity: &E) {}

/// `Entity`'s [`EntityProps::update_data_before_sync`].
pub const fn update_data_before_sync<E: Entity + ?Sized>(_entity: &E) {}

/// `Entity`'s [`EntityProps::is_alive`].
pub fn is_alive<E: Entity + ?Sized>(entity: &E) -> bool {
    !entity.is_removed()
}

/// `Entity`'s [`EntityProps::on_removal`].
pub const fn on_removal<E: Entity + ?Sized>(_entity: &E, _reason: RemovalReason) {}

/// `Entity`'s [`EntityProps::restore_owner_reference`].
pub fn restore_owner_reference<E: Entity + ?Sized>(_entity: &E, _owner: &SharedEntity) {}

/// `Entity`'s [`EntityProps::with_weapon_item`].
pub fn with_weapon_item<E: Entity + ?Sized>(
    _entity: &E,
    visitor: &mut dyn FnMut(Option<&ItemStack>),
) {
    visitor(None);
}

/// `Entity`'s [`EntityProps::is_effective_ai`].
pub fn is_effective_ai<E: Entity + ?Sized>(entity: &E) -> bool {
    entity.is_server_driven_movement()
}

/// `Entity`'s [`EntityProps::y_head_rot`].
pub const fn y_head_rot<E: Entity + ?Sized>(_entity: &E) -> f32 {
    0.0
}

/// `Entity`'s [`EntityProps::set_y_head_rot`].
pub const fn set_y_head_rot<E: Entity + ?Sized>(_entity: &E, _y_head_rot: f32) {}

/// `Entity`'s [`EntityProps::spawn_data`].
pub const fn spawn_data<E: Entity + ?Sized>(_entity: &E) -> i32 {
    0
}

pub(super) fn compute_modified_friction(friction: f32, modifier: f32) -> f32 {
    (1.0 - (1.0 - friction) * modifier).clamp(0.0, 1.0)
}

pub(crate) fn apply_entity_look_at(entity: &dyn Entity, from_anchor: EntityAnchor, target: DVec3) {
    let rotation = look_at_rotation(from_anchor.position(entity), target);
    entity.set_rotation(rotation);
    let rotation = entity.rotation();
    entity.set_y_head_rot(rotation.0);
    entity.base().set_old_rotation_to_current();
}

fn look_at_rotation(from: DVec3, target: DVec3) -> (f32, f32) {
    let delta = target - from;
    let horizontal = delta.x.hypot(delta.z);
    let pitch = wrap_degrees(-delta.y.atan2(horizontal).to_degrees() as f32);
    let yaw = wrap_degrees(delta.z.atan2(delta.x).to_degrees() as f32 - DEGREE_90);
    (yaw, pitch)
}

#[cfg(test)]
mod tests;
