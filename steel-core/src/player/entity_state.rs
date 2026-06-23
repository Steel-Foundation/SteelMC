//! Core entity state flags for a player.
//!
//! Groups player pose and shared-flag helpers.

use steel_registry::entity_data::EntityPose;
use steel_registry::entity_type::{EntityAttachmentPoint, EntityAttachments, EntityDimensions};
use steel_utils::WorldAabb;
use steel_utils::types::GameType;

use crate::behavior::BlockCollisionContext;
use crate::entity::{Entity, EntitySyncedData, LivingEntity};
use crate::physics::{CollisionWorld, WorldCollisionProvider};
use crate::player::Player;

const POSE_COLLISION_EPSILON: f64 = 1.0E-7;
const PLAYER_VEHICLE_ATTACHMENT: [EntityAttachmentPoint; 1] =
    [EntityAttachmentPoint::new(0.0, 0.6, 0.0)];
const NO_ATTACHMENT_POINTS: [EntityAttachmentPoint; 0] = [];

const fn player_dimensions_with_vehicle_attachment(
    width: f32,
    height: f32,
    eye_height: f32,
) -> EntityDimensions {
    EntityDimensions::new_with_attachments(
        width,
        height,
        eye_height,
        EntityAttachments::new(
            &NO_ATTACHMENT_POINTS,
            &PLAYER_VEHICLE_ATTACHMENT,
            &NO_ATTACHMENT_POINTS,
            &NO_ATTACHMENT_POINTS,
        ),
    )
}

const PLAYER_STANDING_DIMENSIONS: EntityDimensions =
    player_dimensions_with_vehicle_attachment(0.6, 1.8, 1.62);
const PLAYER_CROUCHING_DIMENSIONS: EntityDimensions =
    player_dimensions_with_vehicle_attachment(0.6, 1.5, 1.27);
const PLAYER_SWIMMING_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.6, 0.6, 0.4);
const PLAYER_SLEEPING_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.2, 0.2, 0.2);
const PLAYER_DYING_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.2, 0.2, 1.62);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PoseFit {
    spectator: bool,
    passenger: bool,
    desired_pose: bool,
    crouching: bool,
    swimming: bool,
}

