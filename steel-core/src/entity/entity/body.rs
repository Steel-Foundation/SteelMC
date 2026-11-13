//! Pose, dimensions, eyes and look direction.

use super::*;

/// Pose, dimensions, eyes and look direction.
///
/// Implemented for every [`Entity`]; entity types change behavior through
/// [`EntityProps`], never by implementing this trait.
pub trait EntityBody: Entity {
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

    /// Gets the nearest horizontal direction to the entity's yaw (horizontal rotation).
    fn direction_yaw(&self) -> Direction {
        let (yaw, _) = self.rotation();
        Direction::from_yaw(yaw)
    }

    /// Default for [`EntityHooks::motion_direction`].
    fn motion_direction_default(&self) -> Direction {
        self.direction_yaw()
    }

    /// Default for [`EntityHooks::look_at`].
    fn look_at_default(&self, from_anchor: EntityAnchor, target: DVec3) {
        apply_entity_look_at(self.as_entity_event_source(), from_anchor, target);
    }

    /// Default for [`EntityHooks::look_at_entity`].
    fn look_at_entity_default(
        &self,
        from_anchor: EntityAnchor,
        target: &dyn Entity,
        target_anchor: EntityAnchor,
    ) {
        self.look_at(from_anchor, target_anchor.position(target));
    }

    /// Gets the eye height for this entity.
    ///
    /// Default implementation returns the eye height from the entity type dimensions.
    /// Override for entities with pose-dependent eye heights (e.g., players).
    fn eye_height(&self) -> f64 {
        f64::from(self.base().dimensions().eye_height)
    }

    /// `Entity.getRandomX`: X up to `spread` bounding-box widths either side of the center.
    fn random_x(&self, spread: f64) -> f64 {
        let width = f64::from(self.base().dimensions().width);
        self.position().x + width * (2.0 * rng().random::<f64>() - 1.0) * spread
    }

    /// `Entity.getRandomY()`: Y anywhere within the bounding box height.
    fn random_y(&self) -> f64 {
        let height = f64::from(self.base().dimensions().height);
        self.position().y + height * rng().random::<f64>()
    }

    /// `Entity.getRandomY(spread)`: Y up to `spread` bounding-box heights either side of the feet.
    fn random_y_spread(&self, spread: f64) -> f64 {
        let height = f64::from(self.base().dimensions().height);
        self.position().y + height * (2.0 * rng().random::<f64>() - 1.0) * spread
    }

    /// `Entity.getRandomZ`: Z up to `spread` bounding-box widths either side of the center.
    fn random_z(&self, spread: f64) -> f64 {
        let width = f64::from(self.base().dimensions().width);
        self.position().z + width * (2.0 * rng().random::<f64>() - 1.0) * spread
    }

    /// Gets the Y coordinate of the entity's eyes.
    fn eye_y(&self) -> f64 {
        self.position().y + self.eye_height()
    }

    /// Default for [`EntityHooks::is_in_wall`].
    fn is_in_wall_default(&self) -> bool {
        if self.no_physics() {
            return false;
        }

        let Some(world) = self.level() else {
            return false;
        };

        let position = self.position();
        let check_width = f64::from(self.base().dimensions().width * 0.8);
        let eye_box = entity_eye_suffocation_box(
            DVec3::new(position.x, self.eye_y(), position.z),
            check_width,
        );

        !block_effects::for_each_block_in_aabb(eye_box, |pos| {
            let state = world.get_block_state(pos);
            !block_state_suffocates_eye_box(state, world.as_ref(), pos, eye_box)
        })
    }

    /// Computes a unit look-direction vector from the given pitch and yaw in degrees.
    fn calculate_view_vector(&self, pitch_degrees: f32, yaw_degrees: f32) -> DVec3 {
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

    /// Returns this entity's physical pose.
    fn pose(&self) -> EntityPose {
        self.base().pose()
    }

    /// Default for [`EntityHooks::dimensions_for_pose`].
    fn dimensions_for_pose_default(&self, _pose: EntityPose) -> EntityDimensions {
        self.entity_type().dimensions
    }

    /// Default for [`EntityHooks::refresh_dimensions`].
    fn refresh_dimensions_default(&self) {
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
    fn fudge_position_after_size_change(&self, previous_dimensions: EntityDimensions) -> bool {
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
    fn set_pose(&self, pose: EntityPose) {
        self.base()
            .set_pose_and_dimensions(pose, self.dimensions_for_pose(pose));
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_pose(pose);
        }
    }

    /// Default for [`EntityHooks::is_crouching`].
    fn is_crouching_default(&self) -> bool {
        self.pose() == EntityPose::Sneaking
    }
}

impl<T: Entity + ?Sized> EntityBody for T {}
