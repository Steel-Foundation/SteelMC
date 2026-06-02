//! Core entity state flags for a player.
//!
//! Groups the boolean/simple state flags that describe what the player is
//! physically doing: sleeping, gliding, sneaking, sprinting.

use steel_registry::entity_data::EntityPose;
use steel_registry::entity_type::EntityDimensions;
use steel_registry::vanilla_attributes;
use steel_utils::Identifier;

use crate::entity::attribute::{AttributeModifier, AttributeModifierOperation};
use crate::entity::{EntitySharedFlags, LivingEntity};
use crate::player::Player;

const SPRINT_SPEED_MODIFIER_AMOUNT: f64 = 0.3;

const PLAYER_STANDING_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.6, 1.8, 1.62);
const PLAYER_CROUCHING_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.6, 1.5, 1.27);
const PLAYER_SWIMMING_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.6, 0.6, 0.4);
const PLAYER_SLEEPING_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.2, 0.2, 0.2);
const PLAYER_DYING_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.2, 0.2, 1.62);

/// Physical state flags for a player entity.
pub(super) struct EntityState {
    /// Whether the player is currently sleeping in a bed.
    sleeping: bool,
    /// Whether the player is currently fall flying (elytra gliding).
    fall_flying: bool,
    /// Whether the player is sneaking (shift key down).
    crouching: bool,
    /// Whether the player is sprinting.
    sprinting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EntityStateSnapshot {
    pub sleeping: bool,
    pub fall_flying: bool,
    pub crouching: bool,
    pub sprinting: bool,
}

impl EntityState {
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            sleeping: false,
            fall_flying: false,
            crouching: false,
            sprinting: false,
        }
    }

    #[must_use]
    pub(super) const fn snapshot(&self) -> EntityStateSnapshot {
        EntityStateSnapshot {
            sleeping: self.sleeping,
            fall_flying: self.fall_flying,
            crouching: self.crouching,
            sprinting: self.sprinting,
        }
    }

    pub(super) const fn set_sleeping(&mut self, sleeping: bool) {
        self.sleeping = sleeping;
    }

    pub(super) const fn set_fall_flying(&mut self, fall_flying: bool) {
        self.fall_flying = fall_flying;
    }

    pub(super) const fn set_crouching(&mut self, crouching: bool) {
        self.crouching = crouching;
    }

    pub(super) const fn set_sprinting(&mut self, sprinting: bool) {
        self.sprinting = sprinting;
    }

    pub(super) const fn reset_transient(&mut self) {
        self.fall_flying = false;
        self.sleeping = false;
        self.crouching = false;
        self.sprinting = false;
    }
}

impl Player {
    /// Returns vanilla `Avatar.POSES` dimensions for a player pose.
    pub(super) const fn dimensions_for_pose(pose: EntityPose) -> EntityDimensions {
        match pose {
            EntityPose::Sleeping => PLAYER_SLEEPING_DIMENSIONS,
            EntityPose::FallFlying | EntityPose::Swimming | EntityPose::SpinAttack => {
                PLAYER_SWIMMING_DIMENSIONS
            }
            EntityPose::Sneaking => PLAYER_CROUCHING_DIMENSIONS,
            EntityPose::Dying => PLAYER_DYING_DIMENSIONS,
            _ => PLAYER_STANDING_DIMENSIONS,
        }
    }

    pub(super) fn entity_state_snapshot(&self) -> EntityStateSnapshot {
        self.entity_state.lock().snapshot()
    }

    pub(super) fn reset_entity_state(&self) {
        self.entity_state.lock().reset_transient();
    }

    /// Returns true if the player is shifting (sneaking).
    pub fn is_crouching(&self) -> bool {
        self.entity_state.lock().snapshot().crouching
    }

    /// Sets whether the player is shifting (sneaking).
    pub fn set_crouching(&self, crouching: bool) {
        self.entity_state.lock().set_crouching(crouching);
    }

