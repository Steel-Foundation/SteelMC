use std::sync::Weak;

use crate::entity::{RemovalReason, SharedEntity, WeakEntity};

/// Non-physical lifecycle state shared by every entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EntityLifecycleState {
    pub(super) removal_reason: Option<RemovalReason>,
    pub(super) pending_world_change: Option<PendingWorldChangeToken>,
    pub(super) next_world_change_token: u64,
}

impl EntityLifecycleState {
    pub(super) const fn new() -> Self {
        Self {
            removal_reason: None,
            pending_world_change: None,
            next_world_change_token: 1,
        }
    }

    pub(super) fn next_world_change_token(&mut self) -> PendingWorldChangeToken {
        let token = PendingWorldChangeToken(self.next_world_change_token);
        self.next_world_change_token = self.next_world_change_token.wrapping_add(1).max(1);
        token
    }
}

/// Runtime token for an in-flight world change request.
///
/// The token is intentionally not persisted. It protects async preparation jobs
/// from completing or clearing a newer transition started by the same entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PendingWorldChangeToken(u64);

/// Vanilla passenger and vehicle relationship state.
///
/// Stored separately from movement state because riding relationships affect
/// collision, tracking, saving, and future pathfinding without being part of
/// the entity's physical pose/velocity snapshot.
#[derive(Default)]
pub(super) struct EntityRelationshipState {
    pub(super) vehicle: Option<WeakEntity>,
    pub(super) passengers: Vec<WeakEntity>,
    pub(super) boarding_cooldown: i32,
}

impl EntityRelationshipState {
    pub(super) fn vehicle(&mut self) -> Option<SharedEntity> {
        let vehicle = self.vehicle.as_ref().and_then(Weak::upgrade);
        if vehicle.is_none() {
            self.vehicle = None;
        }
        vehicle
    }

    pub(super) fn passengers(&mut self) -> Vec<SharedEntity> {
        let mut live_passengers = Vec::new();
        self.passengers.retain(|passenger| {
            if let Some(entity) = passenger.upgrade() {
                live_passengers.push(entity);
                true
            } else {
                false
            }
        });
        live_passengers
    }

    pub(super) fn first_passenger(&mut self) -> Option<SharedEntity> {
        self.passengers
            .retain(|passenger| passenger.strong_count() > 0);
        self.passengers.first().and_then(Weak::upgrade)
    }

    pub(super) fn has_passenger_id(&mut self, passenger_id: i32) -> bool {
        self.passengers
            .retain(|passenger| passenger.strong_count() > 0);
        self.passengers.iter().any(|passenger| {
            passenger
                .upgrade()
                .is_some_and(|entity| entity.id() == passenger_id)
        })
    }

    pub(super) fn remove_passenger_id(&mut self, passenger_id: i32) -> bool {
        let mut removed = false;
        self.passengers.retain(|passenger| {
            let Some(entity) = passenger.upgrade() else {
                return false;
            };
            if entity.id() == passenger_id {
                removed = true;
                false
            } else {
                true
            }
        });
        removed
    }
}
