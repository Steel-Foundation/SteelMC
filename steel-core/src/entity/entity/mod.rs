use std::collections::BTreeSet;

use super::*;
use crate::entity::{leash::Leashable, pushing::EntityPush};
use rand::{RngExt, rng};
use steel_math::DEGREE_90;
use steel_utils::DowncastTypeKey;
use steel_utils::nbt::{NbtNumeric, nbt_collection_values};

mod hooks;

pub use hooks::{EntityDefaults, EntityHooks, EntityProps};
use hooks::{entity_hook_dispatch, entity_hook_list};

/// Vanilla `Entity.refreshDimensions` small-entity limit: only entities at most
/// this wide and tall (in blocks) get their position fudged after growing.
const FUDGE_SMALL_DIMENSION_LIMIT: f32 = 4.0;
/// Vanilla `Entity.fudgePositionAfterSizeChange` epsilon padding (vanilla `1.0E-6`).
const FUDGE_POSITION_EPSILON: f64 = 1.0e-6;

const MAX_ENTITY_MOTION_COMPONENT: f64 = 10.0;

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
        if value.abs() > MAX_ENTITY_MOTION_COMPONENT {
            0.0
        } else {
            value
        }
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
/// (see [`EntityProps`]). Every other method is `final`. `#[derive(Entity)]`
/// writes the impl:
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

    entity_hook_list!(entity_hook_dispatch);

    /// Returns this entity's component whose downcast key is `key`; see
    /// [`Components::query`](crate::entity::component::Components::query).
    fn erased_component(&self, _key: DowncastTypeKey) -> Option<&dyn ErasedType> {
        None
    }

    /// Default for [`Self::is_always_ticking`].
    final fn is_always_ticking_default(&self) -> bool {
        false
    }

    /// Default for [`Self::broadcast_to_player`].
    final fn broadcast_to_player_default(&self, _player: &Player) -> bool {
        true
    }

    /// Gets the entity's unique network ID (session-local).
    final fn id(&self) -> i32 {
        self.base().id()
    }

    /// Gets the generation counter of this runtime construction of the entity.
    final fn generation(&self) -> EntityGeneration {
        self.base().generation()
    }

    /// Gets the UUID of the entity (persistent identifier).
    final fn uuid(&self) -> Uuid {
        self.base().uuid()
    }

    /// Default for [`Self::scoreboard_name`].
    final fn scoreboard_name_default(&self) -> String {
        self.uuid().to_string()
    }

    /// Default for [`Self::name`].
    final fn name_default(&self) -> TextComponent {
        self.custom_name().map_or_else(
            || entity_type_name(self.entity_type()),
            remove_entity_name_actions,
        )
    }

    /// Default for [`Self::display_name`].
    final fn display_name_default(&self) -> TextComponent {
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
    final fn plain_text_name_default(&self) -> String {
        self.name().to_plain(&DisplayResolutor)
    }

    /// Returns the vanilla-shaped entity NBT used by command predicates.
    ///
    /// Mirrors `NbtPredicate.getEntityTagToCompare`: base and type-specific
    /// `Entity.saveWithoutId` data, passengers, and a player's selected item.
    final fn nbt_for_data_compare(&self) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        let position = self.vehicle().map_or_else(
            || self.position(),
            |vehicle| {
                DVec3::new(
                    vehicle.position().x,
                    self.position().y,
                    vehicle.position().z,
                )
            },
        );
        let velocity = self.velocity();
        let (yaw, pitch) = self.rotation();
        let fire_freeze = self.fire_freeze_state();

        nbt.insert(
            "Pos",
            NbtList::Double(vec![position.x, position.y, position.z]),
        );
        nbt.insert(
            "Motion",
            NbtList::Double(vec![velocity.x, velocity.y, velocity.z]),
        );
        nbt.insert("Rotation", NbtList::Float(vec![yaw, pitch]));
        nbt.insert("fall_distance", self.fall_distance());
        nbt.insert(
            "Fire",
            NbtTag::Short(fire_freeze.remaining_fire_ticks() as i16),
        );
        nbt.insert("Air", NbtTag::Short(self.air_supply() as i16));
        nbt.insert("OnGround", nbt_bool(self.on_ground()));
        nbt.insert("Invulnerable", nbt_bool(self.is_invulnerable()));
        nbt.insert("PortalCooldown", self.portal_cooldown());
        nbt.insert(
            "UUID",
            NbtTag::IntArray(self.uuid().to_int_array().to_vec()),
        );

        if let Some(custom_name) = self.custom_name() {
            nbt.insert("CustomName", custom_name.to_codec_nbt());
        }
        if self.is_custom_name_visible() {
            nbt.insert("CustomNameVisible", nbt_bool(true));
        }
        if self.is_silent() {
            nbt.insert("Silent", nbt_bool(true));
        }
        if self.is_no_gravity() {
            nbt.insert("NoGravity", nbt_bool(true));
        }
        if self.has_glowing_tag() {
            nbt.insert("Glowing", nbt_bool(true));
        }
        if fire_freeze.ticks_frozen() > 0 {
            nbt.insert("TicksFrozen", fire_freeze.ticks_frozen());
        }
        if fire_freeze.has_visual_fire() {
            nbt.insert("HasVisualFire", nbt_bool(true));
        }

        let tags = self.tags();
        if !tags.is_empty() {
            nbt.insert("Tags", NbtList::from(tags));
        }
        let custom_data = self.custom_data();
        if !custom_data.is_empty() {
            nbt.insert("data", NbtTag::Compound(custom_data));
        }

        if let Some(living) = self.as_living_entity() {
            living.save_command_nbt(&mut nbt);
        }
        self.save_additional(&mut nbt);

        if let Some(player) = self.as_player() {
            player.save_command_nbt(&mut nbt);
        }

        let passengers = self
            .passengers()
            .into_iter()
            .filter_map(|passenger| passenger.nbt_for_passenger_save())
            .collect::<Vec<_>>();
        if !passengers.is_empty() {
            nbt.insert("Passengers", NbtList::Compound(passengers));
        }

        if let Some(player) = self.as_player() {
            let inventory = player.inventory.lock();
            let selected_item = inventory.get_selected_item();
            if !selected_item.is_empty() {
                nbt.insert("SelectedItem", selected_item.to_nbt_tag_ref());
            }
        }

        nbt
    }

    /// Returns passenger-save NBT including the entity type id.
    final fn nbt_for_passenger_save(&self) -> Option<NbtCompound> {
        if !self.removal_reason().is_none_or(RemovalReason::should_save)
            || !self.entity_type().can_serialize
        {
            return None;
        }

        let mut nbt = self.nbt_for_data_compare();
        nbt.insert("id", self.entity_type().key.to_string());
        Some(nbt)
    }

    /// Gets the entity's current position.
    final fn position(&self) -> DVec3 {
        self.base().position()
    }

    /// Gets the position to send in this entity's add-entity packet.
    ///
    /// Mirrors vanilla `Entity.getAddEntityPacket()` overloads. Most entities
    /// spawn at their current position; block-attached entities can override
    /// this with the backing block position used by the vanilla packet.
    final fn spawn_position(&self) -> DVec3 {
        self.position()
    }

    /// Gets the entity's current block position.
    final fn block_position(&self) -> BlockPos {
        let position = self.position();
        BlockPos::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        )
    }

    /// Returns the block state this entity's origin currently overlaps.
    final fn in_block_state(&self, world: &World) -> BlockStateId {
        self.base().in_block_state(world)
    }

    /// Gets the entity position used by vanilla movement traces.
    final fn old_position(&self) -> DVec3 {
        self.base().old_position()
    }

    /// Gets the entity's bounding box for collision queries.
    final fn bounding_box(&self) -> WorldAabb {
        self.base().bounding_box()
    }

    /// Returns whether the bounding box shifted by `delta` is free of block
    /// collisions and liquids.
    final fn is_free(&self, delta: DVec3) -> bool {
        let Some(world) = self.level() else {
            return false;
        };

        let target_box = self.bounding_box().translate(delta);
        let collision_world =
            WorldCollisionProvider::for_entity(&world, self.as_entity_event_source());
        if collision_world.has_collision_with_context(
            &target_box.deflate(COLLISION_EPSILON),
            physics_state_for_move(self.as_entity_event_source()).block_collision_context(),
        ) {
            return false;
        }

        !aabb_contains_any_liquid(&world, target_box)
    }

    /// Default for [`Self::blocks_building`].
    final fn blocks_building_default(&self) -> bool {
        false
    }

    /// Default for [`Self::is_pickable`].
    final fn is_pickable_default(&self) -> bool {
        false
    }

    /// Default for [`Self::attackable`].
    final fn attackable_default(&self) -> bool {
        true
    }

    /// Returns whether this entity handles and consumes an attack before normal damage.
    final fn skip_attack_interaction(&self, _source: &SharedEntity) -> bool {
        false
    }

    /// Default for [`Self::is_pushable`].
    final fn is_pushable_default(&self) -> bool {
        false
    }

    /// Default for [`Self::is_pushed_by_fluid`].
    final fn is_pushed_by_fluid_default(&self) -> bool {
        self.entity_type().flags.is_pushed_by_fluid
    }

    /// Default for [`Self::on_above_bubble_column`].
    final fn on_above_bubble_column_default(&self, drag_down: bool, pos: BlockPos) {
        if let Some(projectile) = self.as_projectile() {
            projectile.on_above_bubble_column_projectile(drag_down, pos);
            return;
        }

        self.default_on_above_bubble_column(drag_down, pos);
    }

    /// Applies the base entity's clamped bubble-column surface movement.
    final fn default_on_above_bubble_column(&self, drag_down: bool, pos: BlockPos) {
        if self.is_flying_player() {
            return;
        }

        let velocity = self.velocity();
        let y = if drag_down {
            (velocity.y - BUBBLE_COLUMN_DOWN_ACCELERATION).max(BUBBLE_COLUMN_ABOVE_DOWN_MIN_SPEED)
        } else {
            (velocity.y + BUBBLE_COLUMN_ABOVE_UP_ACCELERATION).min(BUBBLE_COLUMN_ABOVE_UP_MAX_SPEED)
        };
        self.set_velocity(DVec3::new(velocity.x, y, velocity.z));

        if let Some(world) = self.level() {
            world.send_bubble_column_particles(pos);
        }
    }

    /// Default for [`Self::on_inside_bubble_column`].
    final fn on_inside_bubble_column_default(&self, drag_down: bool) {
        if let Some(projectile) = self.as_projectile() {
            projectile.on_inside_bubble_column_projectile(drag_down);
            return;
        }

        self.default_on_inside_bubble_column(drag_down);
    }

    /// Applies the base entity's clamped movement inside a bubble-column.
    final fn default_on_inside_bubble_column(&self, drag_down: bool) {
        if self.is_flying_player() {
            return;
        }

        let velocity = self.velocity();
        let y = if drag_down {
            (velocity.y - BUBBLE_COLUMN_DOWN_ACCELERATION).max(BUBBLE_COLUMN_INSIDE_DOWN_MIN_SPEED)
        } else {
            (velocity.y + BUBBLE_COLUMN_INSIDE_UP_ACCELERATION)
                .min(BUBBLE_COLUMN_INSIDE_UP_MAX_SPEED)
        };
        self.set_velocity(DVec3::new(velocity.x, y, velocity.z));
        self.reset_fall_distance();
    }

    /// Default for [`Self::is_spectator`].
    final fn is_spectator_default(&self) -> bool {
        false
    }

    /// Returns whether this entity is excluded from pressure plates and tripwires.
    ///
    /// Display-like entities and marker entities override this capability.
    final fn is_ignoring_block_triggers(&self) -> bool {
        false
    }

    /// Default for [`Self::explosion_indirect_source`].
    final fn explosion_indirect_source_default(&self) -> Option<SharedEntity> {
        self.projectile_owner()
            .filter(|owner| owner.as_living_entity().is_some())
    }

    /// Default for [`Self::explosion_damage_origin`].
    final fn explosion_damage_origin_default(&self) -> DVec3 {
        let position = self.position();
        DVec3::new(position.x, self.get_eye_y(), position.z)
    }

    /// Default for [`Self::ignore_explosion`].
    final fn ignore_explosion_default(&self, _explosion: &dyn Explosion) -> bool {
        false
    }

    /// Default for [`Self::block_explosion_resistance`].
    #[expect(
        unused_variables,
        reason = "default implementation preserves resistance"
    )]
    final fn block_explosion_resistance_default(
        &self,
        explosion: &dyn Explosion,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        fluid: FluidState,
        resistance: f32,
    ) -> f32 {
        resistance
    }

    /// Default for [`Self::should_block_explode`].
    #[expect(unused_variables, reason = "default implementation allows destruction")]
    final fn should_block_explode_default(
        &self,
        explosion: &dyn Explosion,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        power: f32,
    ) -> bool {
        true
    }

    /// Default for [`Self::on_explosion_hit`].
    final fn on_explosion_hit_default(&self, _explosion_source: Option<&dyn Entity>) {}

    /// Returns how vanilla lets this entity respond to piston movement.
    final fn piston_push_reaction(&self) -> PushReaction {
        if self.is_marker_armor_stand() {
            PushReaction::IgnoreEntity
        } else {
            self.entity_type().flags.piston_push_reaction
        }
    }

    /// Returns whether this entity's main supporting block is `pos`.
    final fn is_supported_by(&self, pos: BlockPos) -> bool {
        self.base().supporting_block() == Some(pos)
    }

    /// Returns whether vanilla lets this entity interact with its loaded level.
    final fn can_interact_with_level(&self) -> bool {
        self.is_alive() && !self.is_removed() && !self.is_spectator()
    }

    /// Returns whether this entity is invisible, per its synced entity data.
    final fn is_invisible(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_base_invisible_flag)
    }

    /// Returns whether this entity is sneaking (shift key down), per its synced entity data.
    final fn is_discrete(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_shift_key_down)
    }

    /// Returns whether this entity is allied to `other`.
    final fn is_allied_to(&self, _other: &dyn Entity) -> bool {
        false
    }

    /// Returns whether this entity is a marker armor stand.
    final fn is_marker_armor_stand(&self) -> bool {
        false
    }

    /// Returns whether this entity is a tameable animal owned by `owner`.
    final fn is_tame_owned_by(&self, _owner: &Living<'_>) -> bool {
        false
    }

    /// Default for [`Self::projectile_owner_uuid`].
    final fn projectile_owner_uuid_default(&self) -> Option<Uuid> {
        None
    }

    /// Returns the live vanilla `Projectile` owner when this entity exposes one.
    final fn projectile_owner(&self) -> Option<SharedEntity> {
        None
    }

    /// Default for [`Self::is_flying_player`].
    final fn is_flying_player_default(&self) -> bool {
        false
    }

    /// Default for [`Self::can_be_collided_with`].
    final fn can_be_collided_with_default(&self, _other: Option<&dyn Entity>) -> bool {
        false
    }

    /// Default for [`Self::is_hard_collision_relevant`].
    final fn is_hard_collision_relevant_default(&self) -> bool {
        true
    }

    /// Default for [`Self::can_be_hit_by_projectile`].
    final fn can_be_hit_by_projectile_default(&self) -> bool {
        !self.is_removed() && self.is_pickable()
    }

    /// Returns this entity's Vanilla projectile deflection behavior.
    final fn deflection(&self, _projectile: &dyn Projectile) -> ProjectileDeflection {
        if REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::DEFLECTS_PROJECTILES)
        {
            ProjectileDeflection::Reverse
        } else {
            ProjectileDeflection::None
        }
    }

    /// Gets the vehicle this entity is riding, if present.
    final fn vehicle(&self) -> Option<SharedEntity> {
        self.base().vehicle()
    }

    /// Returns the vehicle this entity directly controls, if any.
    final fn controlled_vehicle(&self) -> Option<SharedEntity> {
        let vehicle = self.vehicle()?;
        let controlled_by_self = vehicle
            .controlling_passenger()
            .is_some_and(|passenger| passenger.id() == self.id());
        controlled_by_self.then_some(vehicle)
    }

    /// Returns whether this entity is riding another entity.
    final fn is_passenger(&self) -> bool {
        self.vehicle().is_some()
    }

    /// Returns whether vanilla allows this entity to start riding `vehicle`.
    final fn can_ride(&self, _vehicle: &dyn Entity) -> bool {
        !self.is_discrete() && self.base().boarding_cooldown() <= 0
    }

    /// Default for [`Self::stop_riding`].
    final fn stop_riding_default(&self) {
        self.base().stop_riding();
    }

    /// Default for [`Self::start_riding`].
    final fn start_riding_default(&self, entity_to_ride: &SharedEntity) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        let Some(passenger) = world.get_entity_by_id(self.id()) else {
            return false;
        };
        start_riding_entities(&passenger, entity_to_ride)
    }

    /// Gets this entity's direct passengers.
    final fn passengers(&self) -> Vec<SharedEntity> {
        self.base().passengers()
    }

    /// Counts indirect player passengers.
    final fn count_player_passengers(&self) -> usize {
        fn count_passenger_tree(
            passengers: Vec<SharedEntity>,
            visited: &mut FxHashSet<i32>,
        ) -> usize {
            let mut total = 0;
            for passenger in passengers {
                if !visited.insert(passenger.id()) {
                    continue;
                }
                if passenger.entity_type() == &vanilla_entities::PLAYER {
                    total += 1;
                }
                total += count_passenger_tree(passenger.passengers(), visited);
            }
            total
        }

        let mut visited = FxHashSet::default();
        visited.insert(self.id());
        count_passenger_tree(self.passengers(), &mut visited)
    }

    /// Returns whether this entity has exactly one indirect player passenger.
    final fn has_exactly_one_player_passenger(&self) -> bool {
        self.count_player_passengers() == 1
    }

    /// Gets this entity's first direct passenger.
    final fn first_passenger(&self) -> Option<SharedEntity> {
        self.base().first_passenger()
    }

    /// Default for [`Self::controlling_passenger`].
    final fn controlling_passenger_default(&self) -> Option<SharedEntity> {
        None
    }

    /// Returns whether this entity can control a vehicle it is riding.
    final fn can_control_vehicle(&self) -> bool {
        !REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::NON_CONTROLLING_RIDER)
    }

    /// Returns whether this entity currently has a controlling passenger.
    final fn has_controlling_passenger(&self) -> bool {
        self.controlling_passenger().is_some()
    }

    /// Returns whether this entity has any direct passengers.
    final fn is_vehicle(&self) -> bool {
        self.base().is_vehicle()
    }

    /// Returns whether this entity's passengers are forced to dismount when
    /// it goes underwater, per the `dismounts_underwater` entity type tag.
    final fn dismounts_underwater(&self) -> bool {
        REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::DISMOUNTS_UNDERWATER)
    }

    /// Returns whether `passenger` is a direct passenger of this entity.
    final fn has_passenger(&self, passenger: &dyn Entity) -> bool {
        self.base().has_passenger_id(passenger.id())
    }

    /// Default for [`Self::can_add_passenger`].
    final fn can_add_passenger_default(&self, _passenger: &dyn Entity) -> bool {
        self.passengers().is_empty()
    }

    /// Returns whether this entity can accept any passenger.
    final fn could_accept_passenger(&self) -> bool {
        true
    }

    /// Returns the current direct passenger index for attachment lookup.
    final fn passenger_index(&self, passenger: &dyn Entity) -> Option<usize> {
        self.passengers()
            .iter()
            .position(|entity| entity.id() == passenger.id())
    }

    /// Returns this passenger's vehicle attachment point.
    final fn vehicle_attachment_point(&self, _vehicle: &dyn Entity) -> DVec3 {
        let dimensions = self.base().dimensions();
        dimensions.attachments.get_clamped(
            EntityAttachment::Vehicle,
            0,
            self.rotation().0,
            dimensions,
        )
    }

    /// Returns this vehicle's passenger attachment point.
    ///
    /// Mirrors vanilla `Entity.getPassengerAttachmentPoint` for the base entity class.
    final fn passenger_attachment_point(&self, passenger: &dyn Entity) -> DVec3 {
        let dimensions = self.base().dimensions();
        let passenger_index = self.passenger_index(passenger).unwrap_or_default();
        dimensions.attachments.get_clamped(
            EntityAttachment::Passenger,
            passenger_index,
            self.rotation().0,
            dimensions,
        )
    }

    /// Returns the world position where `passenger` should ride this vehicle.
    final fn passenger_riding_position(&self, passenger: &dyn Entity) -> DVec3 {
        self.position() + self.passenger_attachment_point(passenger)
    }

    /// Default for [`Self::position_rider`].
    final fn position_rider_default(&self, passenger: &dyn Entity) {
        position_rider_default(self, passenger);
    }

    /// Returns this entity's root vehicle ID, or this entity's ID when it is not riding.
    ///
    /// Mirrors vanilla `Entity.getRootVehicle` using session IDs for object identity.
    final fn root_vehicle_id(&self) -> i32 {
        self.root_vehicle().map_or(self.id(), |entity| entity.id())
    }

    /// Returns this entity's root vehicle, if this entity is riding one.
    final fn root_vehicle(&self) -> Option<SharedEntity> {
        let mut root = self.vehicle()?;
        let mut visited = FxHashSet::default();
        visited.insert(self.id());

        loop {
            if !visited.insert(root.id()) {
                return Some(root);
            }
            let Some(next) = root.vehicle() else {
                return Some(root);
            };
            root = next;
        }
    }

    /// Returns whether this entity and `other` share the same root vehicle.
    final fn is_passenger_of_same_vehicle(&self, other: &dyn Entity) -> bool {
        self.root_vehicle_id() == other.root_vehicle_id()
    }

    /// Returns whether `entity` is an indirect passenger of this entity.
    final fn has_indirect_passenger(&self, entity: &dyn Entity) -> bool {
        let target_id = self.id();
        let mut vehicle = entity.vehicle();
        let mut visited = Vec::new();

        while let Some(ridden) = vehicle {
            let ridden_id = ridden.id();
            if ridden_id == target_id {
                return true;
            }
            if visited.contains(&ridden_id) {
                return false;
            }
            visited.push(ridden_id);
            vehicle = ridden.vehicle();
        }

        false
    }

    /// Default for [`Self::can_collide_with`].
    final fn can_collide_with_default(&self, other: &dyn Entity) -> bool {
        other.can_be_collided_with(Some(self.as_entity_event_source()))
            && !self.is_passenger_of_same_vehicle(other)
    }

    /// Opts this entity into the [`DefaultEntityPush`] fast path. It is read under the
    /// entity manager lock, so it must give the same answer for the entity's whole
    /// lifetime and have no side effects.
    fn as_default_entity_push(&self) -> Option<&dyn DefaultEntityPush> {
        try_as_dyn::<Self, dyn DefaultEntityPush>(self)
    }

    /// Adds an impulse to this entity's velocity and marks velocity for sync.
    final fn push_impulse(&self, impulse: DVec3) {
        self.base().push_impulse(impulse);
    }

    /// Default for [`Self::push_entity`].
    final fn push_entity_default(&self, entity: &dyn Entity) {
        if self.is_passenger_of_same_vehicle(entity) || entity.no_physics() || self.no_physics() {
            return;
        }

        let other_position = entity.position();
        let position = self.position();

        let Some(impulse) = EntityPush::impulse(position, other_position) else {
            return;
        };

        if !self.is_vehicle() && self.is_pushable() {
            self.push_impulse(DVec3::new(-impulse.x, 0.0, -impulse.z));
        }

        if !entity.is_vehicle() && entity.is_pushable() {
            entity.push_impulse(impulse);
        }
    }

    /// Builds this entity's default bounding box at `position`.
    final fn make_bounding_box_at(&self, position: DVec3) -> WorldAabb {
        let dimensions = self.base().dimensions();
        WorldAabb::entity_box(
            position.x,
            position.y,
            position.z,
            f64::from(dimensions.half_width()),
            f64::from(dimensions.height),
        )
    }

    /// Default for [`Self::get_relative_portal_position`].
    final fn get_relative_portal_position_default(
        &self,
        axis: Axis,
        portal_area: FoundRectangle,
    ) -> DVec3 {
        PortalShape::get_relative_position(
            portal_area,
            axis,
            self.position(),
            self.dimensions_for_pose(self.pose()),
        )
    }

    /// Default for [`Self::tick`]: vanilla `Entity.tick`, which only runs
    /// [`Self::base_tick`].
    final fn tick_default(&self) {
        self.base_tick();
    }

    /// Default for [`Self::ride_tick`].
    final fn ride_tick_default(self: Arc<Self>) {
        self.set_velocity(DVec3::ZERO);
        Arc::clone(&self).tick();
        if let Some(vehicle) = self.vehicle() {
            vehicle.position_rider(self.as_entity_event_source());
        }
    }

    /// Default for [`Self::base_tick`].
    final fn base_tick_default(&self) {
        self.entity_base_tick();
    }

    /// Runs vanilla `Entity.handlePortal` behavior currently implemented by Steel.
    final fn handle_portal(&self) {
        self.base().process_portal_cooldown();
        let Some(world) = self.level() else {
            return;
        };
        let Some(process) = self.base().portal_process() else {
            return;
        };

        let player_invulnerable = self
            .as_player()
            .map(|player| player.abilities.lock().invulnerable);
        let transition_time = process
            .portal()
            .transition_time_for_player_state(&world, player_invulnerable);
        match self
            .base()
            .process_portal_teleportation(self.can_use_portal(false), transition_time)
        {
            Some(PortalProcessResult::Ready) => {
                let Some(pending_token) = self.begin_pending_world_change() else {
                    return;
                };
                let Some(entity) = world.get_entity_by_id(self.id()) else {
                    self.finish_pending_world_change(pending_token);
                    return;
                };
                self.reset_portal_cooldown();
                world.queue_world_change(
                    entity,
                    WorldChangeRequest::Portal {
                        portal: process.portal(),
                        source_world: Arc::clone(&world),
                        portal_pos: process.entry_position(),
                        pending_token,
                    },
                );
            }
            Some(PortalProcessResult::Waiting)
                if self
                    .base()
                    .portal_process()
                    .is_some_and(PortalProcessor::has_expired) =>
            {
                self.base().clear_portal_process();
            }
            Some(PortalProcessResult::Waiting) | None => {}
        }
    }

    /// Runs only vanilla `Entity.baseTick` behavior.
    ///
    /// Subtype base-tick chains call this from their owner trait.
    final fn entity_base_tick(&self) {
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
    final fn check_below_world(&self) {
        let Some(world) = self.level() else {
            return;
        };

        if self.position().y < f64::from(world.get_min_y() - 64) {
            self.on_below_world();
        }
    }

    /// Default for [`Self::on_below_world`].
    final fn on_below_world_default(&self) {
        self.set_removed(RemovalReason::Discarded);
    }

    /// Default for [`Self::check_despawn`].
    final fn check_despawn_default(&self) {}

    /// Default for [`Self::apply_inside_block_effect`].
    final fn apply_inside_block_effect_default(&self, effect_type: InsideBlockEffectType) {
        let fire_ignite_extra_ticks = if matches!(effect_type, InsideBlockEffectType::FireIgnite) {
            self.fire_ignite_extra_ticks()
        } else {
            0
        };
        self.base().apply_inside_block_effect(
            effect_type,
            self.can_freeze(),
            self.fire_immune(),
            fire_ignite_extra_ticks,
            self.ticks_required_to_freeze(),
            self.remaining_fire_ticks_cap(),
        );
        self.sync_base_fire_freeze_entity_data();
    }

    /// Gets the world this entity is in.
    ///
    /// Returns `None` if the entity is not in a world or the world was dropped.
    final fn level(&self) -> Option<Arc<World>> {
        self.base().level()
    }

    /// Packs dirty entity data for network synchronization.
    ///
    /// Returns `Some(values)` if there are dirty values to sync, `None` otherwise.
    /// Clears the dirty flags after packing.
    final fn pack_dirty_entity_data(&self) -> Option<Vec<DataValue>> {
        self.synced_data().and_then(EntitySyncedData::pack_dirty)
    }

    /// Packs all non-default entity data for initial spawn.
    ///
    /// Used when sending entity data to a player who just started tracking this entity.
    final fn pack_all_entity_data(&self) -> Vec<DataValue> {
        self.synced_data()
            .map_or_else(Vec::new, EntitySyncedData::pack_all)
    }

    /// Returns the synchronized entity-data container for entities with vanilla data accessors.
    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        None
    }

    /// Default for [`Self::update_data_before_sync`].
    final fn update_data_before_sync_default(&self) {}

    /// Returns true if the entity has been marked for removal.
    final fn is_removed(&self) -> bool {
        self.base().is_removed()
    }

    /// Default for [`Self::is_alive`].
    final fn is_alive_default(&self) -> bool {
        !self.is_removed()
    }

    /// Marks this entity as waiting for a prepared world change.
    final fn begin_pending_world_change(&self) -> Option<PendingWorldChangeToken> {
        self.base().begin_pending_world_change()
    }

    /// Clears a pending world change if it still matches the provided token.
    final fn finish_pending_world_change(&self, token: PendingWorldChangeToken) -> bool {
        self.base().finish_pending_world_change(token)
    }

    /// Returns true while this entity is waiting for a prepared world change.
    final fn is_world_change_pending(&self) -> bool {
        self.base().is_world_change_pending()
    }

    /// Returns true if the given world-change token is still pending.
    final fn is_world_change_token_pending(&self, token: PendingWorldChangeToken) -> bool {
        self.base().is_world_change_token_pending(token)
    }

    /// Default for [`Self::can_use_portal`].
    final fn can_use_portal_default(&self, ignore_passenger: bool) -> bool {
        let entity_type = self.entity_type();
        if entity_type == &vanilla_entities::FISHING_BOBBER
            || entity_type == &vanilla_entities::ENDER_DRAGON
            || entity_type == &vanilla_entities::WITHER
        {
            return false;
        }

        (ignore_passenger || !self.is_passenger()) && self.is_alive()
    }

    /// Default for [`Self::dimension_changing_delay`].
    final fn dimension_changing_delay_default(&self) -> i32 {
        // TODO: move to the projectile preset once projectiles are ported.
        if self.entity_type().is_projectile {
            return 2;
        }
        if let Some(first_passenger) = self.first_passenger()
            && first_passenger.as_player().is_some()
        {
            return first_passenger.dimension_changing_delay();
        }
        300
    }

    /// Returns why this entity was removed, if it has been removed.
    final fn removal_reason(&self) -> Option<RemovalReason> {
        self.base().removal_reason()
    }

    /// Marks the entity as removed with the given reason.
    final fn set_removed(&self, reason: RemovalReason) {
        self.base().set_removed(reason);
    }

    /// Emits a vanilla game event from this entity's exact position with an explicit source entity.
    final fn game_event_with_source_entity(
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
    final fn game_event(&self, event: GameEventRef) {
        self.game_event_with_source_entity(event, Some(self.as_entity_event_source()));
    }

    /// Default for [`Self::kill`].
    final fn kill_default(&self, _world: &World) {
        self.set_removed(RemovalReason::Killed);
        self.game_event(&vanilla_game_events::ENTITY_DIE);
    }

    /// Caches a live owner reference after restoring persisted owner-linked
    /// entities. Most entities do not store owner references.
    final fn restore_owner_reference(&self, _owner: &SharedEntity) {}

    /// Default for [`Self::on_teleported`].
    final fn on_teleported_default(&self) {}

    /// Sets the level callback for lifecycle events (movement, removal).
    final fn set_level_callback(&self, callback: Arc<dyn EntityLevelCallback>) {
        self.base().set_level_callback(callback);
    }

    /// Default for [`Self::notify_leash_holder`].
    final fn notify_leash_holder_default(&self, _leashable: &dyn Entity) {}

    /// Default for [`Self::notify_leashee_removed`].
    final fn notify_leashee_removed_default(&self, _leashable: &dyn Entity) {}

    /// Default for [`Self::player_touch`].
    final fn player_touch_default(&self, _player: &Arc<Player>) {}

    /// Finds leashable mobs in vanilla's nearby leash scan whose holder is this entity.
    final fn leashables_leashed_to(&self) -> Vec<SharedEntity> {
        self.leashables_leashed_to_holder_in_area(self.as_entity_event_source())
    }

    /// Finds leashable mobs in this entity's nearby leash scan whose holder is `holder`.
    final fn leashables_leashed_to_holder_in_area(&self, holder: &dyn Entity) -> Vec<SharedEntity> {
        let Some(world) = self.level() else {
            return Vec::new();
        };
        leashables_leashed_to_holder_in_area_near_position(
            &world,
            world_aabb_center(self.bounding_box()),
            holder,
        )
    }

    /// Transfers leashables currently held by `old_holder` to this entity.
    final fn transfer_leashables_from_holder(&self, old_holder: &dyn Entity) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        let Some(new_holder) = world.get_entity_by_id(self.id()) else {
            return false;
        };

        transfer_leashables_to_holder(
            self.leashables_leashed_to_holder_in_area(old_holder),
            &new_holder,
        )
    }

    /// Drops this entity's leash and all leashables attached to it.
    final fn drop_all_leash_connections(&self, player: Option<&Player>) -> bool {
        let leashables = self.leashables_leashed_to();
        let mut dropped = !leashables.is_empty();

        if let Some(mob) = self.as_leashable()
            && mob.is_leashed()
        {
            mob.drop_leash();
            dropped = true;
        }

        for leashable in leashables {
            if let Some(mob) = leashable.as_leashable() {
                mob.drop_leash();
            }
        }

        if !dropped {
            return false;
        }

        let source_entity = player.map(|player| player as &dyn Entity);
        self.game_event_with_source_entity(&vanilla_game_events::SHEAR, source_entity);
        true
    }

    /// Shears off all leash connections reachable from this entity.
    final fn shear_off_all_leash_connections(&self, player: Option<&Player>) -> bool {
        if !self.drop_all_leash_connections(player) {
            return false;
        }

        if let Some(world) = self.level() {
            let sound_source = player.map_or_else(|| self.sound_source(), Entity::sound_source);
            world.play_sound(
                &sound_events::ITEM_SHEARS_SNIP,
                sound_source,
                self.block_position(),
                1.0,
                1.0,
                None,
            );
        }
        true
    }

    /// Shears the first shearable equipped item on this mob, spawning it as a
    /// dropped item and playing its shearing sound; returns whether anything was sheared.
    final fn attempt_to_shear_equipment(&self, player: &Player, hand: InteractionHand) -> bool {
        let Some(mob) = self.as_mob() else {
            return false;
        };
        if !mob.can_shear_equipment(player) || player.is_secondary_use_active() {
            return false;
        }

        let has_infinite_materials = player.has_infinite_materials();
        for slot in EquipmentSlot::ALL {
            let sheared = {
                let mut equipment = mob.living_base().equipment().lock();
                let item_stack = equipment.get_ref(slot);
                let Some(equippable) = item_stack.get_equippable() else {
                    continue;
                };
                if !equippable.can_be_sheared
                    || !has_infinite_materials
                        && item_stack
                            .has_enchantment_effect(EnchantmentEffectComponent::PreventArmorChange)
                {
                    continue;
                }

                let shearing_sound = equippable.shearing_sound.registry_ref();
                (equipment.take(slot), shearing_sound)
            };
            let (item_stack, shearing_sound) = sheared;
            if item_stack.is_empty() {
                continue;
            }

            player
                .inventory
                .lock()
                .hurt_item_in_hand(hand, 1, has_infinite_materials);
            mob.set_guaranteed_drop(slot);
            mob.set_persistence_required();

            self.game_event_with_source_entity(&vanilla_game_events::SHEAR, Some(player));
            if let Some(shearing_sound) = shearing_sound {
                self.play_sound(shearing_sound, 1.0, 1.0);
            }

            let dimensions = self.base().dimensions();
            let spawn_offset = dimensions
                .attachments
                .get_average(EntityAttachment::Passenger, dimensions);
            let _ = self.spawn_at_location_with_offset(item_stack, spawn_offset);
            // TODO: Trigger PLAYER_SHEARED_EQUIPMENT once advancement criteria exist.
            return true;
        }

        false
    }

    /// Default for [`Self::interact`]: leashing/unleashing, cutting leash
    /// connections, or shearing equipment with shears.
    final fn interact_default(
        &self,
        player: &Player,
        hand: InteractionHand,
        _location: DVec3,
    ) -> InteractionResult {
        let Some(mob) = self.as_mob() else {
            return InteractionResult::Pass;
        };
        let is_alive = self
            .as_living_entity()
            .is_none_or(|living| living.is_alive());
        if !is_alive {
            return InteractionResult::Pass;
        }

        if player.is_secondary_use_active()
            && mob.leashable().can_be_leashed()
            && self
                .as_living_entity()
                .is_none_or(|living| !living.is_baby())
            && self.transfer_leashables_from_holder(player)
        {
            if let Some(world) = self.level() {
                world.game_event(
                    &vanilla_game_events::ENTITY_ACTION,
                    self.block_position(),
                    &GameEventContext::new(Some(player), None),
                );
            }
            self.play_sound(&sound_events::ITEM_LEAD_TIED, 1.0, 1.0);
            return InteractionResult::SuccessServer;
        }

        let holding_shears = {
            let inventory = player.inventory.lock();
            inventory.get_item_in_hand(hand).is(&vanilla_items::SHEARS)
        };
        if holding_shears && self.shear_off_all_leash_connections(Some(player)) {
            let has_infinite_materials = player.has_infinite_materials();
            player
                .inventory
                .lock()
                .hurt_item_in_hand(hand, 1, has_infinite_materials);
            return InteractionResult::Success;
        }
        if holding_shears && self.attempt_to_shear_equipment(player, hand) {
            return InteractionResult::Success;
        }

        if let Some(holder) = mob.leashable().leash_holder() {
            if holder.id() == player.id() {
                if player.has_infinite_materials() {
                    mob.leashable().remove_leash();
                } else {
                    mob.leashable().drop_leash();
                }

                self.game_event_with_source_entity(
                    &vanilla_game_events::ENTITY_INTERACT,
                    Some(player),
                );
                self.play_sound(&sound_events::ITEM_LEAD_UNTIED, 1.0, 1.0);
                return InteractionResult::Success;
            }

            if holder.as_player().is_some() {
                return InteractionResult::Pass;
            }
        }

        let holding_lead = {
            let inventory = player.inventory.lock();
            inventory.get_item_in_hand(hand).is(&vanilla_items::LEAD)
        };
        if !holding_lead || !mob.leashable().can_have_a_leash_attached_to(player) {
            return InteractionResult::Pass;
        }

        let Some(world) = self.level() else {
            return InteractionResult::Pass;
        };
        let Some(player_entity) = world.get_entity_by_id(player.id()) else {
            return InteractionResult::Pass;
        };

        if mob.leashable().is_leashed() {
            mob.leashable().drop_leash();
        }
        if !mob.leashable().set_leashed_to(&player_entity) {
            return InteractionResult::Pass;
        }

        self.play_sound(&sound_events::ITEM_LEAD_TIED, 1.0, 1.0);
        player.inventory.lock().shrink_item_in_hand(hand, 1);
        InteractionResult::SuccessServer
    }

    /// Returns true for entities that implement vanilla living-entity behavior.
    final fn is_living_entity(&self) -> bool {
        self.as_living_entity().is_some()
    }

    /// Returns whether this entity belongs to vanilla's `AbstractArrow` hierarchy.
    final fn is_abstract_arrow(&self) -> bool {
        self.entity_type().is_abstract_arrow
    }

    /// Returns this entity as a projectile when it has projectile behavior.
    fn as_projectile(&self) -> Option<&dyn Projectile> {
        try_as_dyn::<Self, dyn Projectile>(self)
    }

    /// Returns this entity as a living entity when it has living behavior.
    final fn as_living_entity(&self) -> Option<Living<'_>> {
        Living::of(self.as_entity_event_source())
    }

    /// Returns this entity as an item frame when it has item-frame behavior.
    fn as_item_frame(&self) -> Option<&dyn ItemFrame> {
        try_as_dyn::<Self, dyn ItemFrame>(self)
    }

    /// Returns this entity as a player when it is the concrete server player.
    final fn as_player(&self) -> Option<&Player> {
        self.downcast_ref::<Player>()
    }

    /// Default for [`Self::with_weapon_item`].
    final fn with_weapon_item_default(&self, visitor: &mut dyn FnMut(Option<&ItemStack>)) {
        visitor(None);
    }

    /// Returns true for mobs with pathfinding navigation.
    final fn is_pathfinder_mob(&self) -> bool {
        self.as_pathfinder_mob().is_some()
    }

    /// Returns this entity as a pathfinder mob when it has pathfinding behavior.
    final fn as_pathfinder_mob(&self) -> Option<Mob<'_>> {
        self.as_mob().filter(Mob::is_pathfinder)
    }

    /// Returns true for entities that implement vanilla mob behavior.
    final fn is_mob(&self) -> bool {
        self.as_mob().is_some()
    }

    /// Returns this entity as a mob when it has mob behavior.
    final fn as_mob(&self) -> Option<Mob<'_>> {
        Mob::of(self.as_entity_event_source())
    }

    /// Returns this entity as a `Leashable` if it has [`Leashable`] behavior.
    fn as_leashable(&self) -> Option<&dyn Leashable> {
        try_as_dyn::<Self, dyn Leashable>(self)
    }

    /// Whether this entity implements the `Enemy` marker.
    final fn is_enemy(&self) -> bool {
        self.as_enemy().is_some()
    }

    /// Returns the `Enemy` marker, equivalent to vanilla's `instanceof Enemy`.
    fn as_enemy(&self) -> Option<&dyn Enemy> {
        try_as_dyn::<Self, dyn Enemy>(self)
    }

    /// Returns true for entities that implement vanilla item-steered boosts.
    final fn is_item_steerable(&self) -> bool {
        self.as_item_steerable().is_some()
    }

    /// Returns this entity as item steerable when it has item-steering behavior.
    fn as_item_steerable(&self) -> Option<&dyn ItemSteerable> {
        try_as_dyn::<Self, dyn ItemSteerable>(self)
    }

    /// Returns whether this entity's velocity should sync every tick because
    /// it's currently fall-flying with an elytra.
    final fn forces_fall_flying_velocity_sync(&self) -> bool {
        self.as_living_entity()
            .is_some_and(|living| living.is_fall_flying())
    }

    /// Default for [`Self::uses_client_movement_packets`].
    final fn uses_client_movement_packets_default(&self) -> bool {
        if !self.is_removed()
            && let Some(controller) = self.controlling_passenger()
            && controller.id() != self.id()
            && controller.uses_client_movement_packets()
        {
            return true;
        }

        false
    }

    /// Returns true when normal server ticks drive this entity's movement.
    final fn is_server_driven_movement(&self) -> bool {
        !self.uses_client_movement_packets()
    }

    /// Default for [`Self::can_simulate_movement`].
    final fn can_simulate_movement_default(&self) -> bool {
        self.is_server_driven_movement()
    }

    /// Default for [`Self::is_effective_ai`].
    final fn is_effective_ai_default(&self) -> bool {
        self.is_server_driven_movement()
    }

    /// Default for [`Self::is_suppressing_bounce`].
    final fn is_suppressing_bounce_default(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_shift_key_down)
    }

    /// Default for [`Self::is_stepping_carefully`].
    final fn is_stepping_carefully_default(&self) -> bool {
        self.is_suppressing_bounce()
    }

    /// Returns true when vanilla collision context should treat the entity as descending.
    final fn is_descending(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_shift_key_down)
    }

    /// Sets the vanilla shift-key-down shared flag.
    final fn set_shared_shift_key_down(&self, shift_key_down: bool) {
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_shift_key_down(shift_key_down);
        }
    }

    /// Sets the vanilla swimming shared flag.
    final fn set_shared_swimming(&self, swimming: bool) {
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_swimming(swimming);
        }
    }

    /// Default for [`Self::update_swimming`].
    final fn update_swimming_default(&self) {
        let Some(world) = self.level() else {
            return;
        };

        let block_fluid = get_fluid_state(&world, self.block_position());
        let swimming = select_swimming_state(
            self.is_swimming(),
            SwimmingEnvironment {
                sprinting: self
                    .as_living_entity()
                    .is_some_and(|living| living.is_sprinting()),
                passenger: self.is_passenger(),
                in_water: self.is_in_water(),
                under_water: self.is_under_water(),
                block_fluid_is_water: block_fluid.is_water(),
            },
        );
        self.set_shared_swimming(swimming);
    }

    /// Sets the vanilla sprinting shared flag.
    final fn set_shared_sprinting(&self, sprinting: bool) {
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_sprinting(sprinting);
        }
    }

    /// Sets the vanilla fall-flying shared flag.
    final fn set_shared_fall_flying(&self, fall_flying: bool) {
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_fall_flying(fall_flying);
        }
    }

    /// Returns whether vanilla excludes this vehicle from floating kicks.
    final fn is_flying_vehicle(&self) -> bool {
        false
    }

    /// Default for [`Self::on_climbable`].
    final fn on_climbable_default(&self) -> bool {
        false
    }

    /// Default for [`Self::known_movement`].
    final fn known_movement_default(&self) -> DVec3 {
        if let Some(controller) = self.controlling_passenger()
            && !self.is_removed()
            && controller.entity_type() == &vanilla_entities::PLAYER
        {
            return controller.known_movement();
        }

        self.velocity()
    }

    /// Default for [`Self::known_speed`].
    final fn known_speed_default(&self) -> DVec3 {
        if let Some(controller) = self.controlling_passenger()
            && !self.is_removed()
            && controller.entity_type() == &vanilla_entities::PLAYER
        {
            return controller.known_speed();
        }

        self.base().known_speed()
    }

    /// Returns the number of ticks this entity has existed for.
    final fn tick_count(&self) -> i32 {
        self.base().tick_count()
    }

    /// Returns whether this entity has not completed its first tick.
    final fn is_first_tick(&self) -> bool {
        self.base().is_first_tick()
    }

    /// Sets whether this entity has not completed its first tick.
    final fn set_first_tick(&self, first_tick: bool) {
        self.base().set_first_tick(first_tick);
    }

    /// Advances this entity's tick counter by one.
    final fn advance_tick_count(&self) {
        self.base().advance_tick_count();
    }

    /// Default for [`Self::fall_sounds`].
    final fn fall_sounds_default(&self) -> (SoundEventRef, SoundEventRef) {
        (
            &sound_events::ENTITY_GENERIC_SMALL_FALL,
            &sound_events::ENTITY_GENERIC_BIG_FALL,
        )
    }

    /// Gets the entity's rotation as (yaw, pitch) in degrees.
    ///
    /// Yaw is horizontal rotation (0-360), pitch is vertical (-90 to 90).
    final fn rotation(&self) -> (f32, f32) {
        self.base().rotation()
    }

    /// Sets the entity's rotation as (yaw, pitch) in degrees.
    final fn set_rotation(&self, rotation: (f32, f32)) {
        self.base().set_rotation(rotation);
    }

    /// Gets the nearest horizontal direction to the entity's yaw (horizontal rotation).
    final fn direction_yaw(&self) -> Direction {
        let (yaw, _) = self.rotation();
        Direction::from_yaw(yaw)
    }

    /// Default for [`Self::look_at`].
    final fn look_at_default(&self, from_anchor: EntityAnchor, target: DVec3) {
        apply_entity_look_at(self.as_entity_event_source(), from_anchor, target);
    }

    /// Default for [`Self::look_at_entity`].
    final fn look_at_entity_default(
        &self,
        from_anchor: EntityAnchor,
        target: &dyn Entity,
        target_anchor: EntityAnchor,
    ) {
        self.look_at(from_anchor, target_anchor.position(target));
    }

    /// Default for [`Self::y_head_rot`].
    final fn y_head_rot_default(&self) -> f32 {
        0.0
    }

    /// Default for [`Self::set_y_head_rot`].
    final fn set_y_head_rot_default(&self, _y_head_rot: f32) {}

    /// Default for [`Self::set_y_body_rot`].
    final fn set_y_body_rot_default(&self, _y_body_rot: f32) {}

    /// Default for [`Self::spawn_data`].
    final fn spawn_data_default(&self) -> i32 {
        0
    }

    /// Gets the eye height for this entity.
    ///
    /// Default implementation returns the eye height from the entity type dimensions.
    /// Override for entities with pose-dependent eye heights (e.g., players).
    final fn get_eye_height(&self) -> f64 {
        f64::from(self.base().dimensions().eye_height)
    }

    /// Returns the fluid-surface height threshold used for fluid-jump
    /// physics: 0.0 for entities with eye height under 0.4, otherwise 0.4.
    final fn get_fluid_jump_threshold(&self) -> f64 {
        if self.get_eye_height() < 0.4 {
            0.0
        } else {
            0.4
        }
    }

    /// `Entity.getRandomX`: X up to `spread` bounding-box widths either side of the center.
    final fn get_random_x(&self, spread: f64) -> f64 {
        let width = f64::from(self.base().dimensions().width);
        self.position().x + width * (2.0 * rng().random::<f64>() - 1.0) * spread
    }

    /// `Entity.getRandomY()`: Y anywhere within the bounding box height.
    final fn get_random_y(&self) -> f64 {
        let height = f64::from(self.base().dimensions().height);
        self.position().y + height * rng().random::<f64>()
    }

    /// `Entity.getRandomY(spread)`: Y up to `spread` bounding-box heights either side of the feet.
    final fn get_random_y_spread(&self, spread: f64) -> f64 {
        let height = f64::from(self.base().dimensions().height);
        self.position().y + height * (2.0 * rng().random::<f64>() - 1.0) * spread
    }

    /// `Entity.getRandomZ`: Z up to `spread` bounding-box widths either side of the center.
    final fn get_random_z(&self, spread: f64) -> f64 {
        let width = f64::from(self.base().dimensions().width);
        self.position().z + width * (2.0 * rng().random::<f64>() - 1.0) * spread
    }

    /// Gets the Y coordinate of the entity's eyes.
    final fn get_eye_y(&self) -> f64 {
        self.position().y + self.get_eye_height()
    }

    /// Default for [`Self::is_in_wall`].
    final fn is_in_wall_default(&self) -> bool {
        if self.no_physics() {
            return false;
        }

        let Some(world) = self.level() else {
            return false;
        };

        let position = self.position();
        let check_width = f64::from(self.base().dimensions().width * 0.8);
        let eye_box = entity_eye_suffocation_box(
            DVec3::new(position.x, self.get_eye_y(), position.z),
            check_width,
        );

        !block_effects::for_each_block_in_aabb(eye_box, |pos| {
            let state = world.get_block_state(pos);
            !block_state_suffocates_eye_box(state, world.as_ref(), pos, eye_box)
        })
    }

    /// Computes a unit look-direction vector from the given pitch and yaw in degrees.
    final fn calculate_view_vector(&self, pitch_degrees: f32, yaw_degrees: f32) -> DVec3 {
        let pitch = pitch_degrees.to_radians();
        let yaw = -yaw_degrees.to_radians();
        let yaw_cos = trig::cos(yaw);
        let yaw_sin = trig::sin(yaw);
        let pitch_cos = trig::cos(pitch);
        let pitch_sin = trig::sin(pitch);
        DVec3::new(
            f64::from(yaw_sin * pitch_cos),
            f64::from(-pitch_sin),
            f64::from(yaw_cos * pitch_cos),
        )
    }

    /// Returns this entity's current unit look-direction vector.
    final fn look_angle(&self) -> DVec3 {
        let (yaw, pitch) = self.rotation();
        self.calculate_view_vector(pitch, yaw)
    }

    /// Returns the vanilla offset for the hand holding `item`.
    ///
    /// Only players have a hand offset. If both hands contain the item, vanilla
    /// chooses the main hand; otherwise an offhand-only item uses the opposite
    /// arm from the player's configured main arm.
    final fn hand_holding_item_angle(&self, item: ItemRef) -> DVec3 {
        let Some(player) = self.as_player() else {
            return DVec3::ZERO;
        };
        let item_only_in_offhand = {
            let inventory = player.inventory.lock();
            inventory
                .get_item_in_hand(InteractionHand::OffHand)
                .is(item)
                && !inventory
                    .get_item_in_hand(InteractionHand::MainHand)
                    .is(item)
        };
        let main_arm = player.client_information().main_hand;
        let item_arm = if item_only_in_offhand {
            match main_arm {
                HumanoidArm::Left => HumanoidArm::Right,
                HumanoidArm::Right => HumanoidArm::Left,
            }
        } else {
            main_arm
        };
        let yaw_offset = match item_arm {
            HumanoidArm::Left => -80.0,
            HumanoidArm::Right => 80.0,
        };
        self.calculate_view_vector(0.0, self.rotation().0 + yaw_offset) * 0.5
    }

    /// Gets the entity's velocity in blocks per tick.
    final fn velocity(&self) -> DVec3 {
        self.base().velocity()
    }

    /// Sets the entity's velocity.
    final fn set_velocity(&self, velocity: DVec3) {
        self.base().set_velocity(velocity);
    }

    /// Returns whether this entity's velocity has changed enough to need
    /// syncing to clients.
    final fn needs_velocity_sync(&self) -> bool {
        self.base().needs_velocity_sync()
    }

    /// Marks this entity's velocity as needing to be synced to clients.
    final fn mark_velocity_sync(&self) {
        self.base().mark_velocity_sync();
    }

    /// Clears the vanilla velocity sync marker after send processing.
    final fn clear_velocity_sync(&self) {
        self.base().clear_velocity_sync();
    }

    /// Returns true when vanilla hurt-marked velocity sync is pending.
    final fn hurt_marked(&self) -> bool {
        self.base().hurt_marked()
    }

    /// Marks this entity as hurt for vanilla self-inclusive motion sync.
    final fn mark_hurt(&self) {
        self.base().mark_hurt();
    }

    /// Clears the vanilla hurt-marked motion sync flag.
    final fn clear_hurt_mark(&self) {
        self.base().clear_hurt_mark();
    }

    /// Returns accumulated vanilla fall distance.
    final fn fall_distance(&self) -> f64 {
        self.base().fall_distance()
    }

    /// Returns whether this entity is currently inside powder snow.
    final fn is_in_powder_snow(&self) -> bool {
        self.base().is_in_powder_snow()
    }

    /// Returns whether this entity was inside powder snow during the previous base tick.
    final fn was_in_powder_snow(&self) -> bool {
        self.base().was_in_powder_snow()
    }

    /// Sets accumulated vanilla fall distance.
    final fn set_fall_distance(&self, fall_distance: f64) {
        self.base().set_fall_distance(fall_distance);
    }

    /// Resets accumulated vanilla fall distance.
    final fn reset_fall_distance(&self) {
        self.base().reset_fall_distance();
    }

    /// Caps the accumulated fall distance at 1 once the entity is no longer falling faster than 0.5 blocks per tick.
    final fn check_fall_distance_accumulation(&self) {
        if self.velocity().y > -0.5 && self.fall_distance() > 1.0 {
            self.set_fall_distance(1.0);
        }
    }

    /// Returns the current vanilla fire/freeze state.
    final fn fire_freeze_state(&self) -> EntityFireFreezeState {
        self.base().fire_freeze_state()
    }

    /// Returns the ticks remaining before this entity stops burning.
    final fn remaining_fire_ticks(&self) -> i32 {
        self.base().remaining_fire_ticks()
    }

    /// Sets the ticks remaining before this entity stops burning, clamped to
    /// this entity's fire-ticks cap if any.
    final fn set_remaining_fire_ticks(&self, remaining_fire_ticks: i32) {
        self.base().set_remaining_fire_ticks(
            self.remaining_fire_ticks_cap()
                .map_or(remaining_fire_ticks, |cap| remaining_fire_ticks.min(cap)),
        );
        self.sync_base_fire_freeze_entity_data();
    }

    /// Returns ticks this entity has spent freezing in powder snow.
    final fn ticks_frozen(&self) -> i32 {
        self.base().ticks_frozen()
    }

    /// Sets ticks this entity has spent freezing in powder snow, syncing the
    /// change to clients.
    final fn set_ticks_frozen(&self, ticks_frozen: i32) {
        self.base().set_ticks_frozen(ticks_frozen);
        self.sync_base_fire_freeze_entity_data();
    }

    /// Default for [`Self::fire_immune`].
    final fn fire_immune_default(&self) -> bool {
        self.entity_type().fire_immune
    }

    /// Default for [`Self::fire_immune_ticks`].
    final fn fire_immune_ticks_default(&self) -> i32 {
        0
    }

    /// Default for [`Self::should_play_lava_hurt_sound`].
    final fn should_play_lava_hurt_sound_default(&self) -> bool {
        true
    }

    /// Applies vanilla lava-contact damage after lava ignition effects.
    final fn lava_hurt(&self) {
        if self.fire_immune() {
            return;
        }
        let Some(world) = self.level() else {
            return;
        };

        if self.hurt(
            &world,
            &DamageSource::environment(&vanilla_damage_types::LAVA),
            4.0,
        ) && self.should_play_lava_hurt_sound()
        {
            let pitch = rand::random_range(2.0..2.4);
            self.play_sound(&sound_events::ENTITY_GENERIC_BURN, 0.4, pitch);
        }
    }

    /// Default for [`Self::remaining_fire_ticks_cap`].
    final fn remaining_fire_ticks_cap_default(&self) -> Option<i32> {
        None
    }

    /// Default for [`Self::fire_ignite_extra_ticks`].
    final fn fire_ignite_extra_ticks_default(&self) -> i32 {
        0
    }

    /// Returns whether the entity is on fire on the server.
    final fn is_on_fire(&self) -> bool {
        self.base().is_on_fire(self.fire_immune())
    }

    /// Returns whether this entity currently renders as visually on fire.
    final fn has_visual_fire(&self) -> bool {
        self.base().has_visual_fire()
    }

    /// Returns whether the entity has any frozen ticks.
    final fn is_freezing(&self) -> bool {
        self.base().is_freezing()
    }

    /// Default for [`Self::can_freeze`].
    final fn can_freeze_default(&self) -> bool {
        !REGISTRY.entity_types.is_in_tag(
            self.entity_type(),
            &EntityTypeTag::FREEZE_IMMUNE_ENTITY_TYPES,
        )
    }

    /// Returns the ticks of powder-snow exposure required before this entity
    /// starts taking freeze damage.
    final fn ticks_required_to_freeze(&self) -> i32 {
        DEFAULT_TICKS_REQUIRED_TO_FREEZE
    }

    /// Returns whether this entity has reached full-freeze duration.
    final fn is_fully_frozen(&self) -> bool {
        self.base().is_fully_frozen(self.ticks_required_to_freeze())
    }

    /// Returns how frozen this entity is, from 0.0 to 1.0, based on ticks
    /// frozen versus ticks required.
    final fn percent_frozen(&self) -> f32 {
        let ticks_required = self.ticks_required_to_freeze();
        self.ticks_frozen().min(ticks_required) as f32 / ticks_required as f32
    }

    /// Clears accumulated freezing.
    final fn clear_freeze(&self) {
        self.base().clear_freeze();
        self.sync_base_fire_freeze_entity_data();
    }

    /// Clears fire without resetting the vanilla fire immunity cooldown.
    final fn clear_fire(&self) {
        self.base().clear_fire();
        self.sync_base_fire_freeze_entity_data();
    }

    /// Plays the extinguish sound when currently on fire, then clears fire.
    final fn extinguish_fire(&self) {
        if self.is_on_fire() {
            self.play_entity_on_fire_extinguished_sound();
        }
        self.clear_fire();
    }

    /// Ignites this entity for a vanilla tick duration.
    final fn ignite_for_ticks(&self, number_of_ticks: i32) {
        self.base()
            .ignite_for_ticks(number_of_ticks, self.remaining_fire_ticks_cap());
        self.sync_base_fire_freeze_entity_data();
    }

    /// Projects base fire/freeze state into generated synced entity data.
    final fn sync_base_fire_freeze_entity_data(&self) {
        let Some(synced_data) = self.synced_data() else {
            return;
        };

        synced_data.set_base_ticks_frozen(self.ticks_frozen());
        synced_data.set_base_on_fire_flag(self.is_on_fire() || self.has_visual_fire());
    }

    /// Projects all base-owned synchronized fields into generated entity data.
    final fn sync_base_entity_data(&self) {
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

    /// Returns true if this entity is currently touching water.
    final fn is_in_water(&self) -> bool {
        self.fluid_contact().water_height() > 0.0
    }

    /// Returns true if rain falls at this entity's feet or at the top of its bounding box.
    final fn is_in_rain(&self) -> bool {
        let Some(world) = self.level() else {
            return false;
        };

        let pos = self.block_position();
        world.is_raining_at(pos)
            || world.is_raining_at(BlockPos::new(
                pos.x(),
                self.bounding_box().max_y().floor() as i32,
                pos.z(),
            ))
    }

    /// Returns true if this entity is touching water or standing in rain.
    final fn is_in_water_or_rain(&self) -> bool {
        self.is_in_water() || self.is_in_rain()
    }

    /// Returns true if this entity is currently touching lava.
    final fn is_in_lava(&self) -> bool {
        self.base().is_in_lava()
    }

    /// Returns true if this entity's eyes are currently inside water.
    final fn is_eye_in_water(&self) -> bool {
        self.fluid_contact().eye_in_water()
    }

    /// Returns true if this entity's eyes are currently inside lava.
    final fn is_eye_in_lava(&self) -> bool {
        self.fluid_contact().eye_in_lava()
    }

    /// Returns vanilla underwater state.
    final fn is_under_water(&self) -> bool {
        self.base().was_eye_in_water() && self.is_in_water()
    }

    /// Returns cached fluid contact from the last entity fluid refresh.
    final fn fluid_contact(&self) -> EntityFluidContact {
        self.base().fluid_contact()
    }

    /// Refreshes cached fluid contact from this entity's current bounding box.
    final fn refresh_fluid_contact(&self) -> EntityFluidContact {
        self.scan_and_store_fluid_contact(false)
    }

    /// Runs vanilla `Entity.updateFluidInteraction`: refreshes cached contact,
    /// emits the water-entry splash on transition, resets fall distance in
    /// water, and applies fluid currents.
    ///
    /// Returns whether any fluid touches the entity.
    final fn update_fluid_interaction(&self) -> bool {
        let was_touching_water = self.base().was_touching_water();
        let contact = self.scan_and_store_fluid_contact(true);
        let in_water = contact.water_height() > 0.0;
        if in_water {
            self.base().reset_fall_distance_in_water();
            if !was_touching_water && !self.is_first_tick() {
                self.do_water_splash_effect();
            }
        }
        if let Some(world) = self.level() {
            self.apply_fluid_current_impulses(&world, contact);
        }

        contact.any_fluid()
    }

    /// Scans current fluid contact and stores it on the entity base.
    ///
    /// `advance_eye_water_history` scans currents and records `wasTouchingWater` for
    /// vanilla `updateFluidInteraction`.
    final fn scan_and_store_fluid_contact(
        &self,
        advance_eye_water_history: bool,
    ) -> EntityFluidContact {
        let Some(world) = self.level() else {
            let contact = EntityFluidContact::default();
            if advance_eye_water_history {
                self.base().set_fluid_interaction(contact);
            } else {
                self.base().set_fluid_contact(contact);
            }
            return contact;
        };

        let contact = if advance_eye_water_history {
            EntityFluidContact::scan_with_currents(
                &world,
                self.position(),
                self.get_eye_y(),
                self.bounding_box(),
                self.is_pushed_by_fluid(),
            )
        } else {
            EntityFluidContact::scan(
                &world,
                self.position(),
                self.get_eye_y(),
                self.bounding_box(),
            )
        };
        if advance_eye_water_history {
            self.base().set_fluid_interaction(contact);
        } else {
            self.base().set_fluid_contact(contact);
        }
        contact
    }

    /// Applies vanilla water/lava current impulses from a fluid interaction update.
    final fn apply_fluid_current_impulses(&self, world: &Arc<World>, contact: EntityFluidContact) {
        if !self.is_pushed_by_fluid() {
            return;
        }

        let is_player = self.entity_type() == &vanilla_entities::PLAYER;
        let old_velocity = self.velocity();
        let water_impulse =
            contact.water_current_impulse(is_player, old_velocity, WATER_ENTITY_FLOW_SCALE);
        self.apply_fluid_current_impulse(water_impulse);

        let old_velocity = old_velocity + water_impulse;
        let lava_impulse = contact.lava_current_impulse(
            is_player,
            old_velocity,
            LavaFluid::entity_flow_scale(world),
        );
        self.apply_fluid_current_impulse(lava_impulse);
    }

    /// Applies a non-zero fluid current impulse and marks velocity sync.
    final fn apply_fluid_current_impulse(&self, impulse: DVec3) {
        if impulse.length_squared() > 0.0 {
            self.push_impulse(impulse);
        }
    }

    /// Returns true if this entity type ignores vanilla fall damage.
    final fn is_fall_damage_immune(&self) -> bool {
        REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::FALL_DAMAGE_IMMUNE)
    }

    /// Propagates vanilla fall-damage handling to passengers.
    final fn propagate_fall_to_passengers(
        &self,
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
    ) {
        for passenger in self.passengers() {
            passenger.cause_fall_damage(fall_distance, damage_modifier, source);
        }
    }

    /// Default for [`Self::cause_fall_damage`].
    final fn cause_fall_damage_default(
        self: Arc<Self>,
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
    ) -> bool {
        if self.is_fall_damage_immune() {
            return false;
        }

        self.propagate_fall_to_passengers(fall_distance, damage_modifier, source);
        false
    }

    /// Returns true if the entity is on the ground.
    final fn on_ground(&self) -> bool {
        self.base().on_ground()
    }

    /// Returns true if the last movement was clipped horizontally.
    final fn horizontal_collision(&self) -> bool {
        self.base().horizontal_collision()
    }

    /// Returns true if the last movement was clipped vertically.
    final fn vertical_collision(&self) -> bool {
        self.base().vertical_collision()
    }

    /// Returns true if the last vertical collision was below the entity.
    final fn vertical_collision_below(&self) -> bool {
        self.base().vertical_collision_below()
    }

    /// Returns true when movement bypasses collision physics.
    final fn no_physics(&self) -> bool {
        self.base().no_physics()
    }

    /// Returns true when vanilla block-contact effects may run for this entity.
    final fn is_affected_by_blocks(&self) -> bool {
        !self.is_removed() && !self.no_physics()
    }

    /// Sets whether this entity bypasses collision physics.
    final fn set_no_physics(&self, no_physics: bool) {
        self.base().set_no_physics(no_physics);
    }

    /// Updates item-style `noPhysics` from the entity's current collision state.
    final fn update_no_physics_from_current_collision(&self) {
        let Some(world) = self.level() else {
            self.set_no_physics(false);
            return;
        };

        let collision_world =
            WorldCollisionProvider::for_entity(&world, self.as_entity_event_source());
        let colliding = collision_world.has_collision_with_context(
            &self.bounding_box().deflate(NO_PHYSICS_COLLISION_EPSILON),
            BlockCollisionContext::empty(),
        );
        self.set_no_physics(colliding);
        if colliding {
            let bounding_box = self.bounding_box();
            self.move_towards_closest_space(
                self.position().x,
                f64::midpoint(bounding_box.min_y(), bounding_box.max_y()),
                self.position().z,
            );
        }
    }

    /// Nudges velocity toward the closest non-full collision block.
    final fn move_towards_closest_space(&self, x: f64, y: f64, z: f64) {
        let Some(world) = self.level() else {
            return;
        };

        let block_pos = BlockPos::containing(x, y, z);
        let fractional_position = DVec3::new(
            x - f64::from(block_pos.x()),
            y - f64::from(block_pos.y()),
            z - f64::from(block_pos.z()),
        );
        let closest_direction =
            closest_open_space_direction(block_pos, fractional_position, |neighbor_pos| {
                let block_state = world.get_block_state(neighbor_pos);
                let behavior = BLOCK_BEHAVIORS.get_behavior(block_state.get_block());
                let collision_shape = behavior.get_collision_shape(
                    block_state,
                    world.as_ref(),
                    neighbor_pos,
                    BlockCollisionContext::empty(),
                );
                is_shape_full_block(collision_shape)
            });

        let speed = f64::from(rand::random_range(0.1f32..0.3));
        let step = direction_step(closest_direction);
        let scaled_velocity = self.velocity() * 0.75;
        let next_velocity = match closest_direction.axis() {
            Axis::X => DVec3::new(step * speed, scaled_velocity.y, scaled_velocity.z),
            Axis::Y => DVec3::new(scaled_velocity.x, step * speed, scaled_velocity.z),
            Axis::Z => DVec3::new(scaled_velocity.x, scaled_velocity.y, step * speed),
        };
        self.set_velocity(next_velocity);
    }

    /// Default for [`Self::make_stuck_in_block`].
    final fn make_stuck_in_block_default(&self, _state: BlockStateId, speed_multiplier: DVec3) {
        self.base().make_stuck_in_block(speed_multiplier);
    }

    /// Applies current block-contact effects to this entity.
    ///
    /// Mirrors the shared ownership boundary of vanilla `Entity.applyEffectsFromBlocks`.
    final fn apply_effects_from_blocks(&self) {
        let entity = self.as_entity_event_source();
        let movements = self.base().take_movements_for_block_effects();
        apply_effects_from_block_movements(entity, &movements);
    }

    /// Applies block-contact effects for one explicit movement segment.
    ///
    /// Mirrors vanilla's `Entity.applyEffectsFromBlocks(Vec3, Vec3)` overload,
    /// which does not finalize or clear the entity's normal movement trace.
    final fn apply_effects_from_blocks_between(&self, from: DVec3, to: DVec3) {
        let entity = self.as_entity_event_source();
        apply_effects_from_block_movements(entity, &[EntityMovement::new(from, to)]);
    }

    /// Replays the last finalized block-contact movement list.
    final fn apply_effects_from_blocks_for_last_movements(&self) {
        let entity = self.as_entity_event_source();
        let movements = self.base().last_movements_for_block_effects();
        apply_effects_from_block_movements(entity, &movements);
    }

    /// What vanilla's `/teleport` does once an entity has moved: unless it is gliding, it ends up
    /// on the ground without vertical motion.
    final fn land_after_teleport(&self) {
        if self
            .as_living_entity()
            .is_none_or(|living| !living.is_fall_flying())
        {
            let velocity = self.velocity();
            self.set_velocity(DVec3::new(velocity.x, 0.0, velocity.z));
            self.set_on_ground(true);
        }
    }

    /// Sets whether the entity is on the ground.
    final fn set_on_ground(&self, on_ground: bool) {
        let ground_contact = self.ground_contact_after_movement(on_ground, None);
        let movement_flags = self.base().movement_flags().with_on_ground(on_ground);
        self.base()
            .set_movement_flags(movement_flags, ground_contact);
    }

    /// Sets ground and horizontal collision flags from accepted movement.
    final fn set_on_ground_with_movement(
        &self,
        on_ground: bool,
        horizontal_collision: bool,
        movement: DVec3,
    ) {
        let ground_contact = self.ground_contact_after_movement(on_ground, Some(movement));
        self.base()
            .set_on_ground_with_movement(on_ground, horizontal_collision, ground_contact);
    }

    /// Default final state application for accepted client-authored movement.
    ///
    /// Mirrors the shared tail of vanilla player and controlled-vehicle movement
    /// handling after rollback/collision validation has accepted the target.
    final fn default_apply_accepted_client_movement(
        self: Arc<Self>,
        world: &Arc<World>,
        accepted: AcceptedClientMovement,
    ) -> Result<AcceptedClientMovementOutcome, EntityMoveError> {
        if let Some(position) = accepted.position {
            self.try_set_position(position)?;
        }

        self.set_rotation(accepted.rotation);
        self.set_on_ground_with_movement(
            accepted.on_ground,
            accepted.horizontal_collision,
            accepted.movement,
        );
        if Arc::clone(&self).do_check_fall_damage(accepted.movement, accepted.on_ground, world) {
            return Ok(AcceptedClientMovementOutcome::Handled);
        }
        if accepted.reset_fall_distance {
            self.reset_fall_distance();
        }

        Ok(AcceptedClientMovementOutcome::Applied)
    }

    /// Applies final state accepted from a client-authored movement packet.
    final fn apply_accepted_client_movement(
        self: Arc<Self>,
        world: &Arc<World>,
        accepted: AcceptedClientMovement,
    ) -> Result<AcceptedClientMovementOutcome, EntityMoveError> {
        self.default_apply_accepted_client_movement(world, accepted)
    }

    /// Applies final state accepted from a controlled-vehicle movement packet.
    final fn apply_accepted_client_vehicle_movement(
        self: Arc<Self>,
        world: &Arc<World>,
        mut accepted: AcceptedClientMovement,
    ) -> Result<AcceptedClientMovementOutcome, EntityMoveError> {
        accepted.horizontal_collision = self.horizontal_collision();
        accepted.reset_fall_distance = false;
        self.default_apply_accepted_client_movement(world, accepted)
    }

    /// Attempts to set the entity's position through world lifecycle validation.
    ///
    /// For the base method, see [`Entity::try_set_position_base`]
    #[must_use = "movement commits can fail when world entity state rejects the update"]
    final fn try_set_position(&self, pos: DVec3) -> Result<(), EntityMoveError> {
        self.try_set_position_base(pos)
    }

    /// The base method to set the entity's position through world lifecycle validation.
    #[must_use = "movement commits can fail when world entity state rejects the update"]
    final fn try_set_position_base(&self, pos: DVec3) -> Result<(), EntityMoveError> {
        self.base().try_set_position(pos)
    }

    /// Default for [`Self::teleport_to`].
    // TODO: Recursively reposition this entity's passengers (vanilla
    // `Entity.teleportPassengers`, via `getSelfAndPassengers`)
    #[must_use = "movement commits can fail when world entity state rejects the update"]
    final fn teleport_to_default(&self, pos: DVec3) -> Result<(), EntityMoveError> {
        self.try_set_position(pos)
    }

    /// Sets the vanilla movement-trace old position to the current position.
    final fn set_old_position_to_current(&self) {
        self.base().set_old_position_to_current();
    }

    /// Sets the vanilla movement-trace old position explicitly.
    final fn set_old_position(&self, old_position: DVec3) {
        self.base().set_old_position(old_position);
    }

    /// Removes the latest movement segment recorded this tick.
    final fn remove_latest_movement_recording(&self) {
        self.base().remove_latest_movement_recording();
    }

    /// Returns the block position this entity is standing on.
    final fn on_pos(&self, offset: f32) -> Option<BlockPos> {
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

    /// Default for [`Self::block_pos_below_that_affects_movement`].
    final fn block_pos_below_that_affects_movement_default(&self) -> Option<BlockPos> {
        self.on_pos(0.500_001)
    }

    /// Returns vanilla `getOnPosLegacy()`, used by fall/step block hooks.
    final fn on_pos_legacy(&self) -> Option<BlockPos> {
        self.on_pos(0.2)
    }

    /// Default for [`Self::block_speed_factor`].
    #[expect(
        clippy::float_cmp,
        reason = "intentional: vanilla checks static block speed factors against 1.0"
    )]
    final fn block_speed_factor_default(&self) -> f32 {
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

    /// Returns the jump-height multiplier from the block at this entity's
    /// position, falling back to the block below when the current one
    /// doesn't modify jumps.
    #[expect(
        clippy::float_cmp,
        reason = "intentional: vanilla checks static block jump factors against 1.0"
    )]
    final fn block_jump_factor(&self) -> f32 {
        let Some(world) = self.level() else {
            return 1.0;
        };

        let jump_factor_here = world
            .get_block_state(self.block_position())
            .get_block()
            .config
            .jump_factor;
        if jump_factor_here != 1.0 {
            return jump_factor_here;
        }

        let Some(below_pos) = self.block_pos_below_that_affects_movement() else {
            return 1.0;
        };
        world
            .get_block_state(below_pos)
            .get_block()
            .config
            .jump_factor
    }

    /// Returns this entity's physical pose.
    final fn pose(&self) -> EntityPose {
        self.base().pose()
    }

    /// Default for [`Self::dimensions_for_pose`].
    final fn dimensions_for_pose_default(&self, _pose: EntityPose) -> EntityDimensions {
        self.entity_type().dimensions
    }

    /// Refreshes dimensions for the current physical pose.
    final fn refresh_dimensions(&self) {
        let pose = self.pose();
        let old_dimensions = self.base().dimensions();
        let new_dimensions = self.dimensions_for_pose(pose);
        self.base().set_pose_and_dimensions(pose, new_dimensions);

        let is_small = new_dimensions.width <= FUDGE_SMALL_DIMENSION_LIMIT
            && new_dimensions.height <= FUDGE_SMALL_DIMENSION_LIMIT;
        if !self.is_first_tick()
            && self.level().is_some()
            && !self.no_physics()
            && is_small
            && (new_dimensions.width > old_dimensions.width
                || new_dimensions.height > old_dimensions.height)
            && self.as_player().is_none()
        {
            self.fudge_position_after_size_change(old_dimensions);
        }
    }

    /// Moves the entity to the closest free position for its new dimensions
    /// within the previous dimensions' footprint, returning whether a valid
    /// position was found. Vanilla runs this after growing in a confined space;
    /// Steel's `ThrownEgg` hatchlings also use it to fit a newborn chick at the
    /// egg's impact point.
    final fn fudge_position_after_size_change(
        &self,
        previous_dimensions: EntityDimensions,
    ) -> bool {
        let new_dimensions = self.dimensions_for_pose(self.pose());
        let old_center =
            self.position() + DVec3::new(0.0, f64::from(previous_dimensions.height) / 2.0, 0.0);
        let width_delta = f64::from((new_dimensions.width - previous_dimensions.width).max(0.0))
            + FUDGE_POSITION_EPSILON;
        let height_delta = f64::from((new_dimensions.height - previous_dimensions.height).max(0.0))
            + FUDGE_POSITION_EPSILON;
        let allowed_centers = [WorldAabb::of_size(
            old_center,
            width_delta,
            height_delta,
            width_delta,
        )];

        let Some(world) = self.level() else {
            return false;
        };
        let provider = WorldCollisionProvider::for_entity(&world, self.as_entity_event_source());
        if let Some(free_center) = provider.find_free_position(
            &allowed_centers,
            old_center,
            f64::from(new_dimensions.width),
            f64::from(new_dimensions.height),
            f64::from(new_dimensions.width),
        ) {
            let new_position =
                free_center + DVec3::new(0.0, -f64::from(new_dimensions.height) / 2.0, 0.0);
            match self.try_set_position(new_position) {
                Ok(()) => return true,
                Err(error) => {
                    log::warn!(
                        "failed to fudge entity {} position after size change: {error}",
                        self.id()
                    );
                }
            }
        }

        // Vanilla retries ignoring the vertical axis when both dimensions grow,
        // allowing the entity to keep its previous footprint horizontally.
        if new_dimensions.width > previous_dimensions.width
            && new_dimensions.height > previous_dimensions.height
        {
            let allowed_centers_ignoring_y = [WorldAabb::of_size(
                old_center,
                width_delta,
                FUDGE_POSITION_EPSILON,
                width_delta,
            )];
            if let Some(free_center) = provider.find_free_position(
                &allowed_centers_ignoring_y,
                old_center,
                f64::from(new_dimensions.width),
                f64::from(previous_dimensions.height),
                f64::from(new_dimensions.width),
            ) {
                let new_position = free_center
                    + DVec3::new(
                        0.0,
                        -f64::from(previous_dimensions.height) / 2.0 + FUDGE_POSITION_EPSILON,
                        0.0,
                    );
                return self.try_set_position(new_position).is_ok();
            }
        }

        false
    }

    /// Sets the physical pose and synchronized pose metadata.
    final fn set_pose(&self, pose: EntityPose) {
        self.base()
            .set_pose_and_dimensions(pose, self.dimensions_for_pose(pose));
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_pose(pose);
        }
    }

    /// Default for [`Self::is_crouching`].
    final fn is_crouching_default(&self) -> bool {
        self.pose() == EntityPose::Sneaking
    }

    /// Default for [`Self::is_swimming`].
    final fn is_swimming_default(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_swimming)
    }

    /// Returns whether this entity is on rails.
    final fn is_on_rails(&self) -> bool {
        false
    }

    /// Returns whether a block state is climbable for base movement effects.
    final fn is_state_climbable(&self, state: BlockStateId) -> bool {
        let block = state.get_block();
        block.has_tag(&BlockTag::CLIMBABLE) || block == &vanilla_blocks::POWDER_SNOW
    }

    /// Default for [`Self::movement_emission`].
    final fn movement_emission_default(&self) -> EntityMovementEmission {
        EntityMovementEmission::All
    }

    /// Default for [`Self::may_interact`].
    final fn may_interact_default(&self, _world: &World, _pos: BlockPos) -> bool {
        true
    }

    /// Returns this entity's current air supply in ticks, synced to clients.
    final fn air_supply(&self) -> i32 {
        self.base().air_supply()
    }

    /// Sets this entity's air supply in ticks, syncing the change to clients.
    final fn set_air_supply(&self, air_supply: i32) {
        self.base().set_air_supply(air_supply);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_air_supply(air_supply);
        }
    }

    /// Returns this entity's maximum vanilla air supply.
    final fn max_air_supply(&self) -> i32 {
        DEFAULT_MAX_AIR_SUPPLY
    }

    /// Returns the vanilla portal cooldown in ticks.
    final fn portal_cooldown(&self) -> i32 {
        self.base().portal_cooldown()
    }

    /// Sets the vanilla portal cooldown in ticks.
    final fn set_portal_cooldown(&self, portal_cooldown: i32) {
        self.base().set_portal_cooldown(portal_cooldown);
    }

    /// Returns whether the entity is on vanilla portal cooldown.
    final fn is_on_portal_cooldown(&self) -> bool {
        self.base().is_on_portal_cooldown()
    }

    /// Resets vanilla portal cooldown to this entity's dimension-changing delay.
    final fn reset_portal_cooldown(&self) {
        self.set_portal_cooldown(self.dimension_changing_delay());
    }

    /// Marks this entity as inside a vanilla portal during the current tick.
    final fn set_as_inside_portal(&self, portal: PortalKind, entry_position: BlockPos) {
        if self.is_on_portal_cooldown() {
            self.reset_portal_cooldown();
            return;
        }

        self.base().set_as_inside_portal(portal, entry_position);
    }

    /// Returns this entity's optional vanilla custom name.
    final fn custom_name(&self) -> Option<TextComponent> {
        self.base().custom_name()
    }

    /// Sets this entity's optional vanilla custom name.
    final fn set_custom_name(&self, custom_name: Option<TextComponent>) {
        self.base().set_custom_name(custom_name.clone());
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_custom_name(custom_name);
        }
    }

    /// Returns whether vanilla renders this entity's custom name.
    final fn is_custom_name_visible(&self) -> bool {
        self.base().custom_name_visible()
    }

    /// Sets whether vanilla renders this entity's custom name.
    final fn set_custom_name_visible(&self, visible: bool) {
        self.base().set_custom_name_visible(visible);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_custom_name_visible(visible);
        }
    }

    /// Returns whether this entity has the server-owned vanilla glowing tag.
    final fn has_glowing_tag(&self) -> bool {
        self.base().glowing()
    }

    /// Sets this entity's server-owned vanilla glowing tag.
    final fn set_glowing_tag(&self, glowing: bool) {
        self.base().set_glowing(glowing);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_base_glowing_flag(glowing);
        }
    }

    /// Returns a snapshot of this entity's vanilla scoreboard tags.
    final fn tags(&self) -> Vec<String> {
        self.base().tags()
    }

    /// Adds a vanilla scoreboard tag.
    final fn add_tag(&self, tag: String) -> bool {
        self.base().add_tag(tag)
    }

    /// Removes a vanilla scoreboard tag.
    final fn remove_tag(&self, tag: &str) -> bool {
        self.base().remove_tag(tag)
    }

    /// Returns a snapshot of this entity's vanilla custom data.
    final fn custom_data(&self) -> NbtCompound {
        self.base().custom_data()
    }

    /// Replaces this entity's vanilla custom data.
    final fn set_custom_data(&self, custom_data: NbtCompound) {
        self.base().set_custom_data(custom_data);
    }

    /// Default for [`Self::sound_source`].
    final fn sound_source_default(&self) -> SoundSource {
        SoundSource::Neutral
    }

    /// Default for [`Self::swim_sound`].
    final fn swim_sound_default(&self) -> SoundEventRef {
        &sound_events::ENTITY_GENERIC_SWIM
    }

    /// Returns whether sounds from this entity are suppressed.
    final fn is_silent(&self) -> bool {
        self.base().silent()
    }

    /// Sets whether sounds from this entity are suppressed.
    final fn set_silent(&self, silent: bool) {
        self.base().set_silent(silent);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_silent(silent);
        }
    }

    /// Broadcasts a vanilla entity event/status packet near this entity.
    final fn broadcast_entity_event(&self, event: EntityStatus) {
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

    /// Default for [`Self::play_sound`].
    final fn play_sound_default(&self, sound: SoundEventRef, volume: f32, pitch: f32) {
        if self.is_silent() {
            return;
        }

        if let Some(world) = self.level() {
            world.play_sound_at(
                sound,
                self.sound_source(),
                self.position(),
                volume,
                pitch,
                None,
            );
        }
    }

    /// Default for [`Self::swim_splash_sound`].
    final fn swim_splash_sound_default(&self) -> SoundEventRef {
        &sound_events::ENTITY_GENERIC_SPLASH
    }

    /// Default for [`Self::swim_high_speed_splash_sound`].
    final fn swim_high_speed_splash_sound_default(&self) -> SoundEventRef {
        &sound_events::ENTITY_GENERIC_SPLASH
    }

    /// Default for [`Self::do_water_splash_effect`].
    final fn do_water_splash_effect_default(&self) {
        let (movement, volume_modifier) = if let Some(passenger) = self.controlling_passenger() {
            (passenger.velocity(), 0.9)
        } else {
            (self.velocity(), 0.2)
        };
        let speed = ((movement.x * movement.x * 0.2
            + movement.y * movement.y
            + movement.z * movement.z * 0.2)
            .sqrt() as f32
            * volume_modifier)
            .min(1.0);
        let pitch = 1.0 + (rng().random::<f32>() - rng().random::<f32>()) * 0.4;
        if speed < 0.25 {
            self.play_sound(self.swim_splash_sound(), speed, pitch);
        } else {
            self.play_sound(self.swim_high_speed_splash_sound(), speed, pitch);
        }

        // VANILLA CLIENT-LOCAL: the bubble/splash particle loop uses `Level.addParticle`.

        self.game_event(&vanilla_game_events::SPLASH);
    }

    /// Plays vanilla's extinguished-on-fire entity sound.
    final fn play_entity_on_fire_extinguished_sound(&self) {
        let Some(world) = self.level() else {
            return;
        };
        let pitch = 1.6 + (rand::random::<f32>() - rand::random::<f32>()) * 0.4;
        world.play_sound_at(
            &sound_events::ENTITY_GENERIC_EXTINGUISH_FIRE,
            self.sound_source(),
            self.position(),
            0.7,
            pitch,
            None,
        );
    }

    /// Default for [`Self::play_step_sound`].
    final fn play_step_sound_default(&self, _pos: BlockPos, block_state: BlockStateId) {
        self.play_block_step_sound(block_state);
    }

    /// Plays a vanilla block step sound at this entity's current position.
    final fn play_block_step_sound(&self, block_state: BlockStateId) {
        let sound_type = block_state.get_block().config.sound_type;
        self.play_sound(
            sound_type.step_sound,
            sound_type.volume * 0.15,
            sound_type.pitch,
        );
    }

    /// Plays vanilla's muffled secondary step sound.
    final fn play_muffled_step_sound(&self, block_state: BlockStateId) {
        let sound_type = block_state.get_block().config.sound_type;
        self.play_sound(
            sound_type.step_sound,
            sound_type.volume * 0.05,
            sound_type.pitch * 0.8,
        );
    }

    /// Plays vanilla's combination primary and secondary step sounds.
    final fn play_combination_step_sounds(
        &self,
        primary_step_sound: BlockStateId,
        secondary_step_sound: BlockStateId,
    ) {
        self.play_block_step_sound(primary_step_sound);
        self.play_muffled_step_sound(secondary_step_sound);
    }

    /// Plays vanilla walking step sounds, including amethyst chimes.
    final fn walking_step_sound(&self, pos: BlockPos, block_state: BlockStateId) {
        self.play_step_sound(pos, block_state);
        if block_state
            .get_block()
            .has_tag(&BlockTag::CRYSTAL_SOUND_BLOCKS)
        {
            self.play_amethyst_step_sound();
        }
    }

    /// Plays vanilla amethyst step chime when its cooldown permits it.
    final fn play_amethyst_step_sound(&self) {
        let Some(sound) = self.base().amethyst_step_sound(self.tick_count()) else {
            return;
        };
        self.play_sound(
            &sound_events::BLOCK_AMETHYST_BLOCK_CHIME,
            sound.volume,
            sound.pitch,
        );
    }

    /// Plays vanilla swim sound from movement emission.
    final fn water_swim_sound(&self) {
        let velocity = self.velocity();
        let volume = ((velocity.x * velocity.x * 0.2
            + velocity.y * velocity.y
            + velocity.z * velocity.z * 0.2)
            .sqrt() as f32
            * 0.35)
            .min(1.0);
        self.play_swim_sound(volume);
    }

    /// Plays this entity's swim sound at the given volume.
    final fn play_swim_sound(&self, volume: f32) {
        let pitch = 1.0 + (rand::random::<f32>() - rand::random::<f32>()) * 0.4;
        self.play_sound(self.swim_sound(), volume, pitch);
    }

    /// Default for [`Self::is_flapping`].
    final fn is_flapping_default(&self) -> bool {
        false
    }

    /// Default for [`Self::on_flap`].
    final fn on_flap_default(&self) {}

    /// Processes vanilla flap movement side effects.
    final fn process_flapping_movement(&self) {
        if !self.is_flapping() {
            return;
        }

        self.on_flap();
        if self.movement_emission().emits_events() {
            self.game_event(&vanilla_game_events::FLAP);
        }
    }

    /// Returns the next step threshold after movement side effects are produced.
    final fn next_step(&self) -> f32 {
        self.base().movement_progress().move_dist().floor() + 1.0
    }

    /// Applies vanilla movement sounds and game events after a completed move.
    final fn apply_movement_emission_and_play_sound(
        &self,
        emission: EntityMovementEmission,
        clipped_movement: DVec3,
        effect_pos: BlockPos,
        effect_state: BlockStateId,
    ) {
        let Some(world) = self.level() else {
            return;
        };
        let Some(supporting_pos) = self.on_pos(1.0e-5) else {
            return;
        };

        let supporting_state = world.get_block_state(supporting_pos);
        let climbing = self.is_state_climbable(supporting_state);
        let progress = self
            .base()
            .record_movement_progress(clipped_movement, climbing);

        if progress.crossed_next_step() && supporting_state.get_block() != &vanilla_blocks::AIR {
            let only_effect_state_emissions = supporting_pos == effect_pos;
            let mut produced_side_effects = self.vibration_and_sound_effects_from_block(
                effect_pos,
                effect_state,
                emission.emits_sounds(),
                only_effect_state_emissions,
                clipped_movement,
            );
            if !only_effect_state_emissions {
                produced_side_effects |= self.vibration_and_sound_effects_from_block(
                    supporting_pos,
                    supporting_state,
                    false,
                    emission.emits_events(),
                    clipped_movement,
                );
            }

            if produced_side_effects {
                self.base().set_next_step(self.next_step());
            } else if self.is_in_water() {
                self.base().set_next_step(self.next_step());
                if emission.emits_sounds() {
                    self.water_swim_sound();
                }
                if emission.emits_events() {
                    self.game_event(&vanilla_game_events::SWIM);
                }
            }
        } else if supporting_state.get_block() == &vanilla_blocks::AIR {
            self.process_flapping_movement();
        }
    }

    /// Applies movement side effects after vanilla collision and landing updates.
    final fn apply_movement_side_effects_after_move(&self, world: &World, actual_movement: DVec3) {
        let emission = self.movement_emission();
        if !emission.emits_anything() || self.is_passenger() {
            return;
        }

        let Some(effect_pos) = self.on_pos_legacy() else {
            return;
        };
        let effect_state = world.get_block_state(effect_pos);
        self.apply_movement_emission_and_play_sound(
            emission,
            actual_movement,
            effect_pos,
            effect_state,
        );
    }

    /// Emits step side effects from a candidate movement-effect block.
    final fn vibration_and_sound_effects_from_block(
        &self,
        pos: BlockPos,
        block_state: BlockStateId,
        should_sound: bool,
        should_vibrate: bool,
        clipped_movement: DVec3,
    ) -> bool {
        if block_state.get_block() == &vanilla_blocks::AIR {
            return false;
        }

        let is_climbable = self.is_state_climbable(block_state);
        if !(self.on_ground()
            || is_climbable
            || self.is_crouching() && clipped_movement.y == 0.0
            || self.is_on_rails())
            || self.is_swimming()
        {
            return false;
        }

        if should_sound {
            self.walking_step_sound(pos, block_state);
        }
        if should_vibrate && let Some(world) = self.level() {
            world.game_event_at(
                &vanilla_game_events::STEP,
                self.position(),
                &GameEventContext::new(Some(self.as_entity_event_source()), Some(block_state)),
            );
        }

        true
    }

    /// Default for [`Self::max_up_step`].
    final fn max_up_step_default(&self) -> f32 {
        0.0
    }

    /// Default for [`Self::backs_off_from_edge`].
    final fn backs_off_from_edge_default(&self) -> bool {
        false
    }

    // These mirror vanilla's Entity class methods.

    /// Default for [`Self::get_default_gravity`].
    final fn get_default_gravity_default(&self) -> f64 {
        0.0
    }

    /// Returns true if gravity is disabled for this entity.
    final fn is_no_gravity(&self) -> bool {
        self.base().no_gravity()
    }

    /// Sets whether this entity ignores gravity, syncing the change to clients.
    final fn set_no_gravity(&self, no_gravity: bool) {
        self.base().set_no_gravity(no_gravity);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_no_gravity(no_gravity);
        }
    }

    /// Returns whether this entity is invulnerable to non-bypassing damage.
    final fn is_invulnerable(&self) -> bool {
        self.base().invulnerable()
    }

    /// Sets whether this entity is invulnerable to non-bypassing damage.
    final fn set_invulnerable(&self, invulnerable: bool) {
        self.base().set_invulnerable(invulnerable);
    }

    /// Gets the current gravity value.
    ///
    /// Returns 0 if `no_gravity` is set, otherwise returns `get_default_gravity()`.
    final fn get_gravity(&self) -> f64 {
        if self.is_no_gravity() {
            0.0
        } else {
            self.get_default_gravity()
        }
    }

    /// Default for [`Self::get_air_drag`].
    final fn get_air_drag_default(&self) -> f64 {
        DEFAULT_AIR_DRAG
    }

    /// Default for [`Self::get_effective_gravity`].
    final fn get_effective_gravity_default(&self) -> f64 {
        self.get_gravity()
    }

    /// Applies gravity to the entity's velocity.
    final fn apply_gravity(&self) {
        let gravity = self.get_gravity();
        if gravity != 0.0 {
            let mut vel = self.velocity();
            vel.y -= gravity;
            self.set_velocity(vel);
        }
    }

    /// Adds `input` scaled by `speed`, rotated to face this entity's current
    /// yaw, to its velocity.
    final fn move_relative(&self, speed: f32, input: DVec3) {
        let yaw = self.rotation().0;
        self.set_velocity(self.velocity() + get_input_vector(input, speed, yaw));
    }

    /// Moves the entity without collision physics.
    final fn move_without_physics(&self, delta: DVec3) -> Option<MoveResult> {
        let final_position = self.position() + delta;
        if let Err(error) = self.try_set_position(final_position) {
            log::debug!(
                "Rejected no-physics movement for entity {}: {error}",
                self.id()
            );
            return None;
        }
        self.base().clear_collision_flags();

        Some(MoveResult {
            final_position,
            actual_movement: delta,
            on_ground: self.on_ground(),
            horizontal_collision: false,
            vertical_collision: false,
            x_collision: false,
            z_collision: false,
            final_aabb: self.bounding_box(),
        })
    }

    /// Moves the entity with collision detection.
    ///
    /// Updates position, `on_ground`, velocity (on collision), and returns collision info.
    #[expect(
        clippy::too_many_lines,
        reason = "keeps vanilla movement phases in order"
    )]
    final fn move_entity(
        self: Arc<Self>,
        mover_type: MoverType,
        delta: DVec3,
    ) -> Option<MoveResult> {
        let world = self.level()?;
        if self.no_physics() {
            return self.move_without_physics(delta);
        }

        let mut movement = delta;
        if mover_type == MoverType::Piston {
            let game_time = world.game_time();
            movement = self.base().limit_piston_movement(movement, game_time);
            if movement == DVec3::ZERO {
                return None;
            }
        }
        movement = self
            .base()
            .consume_stuck_speed_multiplier(movement, mover_type != MoverType::Piston);

        let physics_state = physics_state_for_move(self.as_entity_event_source());
        let start_position = physics_state.position();

        // Perform collision detection and movement
        let collision_world =
            WorldCollisionProvider::for_entity(&world, self.as_entity_event_source());
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
        if should_apply_resolved_movement(movement, result.actual_movement) {
            self.reset_fall_distance_on_resetting_clip(&world, result.actual_movement);
            if let Err(error) = self.try_set_position(result.final_position) {
                log::debug!(
                    "Rejected resolved movement for entity {}: {error}",
                    self.id()
                );
                self.remove_latest_movement_recording();
                return None;
            }
        }
        let vertical_state_update =
            EntityVerticalMovementStateUpdate::for_move(movement, self.is_server_driven_movement());
        let movement_flags = EntityMovementFlags::after_move_with_previous(
            self.base().movement_flags(),
            vertical_state_update,
            result.on_ground,
            result.horizontal_collision,
            result.vertical_collision,
            movement,
        );
        let ground_contact = if vertical_state_update.refreshes_state() {
            self.ground_contact_after_movement(result.on_ground, Some(result.actual_movement))
        } else {
            self.base().ground_contact()
        };
        self.base()
            .set_movement_flags(movement_flags, ground_contact);

        if self.is_server_driven_movement()
            && Arc::clone(&self).apply_fall_damage_after_move(&result, &world)
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
                result.actual_movement.y,
                self.is_living_entity(),
                self.is_suppressing_bounce(),
                self.get_effective_gravity(),
                self.get_air_drag(),
            );

            let next_velocity =
                if let Some(effect_pos) = self.block_pos_below_that_affects_movement() {
                    let effect_state = world.get_block_state(effect_pos);
                    let behavior = BLOCK_BEHAVIORS.get_behavior(effect_state.get_block());
                    let (res, bounced) = behavior.update_entity_movement_after_fall_on(
                        effect_state,
                        &world,
                        effect_pos,
                        landing_context,
                    );
                    if bounced {
                        behavior.bounce_on(
                            effect_state,
                            &world,
                            effect_pos,
                            EntityFallOnContext::from_entity(
                                self.fall_distance(),
                                self.as_entity_event_source(),
                            ),
                        );
                    }
                    res
                } else {
                    landing_context.default_velocity_after_fall_on()
                };
            self.set_velocity(next_velocity);
        }

        self.apply_movement_side_effects_after_move(&world, result.actual_movement);

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
    final fn apply_fall_damage_after_move(
        self: Arc<Self>,
        result: &MoveResult,
        world: &Arc<World>,
    ) -> bool {
        self.do_check_fall_damage(result.actual_movement, result.on_ground, world)
    }

    /// Resets fall distance when vanilla's fall-damage-resetting clip hits.
    final fn reset_fall_distance_on_resetting_clip(&self, world: &Arc<World>, movement: DVec3) {
        let Some(check_to) =
            fall_damage_reset_clip_target(self.position(), movement, self.fall_distance())
        else {
            return;
        };

        let hit = world.clip(
            self.position(),
            check_to,
            ClipBlockShape::FallDamageResetting {
                entity_is_player: self.entity_type() == &vanilla_entities::PLAYER,
            },
            ClipFluid::Water,
        );
        if !hit.is_miss() {
            self.reset_fall_distance();
        }
    }

    /// Applies fall damage from the block fallen on, then returns whether the
    /// entity was removed (e.g. killed) as a result.
    ///
    /// Callers update on-ground/supporting-block state before this method.
    final fn do_check_fall_damage(
        self: Arc<Self>,
        movement: DVec3,
        on_ground: bool,
        world: &Arc<World>,
    ) -> bool {
        let Some(effect_pos) = self.on_pos_legacy() else {
            return false;
        };
        let effect_state = world.get_block_state(effect_pos);
        Arc::clone(&self).check_fall_damage(movement.y, on_ground, effect_state, effect_pos, world);
        self.is_removed()
    }

    /// Refreshes vanilla supporting-block state before fall-damage side effects.
    final fn refresh_supporting_block_for_fall_damage(&self, movement: DVec3, on_ground: bool) {
        let ground_contact = self.ground_contact_after_movement(on_ground, Some(movement));
        self.base().set_ground_contact(ground_contact);
    }

    /// Default for [`Self::check_fall_damage`].
    final fn check_fall_damage_default(
        self: Arc<Self>,
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
                EntityFallOnContext::from_entity(fall_distance, self.as_entity_event_source());
            if let Some(fall_damage) = behavior.fall_on(on_state, world, pos, fall_context) {
                let damage_applied = Arc::clone(&self).cause_fall_damage(
                    fall_damage.fall_distance,
                    fall_damage.damage_modifier,
                    &fall_damage.source,
                );
                behavior.after_fall_on_damage(
                    on_state,
                    world,
                    pos,
                    self.as_entity_event_source(),
                    &fall_damage,
                    damage_applied,
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
    final fn ground_contact_after_movement(
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

    /// Determines the supporting block state and position underneath the entity while grounded.
    final fn check_supporting_block(
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
        let collision_world =
            WorldCollisionProvider::for_entity(world, self.as_entity_event_source());
        let descending = self.is_descending();
        let mut supporting_block =
            collision_world.find_supporting_block(self.position(), &test_area, descending);

        if supporting_block.is_none()
            && !self.base().on_ground_no_blocks()
            && let Some(movement) = movement
        {
            let previous_test_area = test_area.translate(DVec3::new(-movement.x, 0.0, -movement.z));
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
    /// The item spawns at the entity's position with the given Y offset and
    /// has a default pickup delay.
    ///
    /// Returns `None` if the item stack is empty or the entity has no world.
    final fn spawn_at_location(
        &self,
        item: ItemStack,
        y_offset: f64,
    ) -> Option<Arc<entities::ItemEntity>> {
        let world = self.level()?;
        let pos = self.position();
        let entity = world.spawn_item(DVec3::new(pos.x, pos.y + y_offset, pos.z), item)?;
        entity.set_default_pickup_delay();
        Some(entity)
    }

    /// Spawns an item at this entity's location plus a vanilla attachment offset.
    final fn spawn_at_location_with_offset(
        &self,
        item: ItemStack,
        offset: DVec3,
    ) -> Option<Arc<entities::ItemEntity>> {
        let world = self.level()?;
        let entity = world.spawn_item(self.position() + offset, item)?;
        entity.set_default_pickup_delay();
        Some(entity)
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

    /// Default for [`Self::apply_implicit_item_components`].
    final fn apply_implicit_item_components_default(&self, _item_stack: &ItemStack) {}

    /// Spawn-item data is merged into the entity's current save state before it
    /// is loaded. Keeping that merge at the entity boundary preserves defaults
    /// and entity-specific state when the component only overrides one field.
    final fn apply_spawn_data(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let read_int = |key: &str| {
            nbt.int(key)
                .or_else(|| nbt.short(key).map(i32::from))
                .or_else(|| nbt.byte(key).map(i32::from))
        };

        if let Some(position) = read_nbt_dvec3(&nbt, "Pos")
            && position.is_finite()
        {
            self.base()
                .set_position_local(super::clamp_loaded_entity_position(position));
        }

        if let Some(motion) = read_nbt_dvec3(&nbt, "Motion") {
            self.set_velocity(sanitize_nbt_motion(motion));
        }

        if let Some((yaw, pitch)) = read_nbt_rotation(&nbt, "Rotation") {
            self.set_rotation((yaw, pitch));
            self.set_y_head_rot(yaw);
            self.set_y_body_rot(yaw);
        }

        if let Some(fall_distance) = nbt
            .double("fall_distance")
            .or_else(|| nbt.double("FallDistance"))
        {
            self.set_fall_distance(fall_distance);
        }

        if let Some(on_ground) = nbt.byte("OnGround") {
            self.set_on_ground(on_ground != 0);
        }

        let mut save_data = self.base().save_data();
        if let Some(air_supply) = read_int("Air") {
            save_data.air_supply = air_supply;
        }
        if let Some(portal_cooldown) = read_int("PortalCooldown") {
            save_data.portal_cooldown = portal_cooldown;
        }
        if let Some(no_gravity) = nbt.byte("NoGravity") {
            save_data.no_gravity = no_gravity != 0;
        }
        if let Some(invulnerable) = nbt.byte("Invulnerable") {
            save_data.invulnerable = invulnerable != 0;
        }
        if let Some(custom_name) = nbt
            .get("CustomName")
            .and_then(|tag| TextComponent::from_nbt(&tag.to_owned()))
        {
            save_data.custom_name = Some(custom_name);
        }
        if let Some(custom_name_visible) = nbt.byte("CustomNameVisible") {
            save_data.custom_name_visible = custom_name_visible != 0;
        }
        if let Some(silent) = nbt.byte("Silent") {
            save_data.silent = silent != 0;
        }
        if let Some(glowing) = nbt.byte("Glowing") {
            save_data.glowing = glowing != 0;
        }
        if let Some(tags) = nbt.list("Tags").and_then(|list| list.strings()) {
            save_data.tags = tags
                .iter()
                .take(MAX_ENTITY_TAGS)
                .map(|tag| tag.to_str().into_owned())
                .collect::<BTreeSet<_>>();
        }
        if let Some(custom_data) = nbt.compound("data") {
            save_data.custom_data = custom_data.to_owned();
        }
        self.base().replace_save_data(save_data);

        if let Some(remaining_fire_ticks) = read_int("Fire") {
            self.set_remaining_fire_ticks(remaining_fire_ticks);
        }
        if let Some(ticks_frozen) = read_int("TicksFrozen") {
            self.set_ticks_frozen(ticks_frozen);
        }
        if let Some(has_visual_fire) = nbt.byte("HasVisualFire") {
            self.base().set_visual_fire(has_visual_fire != 0);
        }

        self.load_additional(nbt);
        self.set_old_position_to_current();
        self.base().set_old_rotation_to_current();
        self.sync_base_entity_data();
    }

    /// Returns whether this entity is immune to damage from `source`: already
    /// removed, or invulnerable and the source doesn't bypass invulnerability.
    final fn is_invulnerable_to_base(&self, source: &DamageSource) -> bool {
        self.is_removed()
            || self.is_invulnerable()
                && !source.bypasses_invulnerability()
                && !source.is_creative_player()
            || source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FIRE) && self.fire_immune()
            || source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FALL)
                && self.is_fall_damage_immune()
    }

    /// Default for [`Self::hurt`].
    final fn hurt_default(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        // Vanilla `Projectile.hurtServer` overrides the entity default.
        if let Some(projectile) = self.as_projectile() {
            return Projectile::hurt(projectile, world, source, amount);
        }
        false
    }

    // This already exists for structures and AABB whatever that might be, but I also need it for entities, I hope this is the right spot to put it.
    /// Calculates the squared Euclidean distance from this entity's position to the given position.
    final fn distance_to_sqr(&self, pos: DVec3) -> f64 {
        let dx = self.position().x - pos.x;
        let dy = self.position().y - pos.y;
        let dz = self.position().z - pos.z;

        dx * dx + dy * dy + dz * dz
    }

    /// Sets position and rotation.
    ///
    /// # Panics
    ///
    /// Panics if the active world entity manager rejects the snap position. This is an invariant
    /// failure for loaded entities.
    final fn snap_to(&self, position: DVec3, yaw: f32, pitch: f32) {
        if let Err(error) = self.try_set_position(position) {
            panic!(
                "failed to commit entity {} snap position: {error}",
                self.id()
            );
        }
        self.set_rotation((yaw, pitch));
        self.set_old_position_to_current();
    }

    /// Default for [`Self::killed_entity`].
    final fn killed_entity_default(
        &self,
        _world: &World,
        _entity: &Living<'_>,
        _source: &DamageSource,
    ) -> bool {
        true
    }

    /// Default for [`Self::award_kill_score`].
    final fn award_kill_score_default(&self, _victim: &dyn Entity, _killing_blow: &DamageSource) {
        // TODO: Trigger advancement criterion ENTITY_KILLED_PLAYER if the victim is a player.
    }
}

pub(super) fn compute_modified_friction(friction: f64, modifier: f64) -> f64 {
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
