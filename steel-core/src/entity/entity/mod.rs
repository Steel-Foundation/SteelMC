use std::collections::BTreeSet;

use super::*;
use crate::entity::component::definition::{DefinitionEntries, ENTITY_DEFINITION};
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
mod hooks;
mod interaction;
mod motion;
mod persistence;
mod portals;
mod riding;
mod sounds;
pub use {
    block_contact::EntityBlockContact, body::EntityBody, combat::EntityCombat,
    fire::EntityFireAndFreeze, fluids::EntityFluids, interaction::EntityInteraction,
    motion::EntityMotion, persistence::EntityPersistence, portals::EntityPortals,
    riding::EntityRiding, sounds::EntitySounds,
};

pub use hooks::{EntityDefaults, EntityHooks, EntityProps};
use hooks::{entity_hook_dispatch, entity_hook_list};

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

/// Object-safe access to an entity trait object from default `Entity` methods.
pub trait EntityEventSource {
    /// Returns this entity as a game-event source.
    fn as_entity_event_source(&self) -> &dyn Entity;

    /// Retains this exact entity allocation as a shared entity handle.
    fn into_shared_entity(self: Arc<Self>) -> SharedEntity;
}

impl<T: Entity> EntityEventSource for T {
    fn into_shared_entity(self: Arc<Self>) -> SharedEntity {
        self
    }

    fn as_entity_event_source(&self) -> &dyn Entity {
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
/// components, and their [`EntityHooks`] for the methods entity types change
/// (see [`EntityProps`]). Implementations don't override the other methods;
/// entity types change behavior through hooks. `#[derive(Entity)]` writes the
/// impl.
///
/// Behavior shared by every entity lives in extension traits per concern
/// ([`EntityMotion`], [`EntityRiding`], [`EntityFluids`], ...), implemented for
/// all entities; `use crate::entity::prelude::*` brings them into scope.
///
///
/// ```ignore
/// #[derive(Entity)]
/// #[entity(hooks = Self::ENTITY, save = Self::save_my_entity, load = Self::load_my_entity)]
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
/// impl MyEntity {
///     const ENTITY: EntityProps<Self> = EntityProps {
///         is_pickable: |_| true,
///         ..living_entity::entity_props()
///     };
/// }
/// ```
pub trait Entity: EntityEventSource + ErasedType + Send + Sync + 'static {
    /// Returns a reference to the entity's shared vanilla base fields.
    fn base(&self) -> &EntityBase;

    /// Gets the entity type containing tracking range, dimensions, etc.
    fn entity_type(&self) -> EntityTypeRef;

    /// This entity type's hooks, usually its [`EntityProps`].
    fn entity_hooks(&self) -> &'static dyn EntityHooks;

    /// This entity type's pipelines, usually its [`Definition`]. Entities
    /// written without `#[derive(Entity)]` get vanilla `Entity`'s.
    fn entity_definition(&self) -> &'static dyn DefinitionEntries {
        &*ENTITY_DEFINITION
    }

    entity_hook_list!(entity_hook_dispatch);

    /// Returns this entity's component whose downcast key is `key`; see
    /// [`Components::query`](crate::entity::component::Components::query).
    fn erased_component(&self, _key: DowncastTypeKey) -> Option<&dyn ErasedType> {
        None
    }

    /// Default for [`Self::is_always_ticking`].
    fn is_always_ticking_default(&self) -> bool {
        false
    }

