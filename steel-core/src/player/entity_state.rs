//! Core entity state flags for a player.
//!
//! Groups the boolean/simple state flags that describe what the player is
//! physically doing: sleeping, swimming, gliding, sneaking, sprinting.

use steel_registry::entity_data::EntityPose;
use steel_registry::entity_type::EntityDimensions;
use steel_registry::fluid::FluidStateExt as _;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{
    blocks::block_state_ext::BlockStateExt as _, blocks::properties::BlockStateProperties,
};
use steel_registry::{vanilla_attributes, vanilla_blocks};
use steel_utils::types::GameType;
use steel_utils::{BlockStateId, Identifier, WorldAabb};

use crate::entity::attribute::{AttributeModifier, AttributeModifierOperation};
use crate::entity::{Entity, EntitySharedFlags, LivingEntity};
use crate::fluid::get_fluid_state;
use crate::physics::{CollisionWorld, WorldCollisionProvider};
use crate::player::Player;

const SPRINT_SPEED_MODIFIER_AMOUNT: f64 = 0.3;
const POSE_COLLISION_EPSILON: f64 = 1.0E-7;

const PLAYER_STANDING_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.6, 1.8, 1.62);
const PLAYER_CROUCHING_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.6, 1.5, 1.27);
const PLAYER_SWIMMING_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.6, 0.6, 0.4);
const PLAYER_SLEEPING_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.2, 0.2, 0.2);
const PLAYER_DYING_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.2, 0.2, 1.62);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SwimmingEnvironment {
    sprinting: bool,
    passenger: bool,
    in_water: bool,
    under_water: bool,
    block_fluid_is_water: bool,
}

#[must_use]
const fn select_swimming_state(currently_swimming: bool, env: SwimmingEnvironment) -> bool {
    if env.passenger {
        return false;
    }

    if currently_swimming {
        env.sprinting && env.in_water
    } else {
        env.sprinting && env.under_water && env.block_fluid_is_water
    }
}

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

/// Physical state flags for a player entity.
pub(super) struct EntityState {
    /// Whether the player is currently sleeping in a bed.
    sleeping: bool,
    /// Whether the vanilla swimming shared flag is set.
    swimming: bool,
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
    pub swimming: bool,
    pub fall_flying: bool,
    pub crouching: bool,
    pub sprinting: bool,
}

