use glam::DVec3;
use steel_macros::{ReadFrom, ServerPacket};

fn unpack_on_ground(packed_byte: u8) -> bool {
    packed_byte & 0b0000_0001 != 0
}

fn unpack_horizontal_collision(packed_byte: u8) -> bool {
    packed_byte & 0b0000_0010 != 0
}

/// Constructed packet by the server to more easily be able to handle movement.
#[derive(Clone, Debug)]
pub struct SMovePlayer {
    pub position: DVec3,
    pub y_rot: f32,
    pub x_rot: f32,
    pub on_ground: bool,
    pub horizontal_collision: bool,
    pub has_pos: bool,
    pub has_rot: bool,
}

impl SMovePlayer {
    #[must_use]
    pub fn get_x(&self, fallback: f64) -> f64 {
        if self.has_pos {
            self.position.x
        } else {
            fallback
        }
    }

    #[must_use]
    pub fn get_y(&self, fallback: f64) -> f64 {
        if self.has_pos {
            self.position.y
        } else {
            fallback
        }
    }

    #[must_use]
    pub fn get_z(&self, fallback: f64) -> f64 {
        if self.has_pos {
            self.position.z
        } else {
            fallback
        }
    }

    #[must_use]
    pub fn get_x_rot(&self, fallback: f32) -> f32 {
        if self.has_rot { self.x_rot } else { fallback }
    }

    #[must_use]
    pub fn get_y_rot(&self, fallback: f32) -> f32 {
        if self.has_rot { self.y_rot } else { fallback }
    }
}

#[derive(ReadFrom, Clone, Debug, ServerPacket)]
pub struct SMovePlayerPos {
    pub position: DVec3,
    pub packed_byte: u8,
}

impl From<SMovePlayerPos> for SMovePlayer {
    fn from(value: SMovePlayerPos) -> Self {
        Self {
            position: value.position,
            has_pos: true,
            has_rot: false,
            x_rot: 0.0,
            y_rot: 0.0,
            on_ground: unpack_on_ground(value.packed_byte),
            horizontal_collision: unpack_horizontal_collision(value.packed_byte),
        }
    }
}

#[derive(ReadFrom, Clone, Debug, ServerPacket)]
pub struct SMovePlayerPosRot {
    pub position: DVec3,
    pub y_rot: f32,
    pub x_rot: f32,
    pub packed_byte: u8,
}

impl From<SMovePlayerPosRot> for SMovePlayer {
    fn from(value: SMovePlayerPosRot) -> Self {
        Self {
            position: value.position,
            has_pos: true,
            has_rot: true,
            x_rot: value.x_rot,
            y_rot: value.y_rot,
            on_ground: unpack_on_ground(value.packed_byte),
            horizontal_collision: unpack_horizontal_collision(value.packed_byte),
        }
    }
}

#[derive(ReadFrom, Clone, Debug, ServerPacket)]
pub struct SMovePlayerRot {
    pub y_rot: f32,
    pub x_rot: f32,
    pub packed_byte: u8,
}

impl From<SMovePlayerRot> for SMovePlayer {
    fn from(value: SMovePlayerRot) -> Self {
        Self {
            position: DVec3::ZERO,
            has_pos: false,
            has_rot: true,
            x_rot: value.x_rot,
            y_rot: value.y_rot,
            on_ground: unpack_on_ground(value.packed_byte),
            horizontal_collision: unpack_horizontal_collision(value.packed_byte),
        }
    }
}

/// Status-only movement packet (no position or rotation, just on_ground flag).
///
/// Sent by the client when they haven't moved but want to update their ground status.
#[derive(ReadFrom, Clone, Debug, ServerPacket)]
pub struct SMovePlayerStatusOnly {
    pub packed_byte: u8,
}

impl From<SMovePlayerStatusOnly> for SMovePlayer {
    fn from(value: SMovePlayerStatusOnly) -> Self {
        Self {
            position: DVec3::ZERO,
            has_pos: false,
            has_rot: false,
            x_rot: 0.0,
            y_rot: 0.0,
            on_ground: unpack_on_ground(value.packed_byte),
            horizontal_collision: unpack_horizontal_collision(value.packed_byte),
        }
    }
}
