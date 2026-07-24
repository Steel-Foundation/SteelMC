use super::*;

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
/// Concrete implementations must also claim a unique [`steel_utils::DowncastTypeKey`]
/// through [`steel_utils::DowncastType`].
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
pub trait Entity: EntityEventSource + ErasedType + Send + Sync + 'static {
    /// Returns a reference to the entity's shared vanilla base fields.
    fn base(&self) -> &EntityBase;

    /// Gets the entity type containing tracking range, dimensions, etc.
    fn entity_type(&self) -> EntityTypeRef;

    /// Returns whether this entity ignores chunk ticking visibility.
    ///
    /// Mirrors vanilla `Entity.isAlwaysTicking`.
    fn is_always_ticking(&self) -> bool {
        false
    }

    /// Returns whether this entity should be broadcast to the given player.
    ///
    /// Mirrors vanilla `Entity.broadcastToPlayer`. Most entities are always
    /// broadcastable; players override this for spectator visibility rules.
    fn broadcast_to_player(&self, _player: &Player) -> bool {
        true
    }

    /// Gets the entity's unique network ID (session-local).
    fn id(&self) -> i32 {
        self.base().id()
    }

    /// Gets the UUID of the entity (persistent identifier).
    fn uuid(&self) -> Uuid {
        self.base().uuid()
    }

    /// Returns this entity's vanilla scoreboard holder name.
    ///
    /// Non-player entities use their UUID string. Players override this with
    /// their game profile name.
    fn scoreboard_name(&self) -> String {
        self.uuid().to_string()
    }

    /// Returns this entity's vanilla name component before team decoration.
    fn name(&self) -> TextComponent {
        self.custom_name().map_or_else(
            || entity_type_name(self.entity_type()),
            remove_entity_name_actions,
        )
    }

    /// Returns this entity's vanilla display name.
    fn display_name(&self) -> TextComponent {
        let name = self.name();
        name.clone()
            .hover_event(HoverEvent::show_entity(
                self.entity_type().key.to_string(),
                self.uuid(),
                Some(name),
            ))
            .insertion(self.uuid().to_string())
    }

    /// Returns this entity's plain vanilla name.
    ///
    /// Custom names are resolved as text components; otherwise entities use
    /// their translated type description. Players override this with their
    /// game profile name.
    fn plain_text_name(&self) -> String {
        self.name().to_plain(&DisplayResolutor)
    }

    /// Returns the vanilla-shaped entity NBT used by command predicates.
    ///
    /// Mirrors `NbtPredicate.getEntityTagToCompare`: base and type-specific
    /// `Entity.saveWithoutId` data, passengers, and a player's selected item.
    fn nbt_for_data_compare(&self) -> NbtCompound {
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
    fn nbt_for_passenger_save(&self) -> Option<NbtCompound> {
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
    fn position(&self) -> DVec3 {
        self.base().position()
    }

    /// Gets the position to send in this entity's add-entity packet.
    ///
    /// Mirrors vanilla `Entity.getAddEntityPacket()` overloads. Most entities
    /// spawn at their current position; block-attached entities can override
    /// this with the backing block position used by the vanilla packet.
    fn spawn_position(&self) -> DVec3 {
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

    /// Returns vanilla `Entity.getInBlockState`.
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

    /// Returns vanilla `Entity.isFree()` for the current bounding box shifted by `delta`.
    fn is_free(&self, delta: DVec3) -> bool {
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

    /// Returns whether this entity obstructs block placement.
    ///
    /// Mirrors vanilla `Entity.blocksBuilding`. Base entities do not obstruct
    /// placement unless a concrete entity type opts in.
    fn blocks_building(&self) -> bool {
        false
    }

    /// Returns whether this entity can be targeted by picking and interaction raycasts.
    ///
    /// Mirrors vanilla `Entity.isPickable`. Base entities are not pickable unless
    /// a concrete entity type opts in.
    fn is_pickable(&self) -> bool {
        false
    }

    /// Returns whether this entity can be attacked.
    ///
    /// Mirrors vanilla `Entity.isAttackable`. Concrete entities that override
    /// vanilla to reject player attacks should override this method.
    fn attackable(&self) -> bool {
        true
    }

    /// Returns whether this entity handles and consumes an attack before normal damage.
    ///
    /// Mirrors vanilla `Entity.skipAttackInteraction`.
    fn skip_attack_interaction(&self, _source: &dyn Entity) -> bool {
        false
    }

    /// Returns whether this entity participates in vanilla push separation.
    ///
    /// Mirrors vanilla `Entity.isPushable`. Base entities are not pushable unless
    /// a concrete entity type opts in.
    fn is_pushable(&self) -> bool {
        false
    }

    /// Returns whether vanilla fluid currents can push this entity.
    fn is_pushed_by_fluid(&self) -> bool {
        true
    }

    /// Applies vanilla `Entity.onAboveBubbleColumn`.
    fn on_above_bubble_column(&self, drag_down: bool, _pos: BlockPos) {
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
    }

    /// Applies vanilla `Entity.onInsideBubbleColumn`.
    fn on_inside_bubble_column(&self, drag_down: bool) {
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

    /// Returns whether this entity is invisible to normal entity selectors.
    ///
    /// Mirrors vanilla `Entity.isSpectator`. Base entities are never spectators;
    /// players override this from their game mode.
    fn is_spectator(&self) -> bool {
        false
    }

    /// Returns whether this entity is excluded from pressure plates and tripwires.
    ///
    /// Mirrors vanilla `Entity.isIgnoringBlockTriggers`. Display-like entities
    /// and marker entities override this capability.
    fn is_ignoring_block_triggers(&self) -> bool {
        false
    }

    /// Returns how vanilla lets this entity respond to piston movement.
    fn piston_push_reaction(&self) -> PushReaction {
        if self.is_marker_armor_stand() {
            PushReaction::Ignore
        } else {
            self.entity_type().flags.piston_push_reaction
        }
    }

    /// Returns whether this entity's main supporting block is `pos`.
    fn is_supported_by(&self, pos: BlockPos) -> bool {
        self.base().supporting_block() == Some(pos)
    }

    /// Returns whether vanilla lets this entity interact with its loaded level.
    fn can_interact_with_level(&self) -> bool {
        self.is_alive() && !self.is_removed() && !self.is_spectator()
    }

    /// Returns vanilla `Entity.isInvisible()`.
    fn is_invisible(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_base_invisible_flag)
    }

    /// Returns vanilla `Entity.isDiscrete()`.
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
    fn is_tame_owned_by(&self, _owner: &dyn LivingEntity) -> bool {
        false
    }

    /// Returns the vanilla `Projectile` owner UUID when this entity exposes one.
    fn projectile_owner_uuid(&self) -> Option<Uuid> {
        None
    }

    /// Returns the live vanilla `Projectile` owner when this entity exposes one.
    fn projectile_owner(&self) -> Option<SharedEntity> {
        None
    }

    /// Returns true for vanilla players whose abilities have `flying` set.
    fn is_flying_player(&self) -> bool {
        false
    }

    /// Returns whether `other` can collide with this entity.
    ///
    /// Mirrors vanilla `Entity.canBeCollidedWith`. Base entities cannot be collided
    /// with unless a concrete entity type opts in.
    fn can_be_collided_with(&self, _other: Option<&dyn Entity>) -> bool {
        false
    }

    /// Returns whether projectile collision may interact with this entity.
    ///
    /// Mirrors vanilla `Entity.canBeHitByProjectile`.
    fn can_be_hit_by_projectile(&self) -> bool {
        !self.is_removed() && self.is_pickable()
    }

    /// Returns this entity's Vanilla projectile deflection behavior.
    fn deflection(&self, _projectile: &dyn Projectile) -> ProjectileDeflection {
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
    ///
    /// Mirrors vanilla `Entity.getVehicle`.
    fn vehicle(&self) -> Option<SharedEntity> {
        self.base().vehicle()
    }

    /// Returns the vehicle this entity directly controls, if any.
    ///
    /// Mirrors vanilla `Entity.getControlledVehicle`.
    fn controlled_vehicle(&self) -> Option<SharedEntity> {
        let vehicle = self.vehicle()?;
        let controlled_by_self = vehicle
            .controlling_passenger()
            .is_some_and(|passenger| passenger.id() == self.id());
        controlled_by_self.then_some(vehicle)
    }

    /// Returns whether this entity is riding another entity.
    ///
    /// Mirrors vanilla `Entity.isPassenger`.
    fn is_passenger(&self) -> bool {
        self.vehicle().is_some()
    }

    /// Returns whether vanilla allows this entity to start riding `vehicle`.
    ///
    /// Mirrors vanilla `Entity.canRide`.
    fn can_ride(&self, _vehicle: &dyn Entity) -> bool {
        !self.is_discrete() && self.base().boarding_cooldown() <= 0
    }

    /// Stops riding the current vehicle, if any.
    ///
    /// Mirrors vanilla `Entity.stopRiding`.
    fn stop_riding(&self) {
        self.base().stop_riding();
    }

    /// Starts riding `entity_to_ride` if vanilla boarding rules allow it.
    ///
    /// Mirrors vanilla `Entity.startRiding(Entity)`.
    fn start_riding(&self, entity_to_ride: &SharedEntity) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        let Some(passenger) = world.get_entity_by_id(self.id()) else {
            return false;
        };
        start_riding_entities(&passenger, entity_to_ride)
    }

    /// Gets this entity's direct passengers.
    ///
    /// Mirrors vanilla `Entity.getPassengers`.
    fn passengers(&self) -> Vec<SharedEntity> {
        self.base().passengers()
    }

    /// Counts indirect player passengers.
    ///
    /// Mirrors vanilla `Entity.countPlayerPassengers`.
    fn count_player_passengers(&self) -> usize {
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
    ///
    /// Mirrors vanilla `Entity.hasExactlyOnePlayerPassenger`.
    fn has_exactly_one_player_passenger(&self) -> bool {
        self.count_player_passengers() == 1
    }

    /// Gets this entity's first direct passenger.
    ///
    /// Mirrors vanilla `Entity.getFirstPassenger`.
    fn first_passenger(&self) -> Option<SharedEntity> {
        self.base().first_passenger()
    }

    /// Returns the living passenger currently controlling this entity, if any.
    ///
    /// Mirrors vanilla `Entity.getControllingPassenger`. Base entities have no
    /// controller; controllable vehicles override this based on their own rules.
    fn controlling_passenger(&self) -> Option<SharedEntity> {
        None
    }

    /// Returns whether this entity can control a vehicle it is riding.
    ///
    /// Mirrors vanilla `Entity.canControlVehicle`.
    fn can_control_vehicle(&self) -> bool {
        !REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::NON_CONTROLLING_RIDER)
    }

    /// Returns whether this entity currently has a controlling passenger.
    ///
    /// Mirrors vanilla `Entity.hasControllingPassenger`.
    fn has_controlling_passenger(&self) -> bool {
        self.controlling_passenger().is_some()
    }

    /// Returns whether this entity has any direct passengers.
    ///
    /// Mirrors vanilla `Entity.isVehicle`.
    fn is_vehicle(&self) -> bool {
        self.base().is_vehicle()
    }

    /// Returns vanilla `Entity.dismountsUnderwater`.
    fn dismounts_underwater(&self) -> bool {
        REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::DISMOUNTS_UNDERWATER)
    }

    /// Returns whether `passenger` is a direct passenger of this entity.
    ///
    /// Mirrors vanilla `Entity.hasPassenger(Entity)`.
    fn has_passenger(&self, passenger: &dyn Entity) -> bool {
        self.base().has_passenger_id(passenger.id())
    }

    /// Returns whether this entity can accept `passenger` as a direct passenger.
    ///
    /// Mirrors vanilla `Entity.canAddPassenger`.
    fn can_add_passenger(&self, _passenger: &dyn Entity) -> bool {
        self.passengers().is_empty()
    }

    /// Returns whether this entity can accept any passenger.
    ///
    /// Mirrors vanilla `Entity.couldAcceptPassenger`.
    fn could_accept_passenger(&self) -> bool {
        true
    }

    /// Returns the current direct passenger index for attachment lookup.
    fn passenger_index(&self, passenger: &dyn Entity) -> Option<usize> {
        self.passengers()
            .iter()
            .position(|entity| entity.id() == passenger.id())
    }

    /// Returns this passenger's vehicle attachment point.
    ///
    /// Mirrors vanilla `Entity.getVehicleAttachmentPoint`.
    fn vehicle_attachment_point(&self, _vehicle: &dyn Entity) -> DVec3 {
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
    fn passenger_attachment_point(&self, passenger: &dyn Entity) -> DVec3 {
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
    ///
    /// Mirrors vanilla `Entity.getPassengerRidingPosition`.
    fn passenger_riding_position(&self, passenger: &dyn Entity) -> DVec3 {
        self.position() + self.passenger_attachment_point(passenger)
    }

    /// Repositions a direct passenger from this vehicle's attachment point.
    ///
    /// Mirrors vanilla `Entity.positionRider`.
    fn position_rider(&self, passenger: &dyn Entity) {
        if !self.has_passenger(passenger) {
            return;
        }

        let riding_position = self.passenger_riding_position(passenger);
        let vehicle_attachment = passenger.vehicle_attachment_point(self.as_entity_event_source());
        if let Err(error) = passenger.try_set_position(riding_position - vehicle_attachment) {
            log::debug!(
                "Failed to position passenger {} riding entity {}: {error}",
                passenger.id(),
                self.id()
            );
        }
    }

    /// Returns this entity's root vehicle ID, or this entity's ID when it is not riding.
    ///
    /// Mirrors vanilla `Entity.getRootVehicle` using session IDs for object identity.
    fn root_vehicle_id(&self) -> i32 {
        self.root_vehicle().map_or(self.id(), |entity| entity.id())
    }

    /// Returns this entity's root vehicle, if this entity is riding one.
    ///
    /// Mirrors vanilla `Entity.getRootVehicle`.
    fn root_vehicle(&self) -> Option<SharedEntity> {
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
    ///
    /// Mirrors vanilla `Entity.isPassengerOfSameVehicle`.
    fn is_passenger_of_same_vehicle(&self, other: &dyn Entity) -> bool {
        self.root_vehicle_id() == other.root_vehicle_id()
    }

    /// Returns whether `entity` is an indirect passenger of this entity.
    ///
    /// Mirrors vanilla `Entity.hasIndirectPassenger`.
    fn has_indirect_passenger(&self, entity: &dyn Entity) -> bool {
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

    /// Returns whether this entity can collide with `other`.
    ///
    /// Mirrors vanilla `Entity.canCollideWith`.
    fn can_collide_with(&self, other: &dyn Entity) -> bool {
        other.can_be_collided_with(Some(self.as_entity_event_source()))
            && !self.is_passenger_of_same_vehicle(other)
    }

    /// Adds an impulse to this entity's velocity and marks velocity for sync.
    ///
    /// Mirrors vanilla `Entity.push(double, double, double)`.
    fn push_impulse(&self, impulse: DVec3) {
        if !impulse.is_finite() {
            return;
        }

        self.set_velocity(self.velocity() + impulse);
        self.mark_velocity_sync();
    }

    /// Applies vanilla entity-to-entity push separation.
    ///
    /// Mirrors vanilla `Entity.push(Entity)`.
    fn push_entity(&self, entity: &dyn Entity) {
        if self.is_passenger_of_same_vehicle(entity) || entity.no_physics() || self.no_physics() {
            return;
        }

        let mut x = entity.position().x - self.position().x;
        let mut z = entity.position().z - self.position().z;
        let mut distance = x.abs().max(z.abs());
        if distance < 0.01 {
            return;
        }

        distance = distance.sqrt();
        x /= distance;
        z /= distance;
        let scale = (1.0 / distance).min(1.0) * 0.05;
        x *= scale;
        z *= scale;

        if !self.is_vehicle() && self.is_pushable() {
            self.push_impulse(DVec3::new(-x, 0.0, -z));
        }
        if !entity.is_vehicle() && entity.is_pushable() {
            entity.push_impulse(DVec3::new(x, 0.0, z));
        }
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

    /// Returns vanilla `Entity.getRelativePortalPosition`.
    fn get_relative_portal_position(&self, axis: Axis, portal_area: FoundRectangle) -> DVec3 {
        let offsets = PortalShape::get_relative_position(
            portal_area,
            axis,
            self.position(),
            self.dimensions_for_pose(self.pose()),
        );
        if self.as_living_entity().is_some() {
            reset_forward_direction_of_relative_portal_position(offsets)
        } else {
            offsets
        }
    }

    /// Default vanilla `Entity.tick()` behavior.
    ///
    /// Concrete entity ticks that mirror vanilla `super.tick()` should call this
    /// rather than calling [`Self::base_tick`] directly.
    fn default_tick(&self) {
        self.base_tick();
    }

    /// Called every game tick when the entity is in a ticked chunk.
    ///
    /// Use `self.level()` to access the world for physics, block queries, etc.
    /// The caller handles post-tick dirty data sync.
    ///
    /// Steel keeps the fallback empty because many vanilla subclasses override
    /// tick without calling `super.tick()`.
    fn tick(&self) {}

    /// Called every game tick while this entity is riding another entity.
    ///
    /// Mirrors vanilla `Entity.rideTick`.
    fn ride_tick(&self) {
        self.set_velocity(DVec3::ZERO);
        self.tick();
        if let Some(vehicle) = self.vehicle() {
            vehicle.position_rider(self.as_entity_event_source());
        }
    }

    /// Runs vanilla `Entity.baseTick` pieces Steel currently implements.
    ///
    /// This intentionally stays separate from `tick()` because several vanilla
    /// subclasses override tick without calling `super.tick()`.
    fn base_tick(&self) {
        self.entity_base_tick();
    }

    /// Runs vanilla `Entity.handlePortal` behavior currently implemented by Steel.
    fn handle_portal(&self) {
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
                        source_world: world.clone(),
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
    fn entity_base_tick(&self) {
        self.base().advance_base_tick_state();
        self.handle_portal();
        self.base().advance_powder_snow_contact_for_base_tick();
        self.refresh_fluid_contact_for_base_tick();
        self.update_swimming();
        self.base().reset_fall_distance_in_water();
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
        // Vanilla checks `this instanceof Leashable` inside `Entity.baseTick`.
        if let Some(mob) = self.as_mob() {
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

    /// Runs entity-specific behavior after falling below the world.
    fn on_below_world(&self) {
        self.set_removed(RemovalReason::Discarded);
    }

    /// Runs vanilla pre-tick despawn checks.
    fn check_despawn(&self) {}

    /// Applies an inside-block effect queued by vanilla's step-based collector.
    fn apply_inside_block_effect(&self, effect_type: InsideBlockEffectType) {
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

    /// Updates synchronized entity data just before tracker sync.
    ///
    /// Mirrors vanilla `Entity.updateDataBeforeSync`.
    fn update_data_before_sync(&self) {}

    /// Packs syncable attributes for initial spawn pairing.
    ///
    /// Mirrors vanilla `ServerEntity.sendPairingData`, which sends all syncable
    /// living attributes after the add-entity and metadata packets.
    fn pack_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
        Vec::new()
    }

    /// Drains syncable dirty attributes for per-tick tracking updates.
    ///
    /// Mirrors vanilla `ServerEntity.sendDirtyEntityData`, which sends dirty
    /// living attributes after dirty entity data.
    fn drain_dirty_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
        Vec::new()
    }

    /// Drains dirty mob-effect packet changes for vanilla recipients.
    fn drain_dirty_mob_effects(&self) -> Vec<MobEffectSyncChange> {
        Vec::new()
    }

    /// Packs non-empty equipment slots for initial spawn pairing.
    fn pack_all_equipment(&self) -> Vec<EquipmentSlotItem> {
        Vec::new()
    }

    /// Drains equipment slots that changed since the last tracker sync.
    fn drain_dirty_equipment(&self) -> Vec<EquipmentSlotItem> {
        Vec::new()
    }

    /// Returns true if the entity has been marked for removal.
    fn is_removed(&self) -> bool {
        self.base().is_removed()
    }

    /// Returns whether this entity is alive for vanilla generic entity checks.
    fn is_alive(&self) -> bool {
        !self.is_removed()
    }

    /// Marks this entity as waiting for a prepared world change.
    fn begin_pending_world_change(&self) -> Option<PendingWorldChangeToken> {
        self.base().begin_pending_world_change()
    }

    /// Clears a pending world change if it still matches the provided token.
    fn finish_pending_world_change(&self, token: PendingWorldChangeToken) -> bool {
        self.base().finish_pending_world_change(token)
    }

    /// Returns true while this entity is waiting for a prepared world change.
    fn is_world_change_pending(&self) -> bool {
        self.base().is_world_change_pending()
    }

    /// Returns true if the given world-change token is still pending.
    fn is_world_change_token_pending(&self, token: PendingWorldChangeToken) -> bool {
        self.base().is_world_change_token_pending(token)
    }

    /// Returns whether this entity may enter a portal.
    ///
    /// Mirrors vanilla `Entity.canUsePortal`, including `LivingEntity` sleeping
    /// suppression.
    fn can_use_portal(&self, ignore_passenger: bool) -> bool {
        let entity_type = self.entity_type();
        if entity_type == &vanilla_entities::FISHING_BOBBER
            || entity_type == &vanilla_entities::ENDER_DRAGON
            || entity_type == &vanilla_entities::WITHER
        {
            return false;
        }

        (ignore_passenger || !self.is_passenger())
            && self.is_alive()
            && !self
                .as_living_entity()
                .is_some_and(LivingEntity::is_sleeping)
    }

    /// Returns vanilla's dimension-changing portal cooldown delay in ticks.
    ///
    /// Mirrors vanilla `getDimensionChangingDelay` overrides for players,
    /// projectiles, and base entities with a player passenger. Concrete vehicle
    /// implementations override this method directly.
    fn dimension_changing_delay(&self) -> i32 {
        let entity_type = self.entity_type();
        if self.as_player().is_some() {
            return 10;
        }
        if entity_type.is_projectile {
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
    fn removal_reason(&self) -> Option<RemovalReason> {
        self.base().removal_reason()
    }

    /// Marks the entity as removed with the given reason.
    fn set_removed(&self, reason: RemovalReason) {
        self.base().set_removed(reason);
    }

    /// Emits a vanilla game event from this entity's exact position.
    fn game_event(&self, event: GameEventRef) {
        let Some(world) = self.level() else {
            return;
        };
        world.game_event_at(
            event,
            self.position(),
            &GameEventContext::new(Some(self.as_entity_event_source()), None),
        );
    }

    /// Kills this entity using vanilla's living/non-living class split.
    ///
    /// `world` is vanilla's explicit `ServerLevel` argument. Living entities
    /// use it for damage processing, while death game events use the entity's
    /// attached world.
    fn kill(&self, world: &World) {
        if self.is_living_entity() {
            self.hurt(
                world,
                &DamageSource::environment(&vanilla_damage_types::GENERIC_KILL),
                f32::MAX,
            );
            return;
        }

        self.set_removed(RemovalReason::Killed);
        self.game_event(&vanilla_game_events::ENTITY_DIE);
    }

    /// Caches a live owner reference after restoring persisted owner-linked
    /// entities. Most entities do not store owner references.
    fn restore_owner_reference(&self, _owner: &SharedEntity) {}

    /// Sets the level callback for lifecycle events (movement, removal).
    fn set_level_callback(&self, callback: Arc<dyn EntityLevelCallback>) {
        self.base().set_level_callback(callback);
    }

    /// Called by leashables while this entity is their live leash holder.
    fn notify_leash_holder(&self, _leashable: &dyn Entity) {}

    /// Called when a leashable stops using this entity as its live leash holder.
    fn notify_leashee_removed(&self, _leashable: &dyn Entity) {}

    /// Called when a player touches this entity during nearby pickup processing.
    fn player_touch(self: Arc<Self>, _player: &Arc<Player>) {}

    /// Finds leashable mobs in vanilla's nearby leash scan whose holder is this entity.
    fn leashables_leashed_to(&self) -> Vec<SharedEntity> {
        self.leashables_leashed_to_holder_in_area(self.as_entity_event_source())
    }

    /// Finds leashable mobs in this entity's nearby leash scan whose holder is `holder`.
    fn leashables_leashed_to_holder_in_area(&self, holder: &dyn Entity) -> Vec<SharedEntity> {
        let Some(world) = self.level() else {
            return Vec::new();
        };
        let holder_id = holder.id();
        let scan_area = leash_scan_area(world_aabb_center(self.bounding_box()));
        world.get_entities_in_aabb_matching(&scan_area, |entity| {
            entity.as_mob().is_some_and(|mob| {
                mob.leash_holder()
                    .is_some_and(|holder| holder.id() == holder_id)
            })
        })
    }

    /// Transfers leashables currently held by `old_holder` to this entity.
    fn transfer_leashables_from_holder(&self, old_holder: &dyn Entity) -> bool {
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
    fn drop_all_leash_connections(&self, player: Option<&Player>) -> bool {
        let leashables = self.leashables_leashed_to();
        let mut dropped = !leashables.is_empty();

        if let Some(mob) = self.as_mob()
            && mob.is_leashed()
        {
            mob.drop_leash();
            dropped = true;
        }

        for leashable in leashables {
            if let Some(mob) = leashable.as_mob() {
                mob.drop_leash();
            }
        }

        if !dropped {
            return false;
        }

        if let Some(world) = self.level() {
            let source_entity = player.map(|player| player as &dyn Entity);
            world.game_event(
                &vanilla_game_events::SHEAR,
                self.block_position(),
                &GameEventContext::new(source_entity, None),
            );
        }
        true
    }

    /// Shears off all leash connections reachable from this entity.
    fn shear_off_all_leash_connections(&self, player: Option<&Player>) -> bool {
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

    /// Runs vanilla `Entity.attemptToShearEquipment`.
    fn attempt_to_shear_equipment(&self, player: &Player, hand: InteractionHand) -> bool {
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
            mob.refresh_equipment_attribute_modifiers(slot);
            mob.set_guaranteed_drop(slot);
            mob.set_persistence_required();

            if let Some(world) = self.level() {
                world.game_event(
                    &vanilla_game_events::SHEAR,
                    self.block_position(),
                    &GameEventContext::new(Some(player), None),
                );
            }
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

    /// Handles vanilla entity right-click interaction.
    fn interact(
        &self,
        player: &Player,
        hand: InteractionHand,
        location: DVec3,
    ) -> InteractionResult {
        self.interact_entity(player, hand, location)
    }

    /// Handles shared vanilla `Entity.interact` behavior.
    fn interact_entity(
        &self,
        player: &Player,
        hand: InteractionHand,
        _location: DVec3,
    ) -> InteractionResult {
        let Some(mob) = self.as_mob() else {
            return InteractionResult::Pass;
        };
        let is_alive = self.as_living_entity().is_none_or(LivingEntity::is_alive);
        if !is_alive {
            return InteractionResult::Pass;
        }

        if player.is_secondary_use_active()
            && mob.can_be_leashed()
            && self
                .as_living_entity()
                .is_none_or(|living| !LivingEntity::is_baby(living))
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

        if let Some(holder) = mob.leash_holder() {
            if holder.id() == player.id() {
                if player.has_infinite_materials() {
                    mob.remove_leash();
                } else {
                    mob.drop_leash();
                }

                if let Some(world) = self.level() {
                    world.game_event(
                        &vanilla_game_events::ENTITY_INTERACT,
                        self.block_position(),
                        &GameEventContext::new(Some(player), None),
                    );
                }
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
        if !holding_lead || !mob.can_have_a_leash_attached_to(player) {
            return InteractionResult::Pass;
        }

        let Some(world) = self.level() else {
            return InteractionResult::Pass;
        };
        let Some(player_entity) = world.get_entity_by_id(player.id()) else {
            return InteractionResult::Pass;
        };

        if mob.is_leashed() {
            mob.drop_leash();
        }
        if !mob.set_leashed_to(&player_entity) {
            return InteractionResult::Pass;
        }

        self.play_sound(&sound_events::ITEM_LEAD_TIED, 1.0, 1.0);
        player.inventory.lock().shrink_item_in_hand(hand, 1);
        InteractionResult::SuccessServer
    }

    /// Returns true for entities that implement vanilla living-entity behavior.
    fn is_living_entity(&self) -> bool {
        self.as_living_entity().is_some()
    }

    /// Returns whether this entity belongs to vanilla's `AbstractArrow` hierarchy.
    fn is_abstract_arrow(&self) -> bool {
        self.entity_type().is_abstract_arrow
    }

    /// Returns this entity as a projectile when it has projectile behavior.
    fn as_projectile(&self) -> Option<&dyn Projectile> {
        try_as_dyn::<Self, dyn Projectile>(self)
    }

    /// Returns this entity as a living entity when it has living behavior.
    ///
    /// Mirrors vanilla's frequent `instanceof LivingEntity` branches.
    fn as_living_entity(&self) -> Option<&dyn LivingEntity> {
        try_as_dyn::<Self, dyn LivingEntity>(self)
    }

    /// Returns this entity as an item frame when it has item-frame behavior.
    ///
    /// Mirrors vanilla's `instanceof ItemFrame` branches.
    fn as_item_frame(&self) -> Option<&dyn ItemFrame> {
        try_as_dyn::<Self, dyn ItemFrame>(self)
    }

    /// Returns this entity as a player when it is the concrete server player.
    fn as_player(&self) -> Option<&Player> {
        self.downcast_ref::<Player>()
    }

    /// Visits vanilla `Entity.getWeaponItem` without copying inventory state.
    fn with_weapon_item(&self, visitor: &mut dyn FnMut(Option<&ItemStack>)) {
        let Some(living) = self.as_living_entity() else {
            visitor(None);
            return;
        };
        living.with_equipment_slot(EquipmentSlot::MainHand, &mut |item| visitor(Some(item)));
    }

    /// Returns true for mobs with pathfinding navigation.
    fn is_pathfinder_mob(&self) -> bool {
        self.as_pathfinder_mob().is_some()
    }

    /// Returns this entity as a pathfinder mob when it has pathfinding behavior.
    ///
    /// Mirrors vanilla's frequent `instanceof PathfinderMob` branches.
    fn as_pathfinder_mob(&self) -> Option<&dyn PathfinderMob> {
        try_as_dyn::<Self, dyn PathfinderMob>(self)
    }

    /// Returns true for entities that implement vanilla mob behavior.
    fn is_mob(&self) -> bool {
        self.as_mob().is_some()
    }

    /// Returns this entity as a mob when it has mob behavior.
    ///
    /// Mirrors vanilla's frequent `instanceof Mob` branches.
    fn as_mob(&self) -> Option<&dyn Mob> {
        try_as_dyn::<Self, dyn Mob>(self)
    }

    /// Returns true for entities that implement vanilla animal behavior.
    fn is_animal(&self) -> bool {
        self.as_animal().is_some()
    }

    /// Returns this entity as an animal when it has animal behavior.
    ///
    /// Mirrors vanilla's frequent `instanceof Animal` branches.
    fn as_animal(&self) -> Option<&dyn Animal> {
        try_as_dyn::<Self, dyn Animal>(self)
    }

    /// Returns true for entities that implement vanilla item-steered boosts.
    fn is_item_steerable(&self) -> bool {
        self.as_item_steerable().is_some()
    }

    /// Returns this entity as item steerable when it has item-steering behavior.
    ///
    /// Mirrors vanilla's `instanceof ItemSteerable` branches.
    fn as_item_steerable(&self) -> Option<&dyn ItemSteerable> {
        try_as_dyn::<Self, dyn ItemSteerable>(self)
    }

    /// Returns true when vanilla `ServerEntity` should force velocity sync for fall flying.
    fn forces_fall_flying_velocity_sync(&self) -> bool {
        false
    }

    /// Returns true when movement is driven by serverbound movement packets.
    fn uses_client_movement_packets(&self) -> bool {
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
    fn is_server_driven_movement(&self) -> bool {
        !self.uses_client_movement_packets()
    }

    /// Returns true when vanilla allows this side to apply movement simulation side effects.
    fn can_simulate_movement(&self) -> bool {
        self.is_server_driven_movement()
    }

    /// Returns true when vanilla allows this side to run entity AI/travel logic.
    fn is_effective_ai(&self) -> bool {
        self.is_server_driven_movement()
    }

    /// Returns true when vanilla landing bounce should be suppressed.
    fn is_suppressing_bounce(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_shift_key_down)
    }

    /// Returns true when vanilla block step-on hooks should treat this entity as careful.
    fn is_stepping_carefully(&self) -> bool {
        self.is_suppressing_bounce()
    }

    /// Returns true when vanilla collision context should treat the entity as descending.
    fn is_descending(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_shift_key_down)
    }

    /// Sets the vanilla shift-key-down shared flag.
    fn set_shared_shift_key_down(&self, shift_key_down: bool) {
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_shift_key_down(shift_key_down);
        }
    }

    /// Sets the vanilla swimming shared flag.
    fn set_shared_swimming(&self, swimming: bool) {
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_swimming(swimming);
        }
    }

    /// Updates the vanilla swimming shared flag.
    ///
    /// Mirrors vanilla `Entity.updateSwimming`.
    fn update_swimming(&self) {
        self.default_update_swimming();
    }

    /// Shared body of vanilla `Entity.updateSwimming` for player overrides.
    fn default_update_swimming(&self) {
        let Some(world) = self.level() else {
            return;
        };

        let block_fluid = get_fluid_state(&world, self.block_position());
        let swimming = select_swimming_state(
            self.is_swimming(),
            SwimmingEnvironment {
                sprinting: self
                    .as_living_entity()
                    .is_some_and(LivingEntity::is_sprinting),
                passenger: self.is_passenger(),
                in_water: self.is_in_water(),
                under_water: self.is_under_water(),
                block_fluid_is_water: block_fluid.is_water(),
            },
        );
        self.set_shared_swimming(swimming);
    }

    /// Sets the vanilla sprinting shared flag.
    fn set_shared_sprinting(&self, sprinting: bool) {
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_sprinting(sprinting);
        }
    }

    /// Sets the vanilla fall-flying shared flag.
    fn set_shared_fall_flying(&self, fall_flying: bool) {
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_fall_flying(fall_flying);
        }
    }

    /// Returns vanilla `PowderSnowBlock.canEntityWalkOnPowderSnow`.
    fn default_can_walk_on_powder_snow(&self) -> bool {
        REGISTRY.entity_types.is_in_tag(
            self.entity_type(),
            &EntityTypeTag::POWDER_SNOW_WALKABLE_MOBS,
        )
    }

    /// Returns whether this entity can walk on powder snow.
    fn can_walk_on_powder_snow(&self) -> bool {
        self.default_can_walk_on_powder_snow()
    }

    /// Returns whether vanilla excludes this vehicle from floating kicks.
    fn is_flying_vehicle(&self) -> bool {
        false
    }

    /// Returns true if vanilla rules consider this entity to be on a climbable block.
    fn on_climbable(&self) -> bool {
        false
    }

    /// Returns the movement vector vanilla exposes for block-contact logic.
    fn known_movement(&self) -> DVec3 {
        if let Some(controller) = self.controlling_passenger()
            && !self.is_removed()
            && controller.entity_type() == &vanilla_entities::PLAYER
        {
            return controller.known_movement();
        }

        self.velocity()
    }

    /// Returns the base-tick displacement vanilla exposes as `getKnownSpeed`.
    fn known_speed(&self) -> DVec3 {
        if let Some(controller) = self.controlling_passenger()
            && !self.is_removed()
            && controller.entity_type() == &vanilla_entities::PLAYER
        {
            return controller.known_speed();
        }

        self.base().known_speed()
    }

    /// Returns vanilla `Entity.tickCount`.
    fn tick_count(&self) -> i32 {
        self.base().tick_count()
    }

    /// Advances vanilla `Entity.tickCount`.
    fn advance_tick_count(&self) {
        self.base().advance_tick_count();
    }

    /// Returns vanilla small and big fall sounds for this entity.
    fn fall_sounds(&self) -> (SoundEventRef, SoundEventRef) {
        (
            &sound_events::ENTITY_GENERIC_SMALL_FALL,
            &sound_events::ENTITY_GENERIC_BIG_FALL,
        )
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

    /// Rotates this entity to face a fixed position.
    fn look_at(&self, from_anchor: EntityAnchor, target: DVec3) {
        apply_entity_look_at(self.as_entity_event_source(), from_anchor, target);
    }

    /// Rotates this entity to follow an anchored point on another entity.
    fn look_at_entity(
        &self,
        from_anchor: EntityAnchor,
        target: &dyn Entity,
        target_anchor: EntityAnchor,
    ) {
        self.look_at(from_anchor, target_anchor.position(target));
    }

    /// Returns vanilla `Entity.getYHeadRot`.
    fn head_yaw(&self) -> f32 {
        self.as_living_entity()
            .map_or(0.0, LivingEntity::y_head_rot)
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

    /// Returns vanilla `Entity.getFluidJumpThreshold()`.
    fn get_fluid_jump_threshold(&self) -> f64 {
        if self.get_eye_height() < 0.4 {
            0.0
        } else {
            0.4
        }
    }

    /// Gets the Y coordinate of the entity's eyes.
    ///
    /// Equivalent to vanilla's `Entity.getEyeY()`.
    fn get_eye_y(&self) -> f64 {
        self.position().y + self.get_eye_height()
    }

    /// Mirrors vanilla `Entity.isInWall`.
    fn is_in_wall(&self) -> bool {
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

    /// Calculates vanilla `Entity.calculateViewVector()`.
    fn calculate_view_vector(&self, pitch_degrees: f32, yaw_degrees: f32) -> DVec3 {
        let pitch = pitch_degrees.to_radians();
        let yaw = -yaw_degrees.to_radians();
        let yaw_cos = yaw.cos();
        let yaw_sin = yaw.sin();
        let pitch_cos = pitch.cos();
        let pitch_sin = pitch.sin();
        DVec3::new(
            f64::from(yaw_sin * pitch_cos),
            f64::from(-pitch_sin),
            f64::from(yaw_cos * pitch_cos),
        )
    }

    /// Returns vanilla `Entity.getLookAngle()`.
    fn look_angle(&self) -> DVec3 {
        let (yaw, pitch) = self.rotation();
        self.calculate_view_vector(pitch, yaw)
    }

    /// Returns the vanilla offset for the hand holding `item`.
    ///
    /// Only players have a hand offset. If both hands contain the item, vanilla
    /// chooses the main hand; otherwise an offhand-only item uses the opposite
    /// arm from the player's configured main arm.
    fn hand_holding_item_angle(&self, item: ItemRef) -> DVec3 {
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
    fn velocity(&self) -> DVec3 {
        self.base().velocity()
    }

    /// Sets the entity's velocity.
    fn set_velocity(&self, velocity: DVec3) {
        self.base().set_velocity(velocity);
    }

    /// Returns true when vanilla `ServerEntity` should consider sending velocity.
    fn needs_velocity_sync(&self) -> bool {
        self.base().needs_velocity_sync()
    }

    /// Marks velocity for vanilla `ServerEntity` synchronization.
    fn mark_velocity_sync(&self) {
        self.base().mark_velocity_sync();
    }

    /// Clears the vanilla velocity sync marker after send processing.
    fn clear_velocity_sync(&self) {
        self.base().clear_velocity_sync();
    }

    /// Returns true when vanilla hurt-marked velocity sync is pending.
    fn hurt_marked(&self) -> bool {
        self.base().hurt_marked()
    }

    /// Marks this entity as hurt for vanilla self-inclusive motion sync.
    fn mark_hurt(&self) {
        self.base().mark_hurt();
    }

    /// Clears the vanilla hurt-marked motion sync flag.
    fn clear_hurt_mark(&self) {
        self.base().clear_hurt_mark();
    }

    /// Returns accumulated vanilla fall distance.
    fn fall_distance(&self) -> f64 {
        self.base().fall_distance()
    }

    /// Returns whether this entity is currently inside powder snow.
    fn is_in_powder_snow(&self) -> bool {
        self.base().is_in_powder_snow()
    }

    /// Returns whether this entity was inside powder snow during the previous base tick.
    fn was_in_powder_snow(&self) -> bool {
        self.base().was_in_powder_snow()
    }

    /// Sets accumulated vanilla fall distance.
    fn set_fall_distance(&self, fall_distance: f64) {
        self.base().set_fall_distance(fall_distance);
    }

    /// Resets accumulated vanilla fall distance.
    fn reset_fall_distance(&self) {
        self.base().reset_fall_distance();
    }

    /// Mirrors vanilla `Entity.checkFallDistanceAccumulation()`.
    fn check_fall_distance_accumulation(&self) {
        if self.velocity().y > -0.5 && self.fall_distance() > 1.0 {
            self.set_fall_distance(1.0);
        }
    }

    /// Returns the current vanilla fire/freeze state.
    fn fire_freeze_state(&self) -> EntityFireFreezeState {
        self.base().fire_freeze_state()
    }

    /// Returns vanilla `remainingFireTicks`.
    fn remaining_fire_ticks(&self) -> i32 {
        self.base().remaining_fire_ticks()
    }

    /// Sets vanilla `remainingFireTicks`.
    fn set_remaining_fire_ticks(&self, remaining_fire_ticks: i32) {
        self.base().set_remaining_fire_ticks(
            self.remaining_fire_ticks_cap()
                .map_or(remaining_fire_ticks, |cap| remaining_fire_ticks.min(cap)),
        );
        self.sync_base_fire_freeze_entity_data();
    }

    /// Returns synchronized vanilla `TicksFrozen`.
    fn ticks_frozen(&self) -> i32 {
        self.base().ticks_frozen()
    }

    /// Sets synchronized vanilla `TicksFrozen`.
    fn set_ticks_frozen(&self, ticks_frozen: i32) {
        self.base().set_ticks_frozen(ticks_frozen);
        self.sync_base_fire_freeze_entity_data();
    }

    /// Returns whether this entity is immune to fire effects and fire damage.
    fn fire_immune(&self) -> bool {
        self.entity_type().fire_immune
    }

    /// Returns vanilla fire immunity cooldown ticks after not being ignited.
    fn fire_immune_ticks(&self) -> i32 {
        0
    }

    /// Returns whether vanilla should play this entity's lava hurt sound.
    fn should_play_lava_hurt_sound(&self) -> bool {
        true
    }

    /// Applies vanilla lava-contact damage after lava ignition effects.
    fn lava_hurt(&self) {
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
            let pitch = 2.0 + rand::random::<f32>() * 0.4;
            self.play_sound(&sound_events::ENTITY_GENERIC_BURN, 0.4, pitch);
        }
    }

    /// Maximum vanilla `remainingFireTicks` this entity can store.
    fn remaining_fire_ticks_cap(&self) -> Option<i32> {
        None
    }

    /// Returns extra ticks added by fire-block ignition before 8-second ignition.
    fn fire_ignite_extra_ticks(&self) -> i32 {
        0
    }

    /// Returns whether the entity is on fire on the server.
    fn is_on_fire(&self) -> bool {
        self.base().is_on_fire(self.fire_immune())
    }

    /// Returns vanilla `hasVisualFire`.
    fn has_visual_fire(&self) -> bool {
        self.base().has_visual_fire()
    }

    /// Returns whether the entity has any frozen ticks.
    fn is_freezing(&self) -> bool {
        self.base().is_freezing()
    }

    /// Returns vanilla `Entity.canFreeze()` without living-equipment overrides.
    fn default_can_freeze(&self) -> bool {
        !REGISTRY.entity_types.is_in_tag(
            self.entity_type(),
            &EntityTypeTag::FREEZE_IMMUNE_ENTITY_TYPES,
        )
    }

    /// Returns whether this entity may accumulate frozen ticks.
    fn can_freeze(&self) -> bool {
        self.default_can_freeze()
    }

    /// Returns vanilla `getTicksRequiredToFreeze`.
    fn ticks_required_to_freeze(&self) -> i32 {
        DEFAULT_TICKS_REQUIRED_TO_FREEZE
    }

    /// Returns whether this entity has reached full-freeze duration.
    fn is_fully_frozen(&self) -> bool {
        self.base().is_fully_frozen(self.ticks_required_to_freeze())
    }

    /// Returns vanilla `Entity.getPercentFrozen`.
    fn percent_frozen(&self) -> f32 {
        let ticks_required = self.ticks_required_to_freeze();
        self.ticks_frozen().min(ticks_required) as f32 / ticks_required as f32
    }

    /// Clears accumulated freezing.
    fn clear_freeze(&self) {
        self.base().clear_freeze();
        self.sync_base_fire_freeze_entity_data();
    }

    /// Clears fire without resetting the vanilla fire immunity cooldown.
    fn clear_fire(&self) {
        self.base().clear_fire();
        self.sync_base_fire_freeze_entity_data();
    }

    /// Ignites this entity for a vanilla tick duration.
    fn ignite_for_ticks(&self, number_of_ticks: i32) {
        self.base()
            .ignite_for_ticks(number_of_ticks, self.remaining_fire_ticks_cap());
        self.sync_base_fire_freeze_entity_data();
    }

    /// Projects base fire/freeze state into generated synced entity data.
    fn sync_base_fire_freeze_entity_data(&self) {
        let Some(synced_data) = self.synced_data() else {
            return;
        };

        synced_data.set_base_ticks_frozen(self.ticks_frozen());
        synced_data.set_base_on_fire_flag(self.is_on_fire() || self.has_visual_fire());
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

    /// Returns true if this entity is currently touching water.
    fn is_in_water(&self) -> bool {
        self.fluid_contact().water_height() > 0.0
    }

    /// Returns true if this entity is currently touching lava.
    fn is_in_lava(&self) -> bool {
        self.fluid_contact().lava_height() > 0.0
    }

    /// Returns true if this entity's eyes are currently inside water.
    fn is_eye_in_water(&self) -> bool {
        self.fluid_contact().eye_in_water()
    }

    /// Returns true if this entity's eyes are currently inside lava.
    fn is_eye_in_lava(&self) -> bool {
        self.fluid_contact().eye_in_lava()
    }

    /// Returns vanilla underwater state.
    fn is_under_water(&self) -> bool {
        self.base().was_eye_in_water() && self.is_in_water()
    }

    /// Returns cached fluid contact from the last entity fluid refresh.
    fn fluid_contact(&self) -> EntityFluidContact {
        self.base().fluid_contact()
    }

    /// Refreshes cached fluid contact from this entity's current bounding box.
    fn refresh_fluid_contact(&self) -> EntityFluidContact {
        self.scan_and_store_fluid_contact(false)
    }

    /// Refreshes cached fluid contact with vanilla base-tick eye-water history.
    fn refresh_fluid_contact_for_base_tick(&self) -> EntityFluidContact {
        self.scan_and_store_fluid_contact(true)
    }

    /// Scans current fluid contact and stores it on the entity base.
    fn scan_and_store_fluid_contact(&self, advance_eye_water_history: bool) -> EntityFluidContact {
        let Some(world) = self.level() else {
            let contact = EntityFluidContact::default();
            if advance_eye_water_history {
                self.base().set_fluid_contact_for_base_tick(contact);
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
            self.base().set_fluid_contact_for_base_tick(contact);
            self.apply_fluid_current_for_base_tick(&world, contact);
        } else {
            self.base().set_fluid_contact(contact);
        }
        contact
    }

    /// Applies vanilla water/lava current impulses from the base-tick fluid scan.
    fn apply_fluid_current_for_base_tick(&self, world: &Arc<World>, contact: EntityFluidContact) {
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
    fn apply_fluid_current_impulse(&self, impulse: DVec3) {
        if impulse.length_squared() > 0.0 {
            self.push_impulse(impulse);
        }
    }

    /// Returns true if this entity type ignores vanilla fall damage.
    fn is_fall_damage_immune(&self) -> bool {
        REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::FALL_DAMAGE_IMMUNE)
    }

    /// Propagates vanilla fall-damage handling to passengers.
    fn propagate_fall_to_passengers(
        &self,
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
    ) {
        for passenger in self.passengers() {
            passenger.cause_fall_damage(fall_distance, damage_modifier, source);
        }
    }

    /// Applies vanilla fall damage.
    ///
    /// Living entities use the shared `LivingEntity` damage path; base entities
    /// only propagate to passengers and return `false`.
    fn cause_fall_damage(
        &self,
        fall_distance: f64,
        damage_modifier: f32,
        source: &DamageSource,
    ) -> bool {
        if let Some(living) = self.as_living_entity() {
            return living.cause_living_fall_damage(fall_distance, damage_modifier, source);
        }
        if self.is_fall_damage_immune() {
            return false;
        }

        self.propagate_fall_to_passengers(fall_distance, damage_modifier, source);
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

    /// Updates item-style `noPhysics` from the entity's current collision state.
    fn update_no_physics_from_current_collision(&self) {
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
    fn move_towards_closest_space(&self, x: f64, y: f64, z: f64) {
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

        let speed = f64::from(rand::random::<f32>().mul_add(0.2, 0.1));
        let step = direction_step(closest_direction);
        let scaled_velocity = self.velocity() * 0.75;
        let next_velocity = match closest_direction.axis() {
            Axis::X => DVec3::new(step * speed, scaled_velocity.y, scaled_velocity.z),
            Axis::Y => DVec3::new(scaled_velocity.x, step * speed, scaled_velocity.z),
            Axis::Z => DVec3::new(scaled_velocity.x, scaled_velocity.y, step * speed),
        };
        self.set_velocity(next_velocity);
    }

    /// Default vanilla stuck-in-block movement for the next movement pass.
    fn default_make_stuck_in_block(&self, _state: BlockStateId, speed_multiplier: DVec3) {
        self.base().make_stuck_in_block(speed_multiplier);
    }

    /// Applies vanilla stuck-in-block movement for the next movement pass.
    fn make_stuck_in_block(&self, state: BlockStateId, speed_multiplier: DVec3) {
        self.default_make_stuck_in_block(state, speed_multiplier);
    }

    /// Applies current block-contact effects to this entity.
    ///
    /// Mirrors the shared ownership boundary of vanilla `Entity.applyEffectsFromBlocks`.
    fn apply_effects_from_blocks(&self) {
        let entity = self.as_entity_event_source();
        let movements = self.base().take_movements_for_block_effects();
        apply_effects_from_block_movements(entity, &movements);
    }

    /// Applies block-contact effects for one explicit movement segment.
    ///
    /// Mirrors vanilla's `Entity.applyEffectsFromBlocks(Vec3, Vec3)` overload,
    /// which does not finalize or clear the entity's normal movement trace.
    fn apply_effects_from_blocks_between(&self, from: DVec3, to: DVec3) {
        let entity = self.as_entity_event_source();
        apply_effects_from_block_movements(entity, &[EntityMovement::new(from, to)]);
    }

    /// Replays the last finalized block-contact movement list.
    fn apply_effects_from_blocks_for_last_movements(&self) {
        let entity = self.as_entity_event_source();
        let movements = self.base().last_movements_for_block_effects();
        apply_effects_from_block_movements(entity, &movements);
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

    /// Default final state application for accepted client-authored movement.
    ///
    /// Mirrors the shared tail of vanilla player and controlled-vehicle movement
    /// handling after rollback/collision validation has accepted the target.
    fn default_apply_accepted_client_movement(
        &self,
        world: &Arc<World>,
        accepted: AcceptedClientMovement,
    ) -> Result<AcceptedClientMovementOutcome, EntityMoveError> {
        if let Some(position) = accepted.position {
            self.try_set_position(position)?;
            self.refresh_fluid_contact();
        }

        self.set_rotation(accepted.rotation);
        self.set_on_ground_with_movement(
            accepted.on_ground,
            accepted.horizontal_collision,
            accepted.movement,
        );
        if self.do_check_fall_damage(accepted.movement, accepted.on_ground, world) {
            return Ok(AcceptedClientMovementOutcome::Handled);
        }
        if accepted.reset_fall_distance {
            self.reset_fall_distance();
        }

        Ok(AcceptedClientMovementOutcome::Applied)
    }

    /// Applies final state accepted from a client-authored movement packet.
    fn apply_accepted_client_movement(
        &self,
        world: &Arc<World>,
        accepted: AcceptedClientMovement,
    ) -> Result<AcceptedClientMovementOutcome, EntityMoveError> {
        self.default_apply_accepted_client_movement(world, accepted)
    }

    /// Applies final state accepted from a controlled-vehicle movement packet.
    fn apply_accepted_client_vehicle_movement(
        &self,
        world: &Arc<World>,
        mut accepted: AcceptedClientMovement,
    ) -> Result<AcceptedClientMovementOutcome, EntityMoveError> {
        accepted.horizontal_collision = self.horizontal_collision();
        accepted.reset_fall_distance = false;
        self.default_apply_accepted_client_movement(world, accepted)
    }

    /// Attempts to set the entity's position through world lifecycle validation.
    #[must_use = "movement commits can fail when world entity state rejects the update"]
    fn try_set_position(&self, pos: DVec3) -> Result<(), EntityMoveError> {
        self.base().try_set_position(pos)
    }

    /// Sets the vanilla movement-trace old position to the current position.
    fn set_old_position_to_current(&self) {
        self.base().set_old_position_to_current();
    }

    /// Sets the vanilla movement-trace old position explicitly.
    fn set_old_position(&self, old_position: DVec3) {
        self.base().set_old_position(old_position);
    }

    /// Removes the latest movement segment recorded this tick.
    fn remove_latest_movement_recording(&self) {
        self.base().remove_latest_movement_recording();
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

    /// Returns vanilla `Entity.getBlockJumpFactor()`.
    #[expect(
        clippy::float_cmp,
        reason = "intentional: vanilla checks static block jump factors against 1.0"
    )]
    fn block_jump_factor(&self) -> f32 {
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
    fn pose(&self) -> EntityPose {
        self.base().pose()
    }

    /// Returns vanilla dimensions for a physical pose.
    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let dimensions = self.entity_type().dimensions;
        let Some(living) = self.as_living_entity() else {
            return dimensions;
        };

        if self.entity_type().fixed {
            dimensions
        } else {
            dimensions.scale(living.get_age_scale() * living.get_scale())
        }
    }

    /// Refreshes dimensions for the current physical pose.
    fn refresh_dimensions(&self) {
        let pose = self.pose();
        self.base()
            .set_pose_and_dimensions(pose, self.dimensions_for_pose(pose));
        // TODO: Fudge position after growth once free-position probing exists.
    }

    /// Sets the physical pose and synchronized pose metadata.
    fn set_pose(&self, pose: EntityPose) {
        self.base()
            .set_pose_and_dimensions(pose, self.dimensions_for_pose(pose));
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_pose(pose);
        }
    }

    /// Returns whether vanilla currently considers this entity crouching.
    fn is_crouching(&self) -> bool {
        self.pose() == EntityPose::Sneaking
    }

    /// Returns whether vanilla currently considers this entity swimming.
    fn is_swimming(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_swimming)
    }

    /// Returns whether this entity is on rails.
    fn is_on_rails(&self) -> bool {
        false
    }

    /// Returns whether a block state is climbable for base movement effects.
    fn is_state_climbable(&self, state: BlockStateId) -> bool {
        let block = state.get_block();
        block.has_tag(&BlockTag::CLIMBABLE) || block == &vanilla_blocks::POWDER_SNOW
    }

    /// Returns vanilla movement side effects emitted by this entity.
    fn movement_emission(&self) -> EntityMovementEmission {
        EntityMovementEmission::All
    }

    /// Returns whether this entity may modify the world at a position.
    ///
    /// Vanilla `Entity.mayInteract` defaults to true; player-like entities can
    /// apply world permission checks through overrides.
    fn may_interact(&self, _world: &World, _pos: BlockPos) -> bool {
        true
    }

    /// Returns the synchronized vanilla `Air` value.
    fn air_supply(&self) -> i32 {
        self.base().air_supply()
    }

    /// Sets the synchronized vanilla `Air` value.
    fn set_air_supply(&self, air_supply: i32) {
        self.base().set_air_supply(air_supply);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_air_supply(air_supply);
        }
    }

    /// Returns this entity's maximum vanilla air supply.
    fn max_air_supply(&self) -> i32 {
        DEFAULT_MAX_AIR_SUPPLY
    }

    /// Returns the vanilla portal cooldown in ticks.
    fn portal_cooldown(&self) -> i32 {
        self.base().portal_cooldown()
    }

    /// Sets the vanilla portal cooldown in ticks.
    fn set_portal_cooldown(&self, portal_cooldown: i32) {
        self.base().set_portal_cooldown(portal_cooldown);
    }

    /// Returns whether the entity is on vanilla portal cooldown.
    fn is_on_portal_cooldown(&self) -> bool {
        self.base().is_on_portal_cooldown()
    }

    /// Resets vanilla portal cooldown to this entity's dimension-changing delay.
    ///
    /// Mirrors vanilla `Entity.setPortalCooldown()`.
    fn reset_portal_cooldown(&self) {
        self.set_portal_cooldown(self.dimension_changing_delay());
    }

    /// Marks this entity as inside a vanilla portal during the current tick.
    ///
    /// Mirrors vanilla `Entity.setAsInsidePortal`.
    fn set_as_inside_portal(&self, portal: PortalKind, entry_position: BlockPos) {
        if self.is_on_portal_cooldown() {
            self.reset_portal_cooldown();
            return;
        }

        self.base().set_as_inside_portal(portal, entry_position);
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

    /// Returns this entity's vanilla sound source category.
    fn sound_source(&self) -> SoundSource {
        SoundSource::Neutral
    }

    /// Returns this entity's vanilla swim sound.
    fn swim_sound(&self) -> SoundEventRef {
        &sound_events::ENTITY_GENERIC_SWIM
    }

    /// Returns whether sounds from this entity are suppressed.
    fn is_silent(&self) -> bool {
        self.base().silent()
    }

    /// Sets whether sounds from this entity are suppressed.
    fn set_silent(&self, silent: bool) {
        self.base().set_silent(silent);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_silent(silent);
        }
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

    /// Plays an entity sound at the entity's exact position.
    fn play_sound(&self, sound: SoundEventRef, volume: f32, pitch: f32) {
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

    /// Plays vanilla's extinguished-on-fire entity sound.
    fn play_entity_on_fire_extinguished_sound(&self) {
        let pitch = 1.6 + (rand::random::<f32>() - rand::random::<f32>()) * 0.4;
        self.play_sound(&sound_events::ENTITY_GENERIC_EXTINGUISH_FIRE, 0.7, pitch);
    }

    /// Plays the base vanilla step sound for a block.
    fn play_step_sound(&self, _pos: BlockPos, block_state: BlockStateId) {
        self.play_block_step_sound(block_state);
    }

    /// Plays a vanilla block step sound at this entity's current position.
    fn play_block_step_sound(&self, block_state: BlockStateId) {
        let sound_type = block_state.get_block().config.sound_type;
        self.play_sound(
            sound_type.step_sound,
            sound_type.volume * 0.15,
            sound_type.pitch,
        );
    }

    /// Plays vanilla's muffled secondary step sound.
    fn play_muffled_step_sound(&self, block_state: BlockStateId) {
        let sound_type = block_state.get_block().config.sound_type;
        self.play_sound(
            sound_type.step_sound,
            sound_type.volume * 0.05,
            sound_type.pitch * 0.8,
        );
    }

    /// Plays vanilla's combination primary and secondary step sounds.
    fn play_combination_step_sounds(
        &self,
        primary_step_sound: BlockStateId,
        secondary_step_sound: BlockStateId,
    ) {
        self.play_block_step_sound(primary_step_sound);
        self.play_muffled_step_sound(secondary_step_sound);
    }

    /// Plays vanilla walking step sounds, including amethyst chimes.
    fn walking_step_sound(&self, pos: BlockPos, block_state: BlockStateId) {
        self.play_step_sound(pos, block_state);
        if block_state
            .get_block()
            .has_tag(&BlockTag::CRYSTAL_SOUND_BLOCKS)
        {
            self.play_amethyst_step_sound();
        }
    }

    /// Plays vanilla amethyst step chime when its cooldown permits it.
    fn play_amethyst_step_sound(&self) {
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
    fn water_swim_sound(&self) {
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
    fn play_swim_sound(&self, volume: f32) {
        let pitch = 1.0 + (rand::random::<f32>() - rand::random::<f32>()) * 0.4;
        self.play_sound(self.swim_sound(), volume, pitch);
    }

    /// Returns whether the entity is currently flapping.
    fn is_flapping(&self) -> bool {
        false
    }

    /// Runs entity-specific flap side effects.
    fn on_flap(&self) {}

    /// Processes vanilla flap movement side effects.
    fn process_flapping_movement(&self) {
        if !self.is_flapping() {
            return;
        }

        self.on_flap();
        if self.movement_emission().emits_events()
            && let Some(world) = self.level()
        {
            world.game_event_at(
                &vanilla_game_events::FLAP,
                self.position(),
                &GameEventContext::new(Some(self.as_entity_event_source()), None),
            );
        }
    }

    /// Returns the next step threshold after movement side effects are produced.
    fn next_step(&self) -> f32 {
        self.base().movement_progress().move_dist().floor() + 1.0
    }

    /// Applies vanilla movement sounds and game events after a completed move.
    fn apply_movement_emission_and_play_sound(
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
                    world.game_event_at(
                        &vanilla_game_events::SWIM,
                        self.position(),
                        &GameEventContext::new(Some(self.as_entity_event_source()), None),
                    );
                }
            }
        } else if supporting_state.get_block() == &vanilla_blocks::AIR {
            self.process_flapping_movement();
        }
    }

    /// Applies movement side effects after vanilla collision and landing updates.
    fn apply_movement_side_effects_after_move(&self, world: &World, actual_movement: DVec3) {
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
    fn vibration_and_sound_effects_from_block(
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

    /// Maximum height this entity can step up during normal movement.
    fn max_up_step(&self) -> f32 {
        0.0
    }

    /// Whether movement should apply player-style sneak edge prevention.
    fn backs_off_from_edge(&self) -> bool {
        false
    }

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
        self.base().no_gravity()
    }

    /// Sets the shared vanilla `NoGravity` flag.
    fn set_no_gravity(&self, no_gravity: bool) {
        self.base().set_no_gravity(no_gravity);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_no_gravity(no_gravity);
        }
    }

    /// Returns the shared vanilla `Invulnerable` flag.
    fn is_invulnerable(&self) -> bool {
        self.base().invulnerable()
    }

    /// Sets the shared vanilla `Invulnerable` flag.
    fn set_invulnerable(&self, invulnerable: bool) {
        self.base().set_invulnerable(invulnerable);
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

    /// Applies vanilla `Entity.moveRelative()`.
    fn move_relative(&self, speed: f32, input: DVec3) {
        let yaw = self.rotation().0;
        self.set_velocity(self.velocity() + get_input_vector(input, speed, yaw));
    }

    /// Moves the entity without collision physics.
    fn move_without_physics(&self, delta: DVec3) -> Option<MoveResult> {
        let final_position = self.position() + delta;
        if let Err(error) = self.try_set_position(final_position) {
            log::debug!(
                "Rejected no-physics movement for entity {}: {error}",
                self.id()
            );
            return None;
        }
        self.base().clear_collision_flags();
        self.refresh_fluid_contact();

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
    /// Mirrors vanilla's `Entity.move(MoverType, Vec3)`.
    /// Updates position, `on_ground`, velocity (on collision), and returns collision info.
    fn move_entity(&self, mover_type: MoverType, delta: DVec3) -> Option<MoveResult> {
        let world = self.level()?;
        if self.no_physics() {
            return self.move_without_physics(delta);
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
        self.refresh_fluid_contact();

        if self.is_server_driven_movement() && self.apply_fall_damage_after_move(&result, &world) {
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
    fn apply_fall_damage_after_move(&self, result: &MoveResult, world: &Arc<World>) -> bool {
        self.do_check_fall_damage(result.actual_movement, result.on_ground, world)
    }

    /// Resets fall distance when vanilla's fall-damage-resetting clip hits.
    fn reset_fall_distance_on_resetting_clip(&self, world: &Arc<World>, movement: DVec3) {
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

    /// Refreshes vanilla supporting-block state before fall-damage side effects.
    fn refresh_supporting_block_for_fall_damage(&self, movement: DVec3, on_ground: bool) {
        let ground_contact = self.ground_contact_after_movement(on_ground, Some(movement));
        self.base().set_ground_contact(ground_contact);
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
                EntityFallOnContext::from_entity(fall_distance, self.as_entity_event_source());
            if let Some(fall_damage) = behavior.fall_on(on_state, world, pos, fall_context) {
                let damage_applied = self.cause_fall_damage(
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

    /// Spawns an item at this entity's location plus a vanilla attachment offset.
    fn spawn_at_location_with_offset(
        &self,
        item: ItemStack,
        offset: DVec3,
    ) -> Option<Arc<entities::ItemEntity>> {
        let world = self.level()?;
        world.spawn_item(self.position() + offset, item)
    }

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
    fn load_additional(&self, _nbt: BorrowedNbtCompoundView<'_, '_>) {}

    /// Applies damage to this entity.
    ///
    /// Vanilla: `Entity.hurtServer()` — overridden by `LivingEntity` (complex
    /// armor/effects/invulnerability logic) and `ItemEntity` (health decrement
    /// and discard). `world` must preserve vanilla's explicit `ServerLevel`
    /// argument rather than being inferred from the target. Default returns
    /// `false` (entity ignores damage).
    #[expect(
        unused_variables,
        reason = "default trait impl; parameters used by overrides"
    )]
    fn hurt(&self, world: &World, source: &DamageSource, amount: f32) -> bool {
        false
    }
}

pub(crate) fn apply_entity_look_at(entity: &dyn Entity, from_anchor: EntityAnchor, target: DVec3) {
    let rotation = look_at_rotation(from_anchor.position(entity), target);
    entity.set_rotation(rotation);
    let rotation = entity.rotation();
    if let Some(living) = entity.as_living_entity() {
        living.set_y_head_rot(rotation.0);
    }
    entity.base().set_old_rotation_to_current();
}

fn look_at_rotation(from: DVec3, target: DVec3) -> (f32, f32) {
    let delta = target - from;
    let horizontal = delta.x.hypot(delta.z);
    let pitch = wrap_look_at_degrees(-delta.y.atan2(horizontal).to_degrees() as f32);
    let yaw = wrap_look_at_degrees(delta.z.atan2(delta.x).to_degrees() as f32 - 90.0);
    (yaw, pitch)
}

fn wrap_look_at_degrees(mut degrees: f32) -> f32 {
    degrees %= 360.0;
    if degrees >= 180.0 {
        degrees -= 360.0;
    }
    if degrees < -180.0 {
        degrees += 360.0;
    }
    degrees
}

#[cfg(test)]
mod look_at_tests {
    use glam::DVec3;

    use super::look_at_rotation;

    #[test]
    fn look_at_rotation_matches_vanilla_axes() {
        assert_eq!(
            look_at_rotation(DVec3::ZERO, DVec3::new(0.0, 0.0, 1.0)),
            (0.0, 0.0)
        );
        assert_eq!(
            look_at_rotation(DVec3::ZERO, DVec3::new(1.0, 0.0, 0.0)),
            (-90.0, 0.0)
        );
        assert_eq!(
            look_at_rotation(DVec3::ZERO, DVec3::new(0.0, 1.0, 1.0)),
            (0.0, -45.0)
        );
        assert_eq!(
            look_at_rotation(DVec3::ZERO, DVec3::new(-1.0, 0.0, -1.0)),
            (135.0, 0.0)
        );
    }
}
