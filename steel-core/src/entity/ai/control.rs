//! Mob control state.

use glam::DVec3;

pub(crate) const DEFAULT_LOOK_Y_MAX_ROT_SPEED: f32 = 10.0;
pub(crate) const DEFAULT_LOOK_X_MAX_ROT_ANGLE: f32 = 40.0;
const HEAD_STABLE_ANGLE: f32 = 15.0;
const DELAY_UNTIL_STARTING_TO_FACE_FORWARD: i32 = 10;
const HOW_LONG_IT_TAKES_TO_FACE_FORWARD: f32 = 10.0;

fn wrap_degrees(mut degrees: f32) -> f32 {
    degrees %= 360.0;
    if degrees >= 180.0 {
        degrees -= 360.0;
    }
    if degrees < -180.0 {
        degrees += 360.0;
    }
    degrees
}

pub(crate) fn rotate_towards(from_angle: f32, to_angle: f32, max_rot: f32) -> f32 {
    let diff = wrap_degrees(to_angle - from_angle);
    let diff_clamped = diff.clamp(-max_rot, max_rot);
    from_angle + diff_clamped
}

pub(crate) fn rotate_if_necessary(base_angle: f32, target_angle: f32, max_angle_diff: f32) -> f32 {
    let delta_angle = wrap_degrees(target_angle - base_angle);
    let delta_angle_clamped = delta_angle.clamp(-max_angle_diff, max_angle_diff);
    target_angle - delta_angle_clamped
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MoveControlOperation {
    Wait,
    MoveTo,
    Strafe,
    Jumping,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveControl {
    wanted_position: DVec3,
    speed_modifier: f64,
    strafe_forward: f32,
    strafe_right: f32,
    operation: MoveControlOperation,
}

impl MoveControl {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            wanted_position: DVec3::ZERO,
            speed_modifier: 0.0,
            strafe_forward: 0.0,
            strafe_right: 0.0,
            operation: MoveControlOperation::Wait,
        }
    }

    #[must_use]
    pub const fn operation(&self) -> MoveControlOperation {
        self.operation
    }

    #[must_use]
    pub const fn wanted_position(&self) -> DVec3 {
        self.wanted_position
    }

    #[must_use]
    pub const fn speed_modifier(&self) -> f64 {
        self.speed_modifier
    }

    #[must_use]
    pub const fn strafe_forward(&self) -> f32 {
        self.strafe_forward
    }

    #[must_use]
    pub const fn strafe_right(&self) -> f32 {
        self.strafe_right
    }

    pub fn set_wanted_position(&mut self, position: DVec3, speed_modifier: f64) {
        self.wanted_position = position;
        self.speed_modifier = speed_modifier;
        if self.operation != MoveControlOperation::Jumping {
            self.operation = MoveControlOperation::MoveTo;
        }
    }

    pub const fn strafe(&mut self, forward: f32, right: f32) {
        self.operation = MoveControlOperation::Strafe;
        self.strafe_forward = forward;
        self.strafe_right = right;
        self.speed_modifier = 0.25;
    }

    pub const fn set_wait(&mut self) {
        self.operation = MoveControlOperation::Wait;
    }

    pub const fn set_jumping(&mut self) {
        self.operation = MoveControlOperation::Jumping;
    }
}

impl Default for MoveControl {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JumpControl {
    jump: bool,
}

impl JumpControl {
    #[must_use]
    pub const fn new() -> Self {
        Self { jump: false }
    }

    pub const fn jump(&mut self) {
        self.jump = true;
    }

    pub const fn tick(&mut self) -> bool {
        let jump = self.jump;
        self.jump = false;
        jump
    }
}

impl Default for JumpControl {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LookControl {
    wanted_position: DVec3,
    y_max_rot_speed: f32,
    x_max_rot_angle: f32,
    look_at_cooldown: i32,
}

impl LookControl {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            wanted_position: DVec3::ZERO,
            y_max_rot_speed: DEFAULT_LOOK_Y_MAX_ROT_SPEED,
            x_max_rot_angle: DEFAULT_LOOK_X_MAX_ROT_ANGLE,
            look_at_cooldown: 0,
        }
    }

    #[must_use]
    pub const fn wanted_position(&self) -> DVec3 {
        self.wanted_position
    }

    #[must_use]
    pub const fn y_max_rot_speed(&self) -> f32 {
        self.y_max_rot_speed
    }

    #[must_use]
    pub const fn x_max_rot_angle(&self) -> f32 {
        self.x_max_rot_angle
    }

    #[must_use]
    pub const fn is_looking_at_target(&self) -> bool {
        self.look_at_cooldown > 0
    }

    pub const fn set_look_at(
        &mut self,
        position: DVec3,
        y_max_rot_speed: f32,
        x_max_rot_angle: f32,
    ) {
        self.wanted_position = position;
        self.y_max_rot_speed = y_max_rot_speed;
        self.x_max_rot_angle = x_max_rot_angle;
        self.look_at_cooldown = 2;
    }

    pub const fn tick_cooldown(&mut self) -> bool {
        if self.look_at_cooldown <= 0 {
            return false;
        }

        self.look_at_cooldown -= 1;
        true
    }
}

