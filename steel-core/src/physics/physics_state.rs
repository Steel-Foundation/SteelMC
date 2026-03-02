//! Entity physics state representation.

use steel_registry::blocks::shapes::AABBd;
use steel_registry::entity_types::{EntityDimensions, EntityTypeRef};
use steel_utils::math::Vector3;

/// Physics state for an entity, tracking position, velocity, and movement properties.
///
/// This struct contains all the information needed to simulate physics for an entity,
/// matching vanilla's Entity class fields related to movement.
#[derive(Debug, Clone)]
pub struct EntityPhysicsState {
    /// Current position (center of bounding box at feet level).
    pub position: Vector3<f64>,

    /// Current velocity (delta movement per tick).
    pub velocity: Vector3<f64>,

    /// Entity's axis-aligned bounding box in world coordinates.
    pub bounding_box: AABBd,

    /// Current entity dimensions (can change with pose/age).
    pub dimensions: EntityDimensions,

    /// Maximum height the entity can step up automatically.
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

/// Default max step height for most entities.
const DEFAULT_MAX_UP_STEP: f32 = 0.6;

impl EntityPhysicsState {
    /// Creates a new physics state for an entity at the given position.
    #[must_use]
    pub fn new(position: Vector3<f64>, entity_type: EntityTypeRef) -> Self {
        Self::with_dimensions(position, entity_type.dimensions, DEFAULT_MAX_UP_STEP)
    }

    /// Creates a new physics state with custom dimensions.
    #[must_use]
    pub fn with_dimensions(
        position: Vector3<f64>,
        dimensions: EntityDimensions,
        max_up_step: f32,
    ) -> Self {
        let bounding_box = Self::make_bounding_box(position, &dimensions);

        Self {
            position,
            velocity: Vector3::new(0.0, 0.0, 0.0),
            bounding_box,
            dimensions,
            max_up_step,
            is_crouching: false,
            on_ground: false,
            horizontal_collision: false,
            vertical_collision: false,
            in_water: false,
            in_lava: false,
            fall_distance: 0.0,
        }
    }

    /// Creates a bounding box from position and dimensions.
    /// Box is centered on X/Z with Y at entity feet (vanilla behavior).
    #[must_use]
    fn make_bounding_box(position: Vector3<f64>, dimensions: &EntityDimensions) -> AABBd {
        let half_width = f64::from(dimensions.width) / 2.0;
        let height = f64::from(dimensions.height);

        AABBd {
            min_x: position.x - half_width,
            min_y: position.y,
            min_z: position.z - half_width,
            max_x: position.x + half_width,
            max_y: position.y + height,
            max_z: position.z + half_width,
        }
    }

    /// Updates the bounding box to match the current position and dimensions.
    pub fn update_bounding_box(&mut self) {
        self.bounding_box = Self::make_bounding_box(self.position, &self.dimensions);
    }

    /// Sets the position and updates the bounding box accordingly.
    pub fn set_position(&mut self, position: Vector3<f64>) {
        self.position = position;
        self.update_bounding_box();
    }

    /// Sets new dimensions and updates the bounding box.
    /// Used when entity changes pose (crouching, swimming) or age (baby).
    pub fn set_dimensions(&mut self, dimensions: EntityDimensions) {
        self.dimensions = dimensions;
        self.update_bounding_box();
    }

    /// Returns the current eye height.
    #[must_use]
    pub const fn eye_height(&self) -> f32 {
        self.dimensions.eye_height
    }

    /// Returns the eye position in world coordinates.
    #[must_use]
    pub fn eye_position(&self) -> Vector3<f64> {
        Vector3::new(
            self.position.x,
            self.position.y + f64::from(self.dimensions.eye_height),
            self.position.z,
        )
    }
}