    /// Default for [`Self::broadcast_to_player`].
    fn broadcast_to_player_default(&self, _player: &Player) -> bool {
        true
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

    /// Default for [`Self::scoreboard_name`].
    fn scoreboard_name_default(&self) -> String {
        self.uuid().to_string()
    }

    /// Default for [`Self::name`].
    fn name_default(&self) -> TextComponent {
        self.custom_name().map_or_else(
            || entity_type_name(self.entity_type()),
            remove_entity_name_actions,
        )
    }

    /// Default for [`Self::display_name`].
    fn display_name_default(&self) -> TextComponent {
        let name = self.name();
        name.clone()
            .hover_event(HoverEvent::show_entity(
                self.entity_type().key.to_string(),
                self.uuid(),
                Some(name),
            ))
            .insertion(self.uuid().to_string())
    }

    /// Default for [`Self::plain_text_name`].
    fn plain_text_name_default(&self) -> String {
        self.name().to_plain(&DisplayResolutor)
    }

    /// Gets the entity's current position.
    fn position(&self) -> DVec3 {
        self.base().position()
    }

    /// Default for [`Entity::spawn_position`].
    fn spawn_position_default(&self) -> DVec3 {
        self.position()
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

    /// Default for [`Self::blocks_building`].
    fn blocks_building_default(&self) -> bool {
        false
    }

    /// Default for [`Self::is_pickable`].
    fn is_pickable_default(&self) -> bool {
        false
    }

    /// Default for [`Self::is_pushable`].
    fn is_pushable_default(&self) -> bool {
        false
    }

    /// Default for [`Self::is_spectator`].
    fn is_spectator_default(&self) -> bool {
        false
    }

    /// Default for [`Entity::piston_push_reaction`].
    fn piston_push_reaction_default(&self) -> PushReaction {
        if self.is_marker_armor_stand() {
            PushReaction::IgnoreEntity
        } else {
            self.entity_type().flags.piston_push_reaction
        }
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

    /// Called every game tick when the entity is in a ticked chunk: runs this
    /// type's `tick` pipeline. The caller handles post-tick dirty data sync.
    fn tick(self: Arc<Self>) {
        let entity = self.into_shared_entity();
        entity.entity_definition().tick(entity.as_ref(), &entity);
    }

    /// Vanilla `Entity.interact`: runs this type's `interact` pipeline.
    fn interact(
        &self,
        player: &Player,
        hand: InteractionHand,
        location: DVec3,
    ) -> InteractionResult {
        self.entity_definition()
            .interact(self.as_entity_event_source(), player, hand, location)
    }

    /// Vanilla `Entity.setYBodyRot`: runs this type's `set_y_body_rot` pipeline.
    fn set_y_body_rot(&self, y_body_rot: f32) {
        self.entity_definition()
            .set_y_body_rot(self.as_entity_event_source(), y_body_rot);
    }

    /// Vanilla `Entity.positionRider`: places `passenger` on this vehicle
    /// through this type's `position_rider` pipeline.
    fn position_rider(&self, passenger: &dyn Entity) {
        self.entity_definition()
            .position_rider(self.as_entity_event_source(), passenger);
    }

    /// Vanilla `Entity.getControllingPassenger`: the passenger steering this
    /// entity, through this type's `controlling_passenger` pipeline.
    fn controlling_passenger(&self) -> Option<SharedEntity> {
        self.entity_definition()
            .controlling_passenger(self.as_entity_event_source())
    }

    /// Vanilla `Entity.hurtServer`: applies damage through this type's
    /// `hurt` pipeline, returning whether it was taken.
    fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        self.entity_definition()
            .hurt(self.as_entity_event_source(), world, source, amount)
    }

    /// Vanilla `Entity.baseTick`: runs this type's `base_tick` pipeline. It
    /// stays separate from [`Self::tick`] because several vanilla subclasses
    /// override `tick` without calling `super.tick()`.
    fn base_tick(&self) {
        self.entity_definition()
            .base_tick(self.as_entity_event_source());
    }

    /// Runs only vanilla `Entity.baseTick` behavior.
    ///
    /// Subtype base-tick chains call this from their owner trait.
    fn entity_base_tick(&self) {
        self.base().advance_base_tick_state();
        self.handle_portal();
        self.base().advance_powder_snow_contact_for_base_tick();
        self.base().update_was_eye_in_water();
        self.update_fluid_interaction();
        self.update_swimming();
        if self
            .base()
            .advance_fire_tick(self.fire_immune(), self.is_in_lava())
            && let Some(world) = self.level()
        {
            self.hurt(
                &world,
                &DamageSource::environment(&vanilla_damage_types::ON_FIRE),
                1.0,
            );
        }
        self.base().dampen_fall_distance_in_lava();
        self.check_below_world();
        self.sync_base_fire_freeze_entity_data();
        self.set_first_tick(false);
        // Vanilla checks `this instanceof Leashable` inside `Entity.baseTick`.
        if let Some(mob) = self.as_leashable() {
            mob.tick_leash();
        }
        // VANILLA CLIENT-LOCAL: `Entity.spawnSprintParticle` creates sprint particles.
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

    /// Default for [`Self::on_below_world`].
    fn on_below_world_default(&self) {
        self.set_removed(RemovalReason::Discarded);
    }

    /// Default for [`Self::check_despawn`].
    fn check_despawn_default(&self) {}

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

    /// Default for [`Self::update_data_before_sync`].
    fn update_data_before_sync_default(&self) {}

    /// Returns true if the entity has been marked for removal.
    fn is_removed(&self) -> bool {
        self.base().is_removed()
    }

    /// Default for [`Self::is_alive`].
    fn is_alive_default(&self) -> bool {
        !self.is_removed()
    }

    /// Returns why this entity was removed, if it has been removed.
    fn removal_reason(&self) -> Option<RemovalReason> {
        self.base().removal_reason()
    }

    /// Marks the entity as removed with the given reason.
    fn set_removed(&self, reason: RemovalReason) {
        if self.base().set_removed(reason) {
            self.on_removal(reason);
        }
    }

    /// Default for [`Entity::on_removal`].
    fn on_removal_default(&self, _reason: RemovalReason) {}

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
        self.game_event_with_source_entity(event, Some(self.as_entity_event_source()));
    }

    /// Default for [`Entity::restore_owner_reference`].
    fn restore_owner_reference_default(&self, _owner: &SharedEntity) {}

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
        Projectile::of(self.as_entity_event_source())
    }

    /// Returns this entity as a living entity when it has living behavior.
    fn as_living_entity(&self) -> Option<Living<'_>> {
        Living::of(self.as_entity_event_source())
    }

