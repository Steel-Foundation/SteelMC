//! Shared vanilla movement-validation helpers for client-authored movement.

use glam::DVec3;

/// Movement error threshold for anti-cheat validation (squared distance).
/// Vanilla uses 0.0625 (1/16 block squared).
pub const MOVEMENT_ERROR_THRESHOLD: f64 = 0.0625;

/// Y-axis tolerance value used by vanilla's movement-error branch.
///
/// Vanilla currently uses `yDist > -0.5 || yDist < 0.5`, which zeroes every
/// finite Y residual before the moved-wrongly check.
pub const Y_TOLERANCE: f64 = 0.5;

/// Collision state used to decide whether a client-authored movement is accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovementCollisionValidation {
    /// Whether the entity bypasses collision physics.
    pub no_physics: bool,
    /// Whether the simulated server position differs too much from the client target.
    pub moved_wrongly: bool,
    /// Whether the entity already intersected collision before the movement.
    pub old_collision: bool,
    /// Whether accepting the target would newly intersect collision.
    pub new_collision: bool,
}

impl MovementCollisionValidation {
    /// Returns true when vanilla rejects this movement and rolls back the entity.
    #[must_use]
    pub const fn rejects(self) -> bool {
        !self.no_physics && ((self.moved_wrongly && !self.old_collision) || self.new_collision)
    }
}

/// Returns the residual between a client target and the server-simulated position.
#[must_use]
pub fn movement_error_delta(target_pos: DVec3, simulated_pos: DVec3) -> DVec3 {
    let error_x = target_pos.x - simulated_pos.x;
    let mut error_y = target_pos.y - simulated_pos.y;
    if error_y > -Y_TOLERANCE || error_y < Y_TOLERANCE {
        error_y = 0.0;
    }
    let error_z = target_pos.z - simulated_pos.z;
    DVec3::new(error_x, error_y, error_z)
}

/// Returns vanilla's post-move Y residual for floating validation.
#[must_use]
pub fn vanilla_post_move_y_dist(target_y: f64, simulated_y: f64) -> f64 {
    let mut y_dist = target_y - simulated_y;
    if y_dist > -Y_TOLERANCE || y_dist < Y_TOLERANCE {
        y_dist = 0.0;
    }
    y_dist
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_error_delta_matches_vanilla_y_branch() {
        let delta = movement_error_delta(DVec3::new(10.0, 120.0, -5.0), DVec3::new(8.0, 0.0, -8.0));

        assert_eq!(delta, DVec3::new(2.0, 0.0, 3.0));
    }

    #[test]
    fn movement_validation_accepts_no_physics_even_with_new_collision() {
        assert!(
            !MovementCollisionValidation {
                no_physics: true,
                moved_wrongly: true,
                old_collision: false,
                new_collision: true,
            }
            .rejects()
        );
    }

    #[test]
    fn movement_validation_rejects_new_collision_for_physical_entity() {
        assert!(
            MovementCollisionValidation {
                no_physics: false,
                moved_wrongly: false,
                old_collision: false,
                new_collision: true,
            }
            .rejects()
        );
    }

    #[test]
    #[expect(clippy::float_cmp, reason = "vanilla branch deliberately zeros Y")]
    fn post_move_y_dist_matches_vanilla_y_branch() {
        assert_eq!(vanilla_post_move_y_dist(64.0, 63.0), 0.0);
        assert_eq!(vanilla_post_move_y_dist(64.0, 65.0), 0.0);
    }
}