#[must_use]
const fn select_actual_pose(desired_pose: EntityPose, fit: PoseFit) -> Option<EntityPose> {
    if !fit.swimming {
        return None;
    }

    if fit.spectator || fit.passenger || fit.desired_pose {
        Some(desired_pose)
    } else if fit.crouching {
        Some(EntityPose::Sneaking)
    } else {
        Some(EntityPose::Swimming)
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

    #[must_use]
    fn bounding_box_for_pose(&self, pose: EntityPose) -> WorldAabb {
        let position = self.base.position();
        let dimensions = <Self as Entity>::dimensions_for_pose(self, pose);
        WorldAabb::entity_box(
            position.x,
            position.y,
            position.z,
            f64::from(dimensions.half_width()),
            f64::from(dimensions.height),
        )
    }

    #[must_use]
    fn can_player_fit_within_blocks_and_entities_when(&self, pose: EntityPose) -> bool {
        let world = self.get_world();
        let collision_world = WorldCollisionProvider::for_entity(&world, self);
        !collision_world.has_collision_with_context(
            &self
                .bounding_box_for_pose(pose)
                .deflate(POSE_COLLISION_EPSILON),
            BlockCollisionContext::entity(self.position().y, self.is_descending())
                .with_can_walk_on_powder_snow(self.can_walk_on_powder_snow()),
        )
    }

    pub(super) fn reset_entity_state(&self) {
        self.set_shared_swimming(false);
        self.set_shared_shift_key_down(false);
        self.clear_sleeping_pos();
        self.set_fall_flying(false);
        self.set_sprinting(false);
    }

    /// Returns true if the player is shifting (sneaking).
    pub fn is_crouching(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_shift_key_down)
    }

    /// Sets whether the player is shifting (sneaking).
    pub fn set_crouching(&self, crouching: bool) {
        self.set_shared_shift_key_down(crouching);
    }

    /// Returns true if vanilla player rules consider the player swimming.
    #[must_use]
    pub fn is_swimming(&self) -> bool {
        self.synced_data()
            .is_some_and(EntitySyncedData::is_swimming)
            && !self.is_flying()
            && self.game_mode() != GameType::Spectator
    }

    /// Returns true if the player is currently fall flying (elytra).
    #[must_use]
    pub fn is_fall_flying(&self) -> bool {
        LivingEntity::is_fall_flying(self)
    }

    /// Returns true if vanilla rules consider this player to be on a climbable block.
    #[must_use]
    pub(super) fn on_climbable(&self) -> bool {
        if self.is_flying() {
            return false;
        }

        self.default_living_on_climbable()
    }

    /// Sets the player's fall flying state.
    pub fn set_fall_flying(&self, fall_flying: bool) {
        LivingEntity::set_fall_flying(self, fall_flying);
    }

    /// Determines the desired pose based on current player state.
    /// Priority: `Sleeping` > `Swimming` > `FallFlying` > `Sneaking` > `Standing`
    // TODO: Add SpinAttack pose (requires riptide trident)
    pub(super) fn get_desired_pose(&self) -> EntityPose {
        if self.is_sleeping() {
            EntityPose::Sleeping
        } else if self.is_swimming() {
            EntityPose::Swimming
        } else if self.is_fall_flying() {
            EntityPose::FallFlying
        } else if self.is_crouching() && !self.is_flying() {
            EntityPose::Sneaking
        } else {
            EntityPose::Standing
        }
    }

    /// Updates the player's pose in entity data based on current state.
    pub(super) fn update_pose(&self) {
        if !self.can_player_fit_within_blocks_and_entities_when(EntityPose::Swimming) {
            return;
        }

        let desired_pose = self.get_desired_pose();
        let is_spectator = self.game_mode() == GameType::Spectator;
        let fits_desired_pose =
            is_spectator || self.can_player_fit_within_blocks_and_entities_when(desired_pose);
        let fits_crouching = !fits_desired_pose
            && self.can_player_fit_within_blocks_and_entities_when(EntityPose::Sneaking);

        let Some(actual_pose) = select_actual_pose(
            desired_pose,
            PoseFit {
                spectator: is_spectator,
                passenger: self.is_passenger(),
                desired_pose: fits_desired_pose,
                crouching: fits_crouching,
                swimming: true,
            },
        ) else {
            return;
        };

        self.base.set_pose_and_dimensions(
            actual_pose,
            <Self as Entity>::dimensions_for_pose(self, actual_pose),
        );
        self.entity_data.lock().base_mut().pose.set(actual_pose);
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::entity_type::EntityAttachment;

    use crate::entity::{SwimmingEnvironment, select_swimming_state};

    use super::*;

    fn assert_vec3_close(left: glam::DVec3, right: glam::DVec3) {
        let diff = left - right;
        assert!(
            diff.length_squared() < 1.0e-12,
            "expected {left:?} to equal {right:?}"
        );
    }

    #[test]
    fn player_pose_dimensions_match_vanilla_avatar() {
        assert_eq!(
            Player::dimensions_for_pose(EntityPose::Standing),
            PLAYER_STANDING_DIMENSIONS
        );
        assert_eq!(
            Player::dimensions_for_pose(EntityPose::Sneaking),
            PLAYER_CROUCHING_DIMENSIONS
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

    #[test]
    fn player_pose_dimensions_preserve_vanilla_vehicle_attachment() {
        let standing = Player::dimensions_for_pose(EntityPose::Standing);
        let crouching = Player::dimensions_for_pose(EntityPose::Sneaking);
        let swimming = Player::dimensions_for_pose(EntityPose::Swimming);

        assert_vec3_close(
            standing
                .attachments
                .get_clamped(EntityAttachment::Vehicle, 0, 0.0, standing),
            glam::DVec3::new(0.0, 0.6, 0.0),
        );
        assert_vec3_close(
            crouching
                .attachments
                .get_clamped(EntityAttachment::Vehicle, 0, 0.0, crouching),
            glam::DVec3::new(0.0, 0.6, 0.0),
        );
        assert_vec3_close(
            swimming
                .attachments
                .get_clamped(EntityAttachment::Vehicle, 0, 0.0, swimming),
            glam::DVec3::ZERO,
        );
    }

    #[test]
    fn swimming_state_continues_while_sprinting_in_water() {
        assert!(select_swimming_state(
            true,
            SwimmingEnvironment {
                sprinting: true,
                passenger: false,
                in_water: true,
                under_water: false,
                block_fluid_is_water: false,
            },
        ));
    }

    #[test]
    fn swimming_state_stops_when_current_swimmer_stops_sprinting() {
        assert!(!select_swimming_state(
            true,
            SwimmingEnvironment {
                sprinting: false,
                passenger: false,
                in_water: true,
                under_water: true,
                block_fluid_is_water: true,
            },
        ));
    }

    #[test]
    fn swimming_state_starts_when_sprinting_underwater_in_water_block() {
        assert!(select_swimming_state(
            false,
            SwimmingEnvironment {
                sprinting: true,
                passenger: false,
                in_water: true,
                under_water: true,
                block_fluid_is_water: true,
            },
        ));
    }

    #[test]
    fn swimming_state_does_not_start_from_body_water_only() {
        assert!(!select_swimming_state(
            false,
            SwimmingEnvironment {
                sprinting: true,
                passenger: false,
                in_water: true,
                under_water: false,
                block_fluid_is_water: true,
            },
        ));
    }

    #[test]
    fn swimming_state_stops_while_passenger() {
        assert!(!select_swimming_state(
            true,
            SwimmingEnvironment {
                sprinting: true,
                passenger: true,
                in_water: true,
                under_water: true,
                block_fluid_is_water: true,
            },
        ));
    }

    #[test]
    fn player_pose_selection_keeps_pose_when_swimming_cannot_fit() {
        assert_eq!(
            select_actual_pose(
                EntityPose::Standing,
                PoseFit {
                    spectator: false,
                    passenger: false,
                    desired_pose: true,
                    crouching: true,
                    swimming: false,
                },
            ),
            None
        );
    }

    #[test]
    fn player_pose_selection_allows_spectator_desired_pose() {
        assert_eq!(
            select_actual_pose(
                EntityPose::Standing,
                PoseFit {
                    spectator: true,
                    passenger: false,
                    desired_pose: false,
                    crouching: false,
                    swimming: true,
                },
            ),
            Some(EntityPose::Standing)
        );
    }

    #[test]
    fn player_pose_selection_allows_passenger_desired_pose() {
        assert_eq!(
            select_actual_pose(
                EntityPose::Standing,
                PoseFit {
                    spectator: false,
                    passenger: true,
                    desired_pose: false,
                    crouching: false,
                    swimming: true,
                },
            ),
            Some(EntityPose::Standing)
        );
    }

    #[test]
    fn player_pose_selection_falls_back_to_crouching_when_desired_pose_is_blocked() {
        assert_eq!(
            select_actual_pose(
                EntityPose::Standing,
                PoseFit {
                    spectator: false,
                    passenger: false,
                    desired_pose: false,
                    crouching: true,
                    swimming: true,
                },
            ),
            Some(EntityPose::Sneaking)
        );
    }

    #[test]
    fn player_pose_selection_falls_back_to_swimming_when_crouching_is_blocked() {
        assert_eq!(
            select_actual_pose(
                EntityPose::Standing,
                PoseFit {
                    spectator: false,
                    passenger: false,
                    desired_pose: false,
                    crouching: false,
                    swimming: true,
                },
            ),
            Some(EntityPose::Swimming)
        );
    }
}
