//! Entity physics state representation.

use steel_registry::blocks::shapes::AABBd;
use steel_utils::math::Vector3;

/// Physics state for an entity, tracking position, velocity, and movement properties.
///
/// This struct contains all the information needed to simulate physics for an entity,
/// matching vanilla's Entity class fields related to movement.
#[derive(Debug, Clone)]
pub struct EntityPhysicsState {
    /// Current position (center of bounding box at feet level for players).
    pub position: Vector3<f64>,

    /// Current velocity (delta movement per tick).
    pub velocity: Vector3<f64>,

    /// Entity's axis-aligned bounding box in world coordinates.
    pub bounding_box: AABBd,

    /// Maximum height the entity can step up automatically (0.6 for players).
    pub max_up_step: f32,

    /// Whether the entity is crouching (affects sneak-edge prevention).
    pub is_crouching: bool,

    /// Whether the entity is on the ground (affects step-up and jump mechanics).
    pub on_ground: bool,

    /// Whether horizontal movement was blocked by collision.
    pub horizontal_collision: bool,

    /// Whether vertical movement was blocked by collision.
    pub vertical_collision: bool,

    /// Whether the entity is in water.
    pub in_water: bool,

    /// Whether the entity is in lava.
    pub in_lava: bool,

    /// Remaining fall distance for fall damage calculation.
    pub fall_distance: f32,
}

impl EntityPhysicsState {
    /// Creates a new physics state for a player at the given position.
    ///
    /// Uses standard player dimensions (0.6 x 1.8) and max step height (0.6).
    #[must_use]
    pub fn new_player(position: Vector3<f64>) -> Self {
        const PLAYER_WIDTH: f64 = 0.6;
        const PLAYER_HEIGHT: f64 = 1.8;
        const PLAYER_MAX_UP_STEP: f32 = 0.6;

        let half_width = PLAYER_WIDTH / 2.0;
        let bounding_box = AABBd {
            min_x: position.x - half_width,
            min_y: position.y,
            min_z: position.z - half_width,
            max_x: position.x + half_width,
            max_y: position.y + PLAYER_HEIGHT,
            max_z: position.z + half_width,
        };

        Self {
            position,
            velocity: Vector3::new(0.0, 0.0, 0.0),
            bounding_box,
            max_up_step: PLAYER_MAX_UP_STEP,
            is_crouching: false,
            on_ground: false,
            horizontal_collision: false,
            vertical_collision: false,
            in_water: false,
            in_lava: false,
            fall_distance: 0.0,
        }
    }

    /// Updates the bounding box to match the current position.
    ///
    /// Maintains the same dimensions but centers on the new position.
    pub fn update_bounding_box(&mut self) {
        let width = self.bounding_box.max_x - self.bounding_box.min_x;
        let height = self.bounding_box.max_y - self.bounding_box.min_y;
        let half_width = width / 2.0;

        self.bounding_box.min_x = self.position.x - half_width;
        self.bounding_box.min_z = self.position.z - half_width;
        self.bounding_box.max_x = self.position.x + half_width;
        self.bounding_box.max_z = self.position.z + half_width;
        self.bounding_box.min_y = self.position.y;
        self.bounding_box.max_y = self.position.y + height;
    }

    /// Sets the position and updates the bounding box accordingly.
    pub fn set_position(&mut self, position: Vector3<f64>) {
        self.position = position;
        self.update_bounding_box();
    }
}