    /// Packs `EntityState` booleans into the vanilla shared flags byte and writes
    /// it into `entity_data.shared_flags`. Dirty-tracking in [`SyncedValue`]
    /// ensures a `SetEntityData` packet is only sent when the value changes.
    pub(super) fn update_shared_flags(&self) {
        let state = self.entity_state.lock();
        let mut flags = EntitySharedFlags::empty();

        // TODO: on_fire, swimming, invisible, glowing
        flags.set(EntitySharedFlags::SHIFT_KEY_DOWN, state.crouching);
        flags.set(EntitySharedFlags::SPRINTING, state.sprinting);
        flags.set(EntitySharedFlags::FALL_FLYING, state.fall_flying);
        drop(state);

        self.entity_data
            .lock()
            .base_mut()
            .shared_flags
            .set(flags.metadata_byte());
    }

    /// Returns true if the player is currently sleeping.
    #[must_use]
    pub fn is_sleeping(&self) -> bool {
        self.entity_state.lock().snapshot().sleeping
    }

    /// Sets the player's sleeping state.
    pub fn set_sleeping(&self, sleeping: bool) {
        self.entity_state.lock().set_sleeping(sleeping);
    }

    /// Returns true if the player is currently fall flying (elytra).
    #[must_use]
    pub fn is_fall_flying(&self) -> bool {
        self.entity_state.lock().snapshot().fall_flying
    }

    /// Sets the player's fall flying state.
    pub fn set_fall_flying(&self, fall_flying: bool) {
        self.entity_state.lock().set_fall_flying(fall_flying);
    }

    /// Determines the desired pose based on current player state.
    /// Priority: `Sleeping` > `FallFlying` > `Sneaking` > `Standing`
    // TODO: Add Swimming pose (requires water detection)
    // TODO: Add SpinAttack pose (requires riptide trident)
    // TODO: Add pose collision checks (force crouch in low ceilings)
    pub(super) fn get_desired_pose(&self) -> EntityPose {
        let es = self.entity_state.lock();
        if es.sleeping {
            EntityPose::Sleeping
        } else if es.fall_flying {
            EntityPose::FallFlying
        } else if es.crouching && !self.abilities.lock().flying {
            EntityPose::Sneaking
        } else {
            EntityPose::Standing
        }
    }

    /// Updates the player's pose in entity data based on current state.
    pub(super) fn update_pose(&self) {
        let desired_pose = self.get_desired_pose();
        self.base
            .set_pose_and_dimensions(desired_pose, Self::dimensions_for_pose(desired_pose));
        self.entity_data.lock().base_mut().pose.set(desired_pose);
    }

    /// Adds or removes the sprint speed modifier on `MOVEMENT_SPEED`.
    ///
    /// Vanilla: `LivingEntity.setSprinting()` — `SPEED_MODIFIER_SPRINTING`.
    pub(super) fn apply_sprint_speed_modifier(&self, sprinting: bool) {
        let mut attrs = self.attributes().lock();
        if sprinting {
            attrs.add_modifier(
                vanilla_attributes::MOVEMENT_SPEED,
                AttributeModifier {
                    id: Identifier::vanilla_static("sprinting"),
                    amount: SPRINT_SPEED_MODIFIER_AMOUNT,
                    operation: AttributeModifierOperation::AddMultipliedTotal,
                },
                false,
            );
        } else {
            attrs.remove_modifier(
                vanilla_attributes::MOVEMENT_SPEED,
                &Identifier::vanilla_static("sprinting"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_pose_dimensions_match_vanilla_avatar() {
        assert_eq!(
            Player::dimensions_for_pose(EntityPose::Standing),
            EntityDimensions::new(0.6, 1.8, 1.62)
        );
        assert_eq!(
            Player::dimensions_for_pose(EntityPose::Sneaking),
            EntityDimensions::new(0.6, 1.5, 1.27)
        );
        assert_eq!(
            Player::dimensions_for_pose(EntityPose::FallFlying),
            EntityDimensions::new(0.6, 0.6, 0.4)
        );
        assert_eq!(
            Player::dimensions_for_pose(EntityPose::Swimming),
            EntityDimensions::new(0.6, 0.6, 0.4)
        );
        assert_eq!(
            Player::dimensions_for_pose(EntityPose::SpinAttack),
            EntityDimensions::new(0.6, 0.6, 0.4)
        );
        assert_eq!(
            Player::dimensions_for_pose(EntityPose::Sleeping),
            EntityDimensions::new(0.2, 0.2, 0.2)
        );
        assert_eq!(
            Player::dimensions_for_pose(EntityPose::Dying),
            EntityDimensions::new(0.2, 0.2, 1.62)
        );
    }
}
