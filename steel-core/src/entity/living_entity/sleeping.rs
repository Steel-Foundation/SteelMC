//! Sleeping in beds.

use super::*;

impl Living<'_> {
    /// Returns the bed position that makes this living entity sleeping.
    #[must_use]
    pub fn sleeping_pos(&self) -> Option<BlockPos> {
        self.living_base().sleeping_pos()
    }

    /// Sets the vanilla living-entity sleeping position.
    pub fn set_sleeping_pos(&self, bed_position: BlockPos) {
        self.living_base().set_sleeping_pos(bed_position);
        self.with_synced(|data| data.sleeping_pos.set(Some(bed_position)));
    }

    /// Clears the vanilla living-entity sleeping position.
    pub fn clear_sleeping_pos(&self) {
        self.living_base().clear_sleeping_pos();
        self.with_synced(|data| data.sleeping_pos.set(None));
    }

    /// Checks if the entity is sleeping.
    #[must_use]
    pub fn is_sleeping(&self) -> bool {
        self.sleeping_pos().is_some()
    }

    /// Starts sleeping at the given bed position.
    pub fn start_sleeping(&self, bed_position: BlockPos) -> Result<(), EntityMoveError> {
        let Some(world) = self.level() else {
            return Err(EntityMoveError::NotLive {
                entity_id: self.id(),
            });
        };
        let block_state = world.get_block_state(bed_position);

        let Some(sleep_height) = BLOCK_BEHAVIORS
            .get_behavior(block_state.get_block())
            .get_sleep_height(block_state, world.as_ref(), bed_position)
        else {
            return Err(EntityMoveError::InvalidSleepPosition {
                entity_id: self.id(),
                bed_position,
            });
        };

        if self.is_passenger() {
            self.stop_riding();
        }

        self.try_set_position(DVec3::new(
            f64::from(bed_position.x()) + 0.5,
            f64::from(bed_position.y()) + sleep_height + BED_SLEEP_POSITION_OFFSET,
            f64::from(bed_position.z()) + 0.5,
        ))?;

        if block_state.is_bed() {
            world.set_block(
                bed_position,
                block_state.set_value(&BlockStateProperties::OCCUPIED, true),
                UpdateFlags::UPDATE_ALL,
            );
        }

        self.set_pose(EntityPose::Sleeping);
        self.set_sleeping_pos(bed_position);
        self.set_velocity(DVec3::ZERO);
        Ok(())
    }
}

/// `LivingEntity`'s [`LivingProps::stop_sleeping`].
///
/// Stops the entity from sleeping.
pub fn stop_sleeping<E: Parts>(entity: &E) {
    let living = &entity.living();
    if let Some(bed_position) = living.sleeping_pos()
        && let Some(world) = living.level()
    {
        let state = world.get_block_state(bed_position);
        if state.is_bed() {
            let facing = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
            world.set_block(
                bed_position,
                state.set_value(&BlockStateProperties::OCCUPIED, false),
                UpdateFlags::UPDATE_ALL,
            );
            BLOCK_BEHAVIORS
                .get_behavior(state.get_block())
                .on_stop_sleeping(state, &world, bed_position);
            let stand_up =
                BedBlock::find_standup_position(&world, living.as_entity(), facing, bed_position)
                    .unwrap_or_else(|| {
                        let above = bed_position.above();
                        DVec3::new(
                            f64::from(above.x()) + 0.5,
                            f64::from(above.y()) + 0.1,
                            f64::from(above.z()) + 0.5,
                        )
                    });
            let bed_center = DVec3::new(
                f64::from(bed_position.x()) + 0.5,
                f64::from(bed_position.y()),
                f64::from(bed_position.z()) + 0.5,
            );
            let look_direction = (bed_center - stand_up).normalize_or_zero();
            let yaw = wrap_degrees(
                (look_direction.z.atan2(look_direction.x).to_degrees() - f64::from(DEGREE_90))
                    as f32,
            );
            if let Err(error) = living.try_set_position(stand_up) {
                log::warn!(
                    "failed to move entity {} to bed stand-up position: {error}",
                    living.id()
                );
            }
            living.set_rotation((yaw, 0.0));
        }
    }

    living.set_pose(EntityPose::Standing);
    living.clear_sleeping_pos();
}