    /// Returns this entity as an item frame when it has item-frame behavior.
    fn as_item_frame(&self) -> Option<&dyn ItemFrame> {
        try_as_dyn::<Self, dyn ItemFrame>(self)
    }

    /// Returns this entity as a player when it is the concrete server player.
    fn as_player(&self) -> Option<&Player> {
        self.downcast_ref::<Player>()
    }

    /// Default for [`Self::with_weapon_item`].
    fn with_weapon_item_default(&self, visitor: &mut dyn FnMut(Option<&ItemStack>)) {
        visitor(None);
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
        Mob::of(self.as_entity_event_source())
    }

    /// Returns this entity as [`Shearable`] when shears act on it.
    fn as_shearable(&self) -> Option<&dyn Shearable> {
        try_as_dyn::<Self, dyn Shearable>(self)
    }

    /// Returns this entity as a `Leashable` if it has [`Leashable`] behavior.
    fn as_leashable(&self) -> Option<&dyn Leashable> {
        try_as_dyn::<Self, dyn Leashable>(self)
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

    /// Default for [`Self::is_effective_ai`].
    fn is_effective_ai_default(&self) -> bool {
        self.is_server_driven_movement()
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

    /// Default for [`Self::y_head_rot`].
    fn y_head_rot_default(&self) -> f32 {
        0.0
    }

    /// Default for [`Self::set_y_head_rot`].
    fn set_y_head_rot_default(&self, _y_head_rot: f32) {}

    /// Default for [`Self::spawn_data`].
    fn spawn_data_default(&self) -> i32 {
        0
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

pub(super) fn compute_modified_friction(friction: f32, modifier: f32) -> f32 {
    (1.0 - (1.0 - friction) * modifier).clamp(0.0, 1.0)
}

/// Repositions a direct passenger from the vehicle's attachment point.
///
/// Shared between the [`Entity::position_rider`] default and entity overrides that
/// extend it (e.g. `Chicken` mirroring the rider's body yaw).
pub(crate) fn position_rider_default<E: Entity + ?Sized>(entity: &E, passenger: &dyn Entity) {
    if !entity.has_passenger(passenger) {
        return;
    }

    let riding_position = entity.passenger_riding_position(passenger);
    let vehicle_attachment = passenger.vehicle_attachment_point(entity.as_entity_event_source());
    if let Err(error) = passenger.try_set_position(riding_position - vehicle_attachment) {
        log::debug!(
            "Failed to position passenger {} riding entity {}: {error}",
            passenger.id(),
            entity.id()
        );
    }
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