impl Default for LookControl {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyRotationInput {
    moving: bool,
    carrying_mob_passenger: bool,
    y_rot: f32,
    y_body_rot: f32,
    y_head_rot: f32,
    max_head_y_rot: f32,
}

impl BodyRotationInput {
    #[must_use]
    pub const fn new(
        moving: bool,
        carrying_mob_passenger: bool,
        y_rot: f32,
        y_body_rot: f32,
        y_head_rot: f32,
        max_head_y_rot: f32,
    ) -> Self {
        Self {
            moving,
            carrying_mob_passenger,
            y_rot,
            y_body_rot,
            y_head_rot,
            max_head_y_rot,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyRotationUpdate {
    y_body_rot: f32,
    y_head_rot: f32,
}

impl BodyRotationUpdate {
    #[must_use]
    pub const fn y_body_rot(self) -> f32 {
        self.y_body_rot
    }

    #[must_use]
    pub const fn y_head_rot(self) -> f32 {
        self.y_head_rot
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyRotationControl {
    head_stable_time: i32,
    last_stable_y_head_rot: f32,
}

impl BodyRotationControl {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            head_stable_time: 0,
            last_stable_y_head_rot: 0.0,
        }
    }

    #[must_use]
    pub const fn head_stable_time(self) -> i32 {
        self.head_stable_time
    }

    #[must_use]
    pub const fn last_stable_y_head_rot(self) -> f32 {
        self.last_stable_y_head_rot
    }

    pub fn tick(&mut self, input: BodyRotationInput) -> BodyRotationUpdate {
        let mut y_body_rot = input.y_body_rot;
        let mut y_head_rot = input.y_head_rot;

        if input.moving {
            y_body_rot = input.y_rot;
            y_head_rot = rotate_if_necessary(y_head_rot, y_body_rot, input.max_head_y_rot);
            self.last_stable_y_head_rot = y_head_rot;
            self.head_stable_time = 0;
        } else if !input.carrying_mob_passenger {
            if (y_head_rot - self.last_stable_y_head_rot).abs() > HEAD_STABLE_ANGLE {
                self.head_stable_time = 0;
                self.last_stable_y_head_rot = y_head_rot;
                y_body_rot = rotate_if_necessary(y_body_rot, y_head_rot, input.max_head_y_rot);
            } else {
                self.head_stable_time += 1;
                if self.head_stable_time > DELAY_UNTIL_STARTING_TO_FACE_FORWARD {
                    let time_since_starting_to_face_forward =
                        self.head_stable_time - DELAY_UNTIL_STARTING_TO_FACE_FORWARD;
                    let face_forward_fraction = (time_since_starting_to_face_forward as f32
                        / HOW_LONG_IT_TAKES_TO_FACE_FORWARD)
                        .clamp(0.0, 1.0);
                    let angle_remaining_until_facing_forward =
                        input.max_head_y_rot * (1.0 - face_forward_fraction);
                    y_body_rot = rotate_if_necessary(
                        y_body_rot,
                        y_head_rot,
                        angle_remaining_until_facing_forward,
                    );
                }
            }
        }

        BodyRotationUpdate {
            y_body_rot,
            y_head_rot,
        }
    }
}

impl Default for BodyRotationControl {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct MobControls {
    pub move_control: MoveControl,
    pub jump_control: JumpControl,
    pub look_control: LookControl,
    pub body_rotation_control: BodyRotationControl,
}

impl MobControls {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            move_control: MoveControl::new(),
            jump_control: JumpControl::new(),
            look_control: LookControl::new(),
            body_rotation_control: BodyRotationControl::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BodyRotationControl, BodyRotationInput};

    fn assert_f32_close(left: f32, right: f32) {
        assert!(
            (left - right).abs() < 1.0e-6,
            "expected {left:?} to equal {right:?}"
        );
    }

    #[test]
    fn body_rotation_control_faces_body_forward_while_moving() {
        let mut control = BodyRotationControl::new();

        let update = control.tick(BodyRotationInput::new(true, false, 90.0, 0.0, 200.0, 75.0));

        assert_f32_close(update.y_body_rot(), 90.0);
        assert_f32_close(update.y_head_rot(), 165.0);
        assert_eq!(control.head_stable_time(), 0);
        assert_f32_close(control.last_stable_y_head_rot(), 165.0);
    }

    #[test]
    fn body_rotation_control_turns_body_when_idle_head_moves() {
        let mut control = BodyRotationControl::new();

        let update = control.tick(BodyRotationInput::new(false, false, 0.0, 0.0, 90.0, 75.0));

        assert_f32_close(update.y_body_rot(), 15.0);
        assert_f32_close(update.y_head_rot(), 90.0);
        assert_eq!(control.head_stable_time(), 0);
        assert_f32_close(control.last_stable_y_head_rot(), 90.0);
    }

    #[test]
    fn body_rotation_control_waits_then_turns_body_toward_stable_head() {
        let mut control = BodyRotationControl::new();
        let first = control.tick(BodyRotationInput::new(false, false, 0.0, 0.0, 90.0, 75.0));
        let mut y_body_rot = first.y_body_rot();

        for _ in 0..10 {
            y_body_rot = control
                .tick(BodyRotationInput::new(
                    false, false, 0.0, y_body_rot, 90.0, 75.0,
                ))
                .y_body_rot();
        }

        assert_eq!(control.head_stable_time(), 10);
        assert_f32_close(y_body_rot, 15.0);

        let update = control.tick(BodyRotationInput::new(
            false, false, 0.0, y_body_rot, 90.0, 75.0,
        ));

        assert_eq!(control.head_stable_time(), 11);
        assert_f32_close(update.y_body_rot(), 22.5);
    }

    #[test]
    fn body_rotation_control_does_not_turn_idle_body_when_carrying_mob_passenger() {
        let mut control = BodyRotationControl::new();

        let update = control.tick(BodyRotationInput::new(false, true, 0.0, 0.0, 90.0, 75.0));

        assert_f32_close(update.y_body_rot(), 0.0);
        assert_f32_close(update.y_head_rot(), 90.0);
        assert_eq!(control.head_stable_time(), 0);
        assert_f32_close(control.last_stable_y_head_rot(), 0.0);
    }
}