impl EntityState {
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            sleeping: false,
            swimming: false,
            fall_flying: false,
            crouching: false,
            sprinting: false,
        }
    }

    #[must_use]
    pub(super) const fn snapshot(&self) -> EntityStateSnapshot {
        EntityStateSnapshot {
            sleeping: self.sleeping,
            swimming: self.swimming,
            fall_flying: self.fall_flying,
            crouching: self.crouching,
            sprinting: self.sprinting,
        }
    }

    pub(super) const fn set_sleeping(&mut self, sleeping: bool) {
        self.sleeping = sleeping;
    }

    pub(super) const fn set_swimming(&mut self, swimming: bool) {
        self.swimming = swimming;
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
        self.swimming = false;
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

    #[must_use]
    fn bounding_box_for_pose(&self, pose: EntityPose) -> WorldAabb {
        let position = self.base.position();
        let dimensions = Self::dimensions_for_pose(pose);
        WorldAabb::entity_box(
            position.x,
            position.y,
            position.z,
            f64::from(dimensions.half_width()),
            f64::from(dimensions.height),
        )
    }

    #[must_use]
    fn can_player_fit_within_blocks_when(&self, pose: EntityPose) -> bool {
        let world = self.get_world();
        let collision_world = WorldCollisionProvider::new(&world);
        collision_world
            .get_block_collisions(
                &self
                    .bounding_box_for_pose(pose)
                    .deflate(POSE_COLLISION_EPSILON),
            )
            .is_empty()
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

        // TODO: on_fire, invisible, glowing
        flags.set(EntitySharedFlags::SHIFT_KEY_DOWN, state.crouching);
        flags.set(EntitySharedFlags::SWIMMING, state.swimming);
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

    /// Returns true if vanilla player rules consider the player swimming.
    #[must_use]
    pub fn is_swimming(&self) -> bool {
        let state = self.entity_state_snapshot();
        state.swimming && !self.is_flying() && self.game_mode() != GameType::Spectator
    }

    fn set_swimming(&self, swimming: bool) {
        self.entity_state.lock().set_swimming(swimming);
    }

    /// Updates the vanilla swimming shared flag.
    pub(super) fn update_swimming(&self) {
        let state = self.entity_state_snapshot();
        let world = self.get_world();
        let block_fluid = get_fluid_state(&world, self.block_position());
        let swimming = select_swimming_state(
            state.swimming && !self.is_flying() && self.game_mode() != GameType::Spectator,
            SwimmingEnvironment {
                sprinting: state.sprinting,
                passenger: self.is_passenger(),
                in_water: self.is_in_water(),
                under_water: self.is_under_water(),
                block_fluid_is_water: block_fluid.is_water(),
            },
        );
        self.set_swimming(swimming);
    }

    /// Returns true if the player is currently fall flying (elytra).
    #[must_use]
    pub fn is_fall_flying(&self) -> bool {
        self.entity_state.lock().snapshot().fall_flying
    }

    /// Returns true if vanilla rules consider this player to be on a climbable block.
    #[must_use]
    pub(super) fn on_climbable(&self) -> bool {
        if self.is_flying() || self.is_spectator() {
            return false;
        }

        let pos = self.block_position();
        let world = self.get_world();
        let state = world.get_block_state(pos);
        let block = state.get_block();

        if self.is_fall_flying() && block.has_tag(&BlockTag::CAN_GLIDE_THROUGH) {
            return false;
        }

        if block.has_tag(&BlockTag::CLIMBABLE) {
            return true;
        }

        block.has_tag(&BlockTag::TRAPDOORS)
            && Self::trapdoor_usable_as_ladder_state(state, world.get_block_state(pos.below()))
    }

    fn trapdoor_usable_as_ladder_state(
        trapdoor_state: BlockStateId,
        below_state: BlockStateId,
    ) -> bool {
        if trapdoor_state.try_get_value(&BlockStateProperties::OPEN) != Some(true) {
            return false;
        }

        below_state.get_block() == &vanilla_blocks::LADDER
            && below_state.try_get_value(&BlockStateProperties::FACING)
                == trapdoor_state.try_get_value(&BlockStateProperties::FACING)
    }

    /// Sets the player's fall flying state.
    pub fn set_fall_flying(&self, fall_flying: bool) {
        self.entity_state.lock().set_fall_flying(fall_flying);
    }

    /// Determines the desired pose based on current player state.
    /// Priority: `Sleeping` > `Swimming` > `FallFlying` > `Sneaking` > `Standing`
    // TODO: Add SpinAttack pose (requires riptide trident)
    pub(super) fn get_desired_pose(&self) -> EntityPose {
        let es = self.entity_state_snapshot();
        if es.sleeping {
            EntityPose::Sleeping
        } else if es.swimming && !self.is_flying() && self.game_mode() != GameType::Spectator {
            EntityPose::Swimming
        } else if es.fall_flying {
            EntityPose::FallFlying
        } else if es.crouching && !self.is_flying() {
            EntityPose::Sneaking
        } else {
            EntityPose::Standing
        }
    }

    /// Updates the player's pose in entity data based on current state.
    pub(super) fn update_pose(&self) {
        if !self.can_player_fit_within_blocks_when(EntityPose::Swimming) {
            return;
        }

        let desired_pose = self.get_desired_pose();
        let is_spectator = self.game_mode() == GameType::Spectator;
        let fits_desired_pose =
            is_spectator || self.can_player_fit_within_blocks_when(desired_pose);
        let fits_crouching =
            !fits_desired_pose && self.can_player_fit_within_blocks_when(EntityPose::Sneaking);

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

        // TODO: Include blocking entities once entity collision pose checks exist.
        self.base
            .set_pose_and_dimensions(actual_pose, Self::dimensions_for_pose(actual_pose));
        self.entity_data.lock().base_mut().pose.set(actual_pose);
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
    use steel_registry::blocks::properties::Direction;
    use steel_registry::test_support::init_test_registry;

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

    #[test]
    fn open_trapdoor_matches_ladder_facing_for_climbable() {
        init_test_registry();

        let trapdoor = vanilla_blocks::OAK_TRAPDOOR
            .default_state()
            .set_value(&BlockStateProperties::OPEN, true)
            .set_value(&BlockStateProperties::FACING, Direction::North);
        let ladder = vanilla_blocks::LADDER
            .default_state()
            .set_value(&BlockStateProperties::FACING, Direction::North);

        assert!(Player::trapdoor_usable_as_ladder_state(trapdoor, ladder));
    }

    #[test]
    fn closed_trapdoor_is_not_usable_as_ladder() {
        init_test_registry();

        let trapdoor = vanilla_blocks::OAK_TRAPDOOR
            .default_state()
            .set_value(&BlockStateProperties::OPEN, false)
            .set_value(&BlockStateProperties::FACING, Direction::North);
        let ladder = vanilla_blocks::LADDER
            .default_state()
            .set_value(&BlockStateProperties::FACING, Direction::North);

        assert!(!Player::trapdoor_usable_as_ladder_state(trapdoor, ladder));
    }

    #[test]
    fn trapdoor_ladder_facing_must_match() {
        init_test_registry();

        let trapdoor = vanilla_blocks::OAK_TRAPDOOR
            .default_state()
            .set_value(&BlockStateProperties::OPEN, true)
            .set_value(&BlockStateProperties::FACING, Direction::North);
        let ladder = vanilla_blocks::LADDER
            .default_state()
            .set_value(&BlockStateProperties::FACING, Direction::South);

        assert!(!Player::trapdoor_usable_as_ladder_state(trapdoor, ladder));
    }
}
