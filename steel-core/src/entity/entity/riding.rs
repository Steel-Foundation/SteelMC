//! Vehicles, passengers and rider positions.

use super::*;

/// Vehicles, passengers and rider positions.
///
/// Implemented for every [`Entity`]; entity types change behavior through
/// [`EntityHooks`], never by implementing this trait.
pub trait EntityRiding: Entity {
    /// Gets the vehicle this entity is riding, if present.
    fn vehicle(&self) -> Option<SharedEntity> {
        self.base().vehicle()
    }

    /// Returns the vehicle this entity directly controls, if any.
    fn controlled_vehicle(&self) -> Option<SharedEntity> {
        let vehicle = self.vehicle()?;
        let controlled_by_self = vehicle
            .controlling_passenger()
            .is_some_and(|passenger| passenger.id() == self.id());
        controlled_by_self.then_some(vehicle)
    }

    /// Returns whether this entity is riding another entity.
    fn is_passenger(&self) -> bool {
        self.vehicle().is_some()
    }

    /// Returns whether vanilla allows this entity to start riding `vehicle`.
    fn can_ride(&self, _vehicle: &dyn Entity) -> bool {
        !self.is_discrete() && self.base().boarding_cooldown() <= 0
    }

    /// Default for [`Entity::stop_riding`].
    fn stop_riding_default(&self) {
        self.base().stop_riding();
    }

    /// Default for [`Entity::start_riding`].
    fn start_riding_default(&self, entity_to_ride: &SharedEntity) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        let Some(passenger) = world.get_entity_by_id(self.id()) else {
            return false;
        };
        start_riding_entities(&passenger, entity_to_ride)
    }

    /// Gets this entity's direct passengers.
    fn passengers(&self) -> Vec<SharedEntity> {
        self.base().passengers()
    }

    /// Counts indirect player passengers.
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
    fn has_exactly_one_player_passenger(&self) -> bool {
        self.count_player_passengers() == 1
    }

    /// Gets this entity's first direct passenger.
    fn first_passenger(&self) -> Option<SharedEntity> {
        self.base().first_passenger()
    }

    /// Default for [`Entity::controlling_passenger`].
    fn controlling_passenger_default(&self) -> Option<SharedEntity> {
        None
    }

    /// Returns whether this entity can control a vehicle it is riding.
    fn can_control_vehicle(&self) -> bool {
        !REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::NON_CONTROLLING_RIDER)
    }

    /// Returns whether this entity currently has a controlling passenger.
    fn has_controlling_passenger(&self) -> bool {
        self.controlling_passenger().is_some()
    }

    /// Returns whether this entity has any direct passengers.
    fn is_vehicle(&self) -> bool {
        self.base().is_vehicle()
    }

    /// Returns whether this entity's passengers are forced to dismount when
    /// it goes underwater, per the `dismounts_underwater` entity type tag.
    fn dismounts_underwater(&self) -> bool {
        REGISTRY
            .entity_types
            .is_in_tag(self.entity_type(), &EntityTypeTag::DISMOUNTS_UNDERWATER)
    }

    /// Returns whether `passenger` is a direct passenger of this entity.
    fn has_passenger(&self, passenger: &dyn Entity) -> bool {
        self.base().has_passenger_id(passenger.id())
    }

    /// Default for [`Entity::can_add_passenger`].
    fn can_add_passenger_default(&self, _passenger: &dyn Entity) -> bool {
        self.passengers().is_empty()
    }

    /// Returns whether this entity can accept any passenger.
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
    fn passenger_riding_position(&self, passenger: &dyn Entity) -> DVec3 {
        self.position() + self.passenger_attachment_point(passenger)
    }

    /// Default for [`Entity::position_rider`].
    fn position_rider_default(&self, passenger: &dyn Entity) {
        position_rider_default(self, passenger);
    }

    /// Returns this entity's root vehicle ID, or this entity's ID when it is not riding.
    ///
    /// Mirrors vanilla `Entity.getRootVehicle` using session IDs for object identity.
    fn root_vehicle_id(&self) -> i32 {
        self.root_vehicle().map_or(self.id(), |entity| entity.id())
    }

    /// Returns this entity's root vehicle, if this entity is riding one.
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
    fn is_passenger_of_same_vehicle(&self, other: &dyn Entity) -> bool {
        self.root_vehicle_id() == other.root_vehicle_id()
    }

    /// Returns whether `entity` is an indirect passenger of this entity.
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

    /// Default for [`Entity::ride_tick`].
    fn ride_tick_default(self: Arc<Self>) {
        self.set_velocity(DVec3::ZERO);
        Arc::clone(&self).tick();
        if let Some(vehicle) = self.vehicle() {
            vehicle.position_rider(self.as_entity_event_source());
        }
    }
}

impl<T: Entity + ?Sized> EntityRiding for T {}
