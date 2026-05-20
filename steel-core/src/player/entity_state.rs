//! Core entity state flags for a player.
//!
//! Groups the boolean/simple state flags that describe what the player is
//! physically doing: sleeping, gliding, on the ground, sneaking, sprinting.

use bitflags::bitflags;
use steel_registry::entity_data::EntityPose;
use steel_registry::vanilla_attributes;
use steel_utils::Identifier;

use crate::entity::attribute::{AttributeModifier, AttributeModifierOperation};
use crate::player::Player;

const SPRINT_SPEED_MODIFIER_AMOUNT: f64 = 0.3;

bitflags! {
    /// Vanilla shared‐flags byte sent in entity metadata.
    struct SharedFlags: u8 {
        const ON_FIRE       = 1 << 0;
        const SHIFT_KEY_DOWN = 1 << 1;
        const SPRINTING     = 1 << 3;
        const SWIMMING      = 1 << 4;
        const INVISIBLE     = 1 << 5;
        const GLOWING       = 1 << 6;
        const FALL_FLYING   = 1 << 7;
    }
}

/// Physical state flags for a player entity.
pub struct EntityState {
    /// Whether the player is currently sleeping in a bed.
    pub sleeping: bool,
    /// Whether the player is currently fall flying (elytra gliding).
    pub fall_flying: bool,
    /// Whether the player is on the ground.
    pub on_ground: bool,
    /// Whether the player is sneaking (shift key down).
    pub crouching: bool,
    /// Whether the player is sprinting.
    pub sprinting: bool,
}

impl EntityState {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sleeping: false,
            fall_flying: false,
            on_ground: false,
            crouching: false,
            sprinting: false,
        }
    }
}

impl Player {
    /// Returns true if the player is shifting (sneaking).
    pub fn is_crouching(&self) -> bool {
        self.entity_state.lock().crouching
    }

    /// Packs `EntityState` booleans into the vanilla shared flags byte and writes
    /// it into `entity_data.shared_flags`. Dirty-tracking in [`SyncedValue`]
    /// ensures a `SetEntityData` packet is only sent when the value changes.
    pub(super) fn update_shared_flags(&self) {
        let state = self.entity_state.lock();
        let mut flags = SharedFlags::empty();

        // TODO: on_fire, swimming, invisible, glowing
        flags.set(SharedFlags::SHIFT_KEY_DOWN, state.crouching);
        flags.set(SharedFlags::SPRINTING, state.sprinting);
        flags.set(SharedFlags::FALL_FLYING, state.fall_flying);
        drop(state);

        self.entity_data.lock().shared_flags.set(flags.bits() as i8);
    }

    /// Returns true if the player is currently sleeping.
    #[must_use]
    pub fn is_sleeping(&self) -> bool {
        self.entity_state.lock().sleeping
    }

    /// Sets the player's sleeping state.
    pub fn set_sleeping(&self, sleeping: bool) {
        self.entity_state.lock().sleeping = sleeping;
    }

    /// Returns true if the player is currently fall flying (elytra).
    #[must_use]
    pub fn is_fall_flying(&self) -> bool {
        self.entity_state.lock().fall_flying
    }

    /// Sets the player's fall flying state.
    pub fn set_fall_flying(&self, fall_flying: bool) {
        self.entity_state.lock().fall_flying = fall_flying;
    }

    /// Returns true if the player is on the ground.
    #[must_use]
    pub fn is_on_ground(&self) -> bool {
        self.entity_state.lock().on_ground
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
        self.entity_data.lock().pose.set(desired_pose);
    }

    /// Adds or removes the sprint speed modifier on `MOVEMENT_SPEED`.
    ///
    /// Vanilla: `LivingEntity.setSprinting()` — `SPEED_MODIFIER_SPRINTING`.
    pub(super) fn apply_sprint_speed_modifier(&self, sprinting: bool) {
        let mut attrs = self.attributes.lock();
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
