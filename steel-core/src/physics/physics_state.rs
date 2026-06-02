//! Entity physics state representation.

use glam::DVec3;
use steel_registry::entity_type::EntityDimensions;
use steel_utils::WorldAabb;

/// Immutable entity movement input used by the collision resolver.
///
/// Steel keeps authoritative physical state on `EntityBase`; this type is a
/// narrow snapshot of the fields vanilla `Entity.move` needs while resolving a
/// single movement.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EntityPhysicsState {
    /// Current position (center of bounding box at feet level).
    position: DVec3,

    /// Entity's axis-aligned bounding box in world coordinates.
    bounding_box: WorldAabb,

    /// Maximum height the entity can step up automatically.
    max_up_step: f32,

    /// Whether the entity backs away from ledges for this movement.
    backs_off_from_edge: bool,

    /// Whether the entity is on the ground (affects step-up and jump mechanics).
    on_ground: bool,

    /// Remaining fall distance for fall damage calculation.
    fall_distance: f64,

    /// Whether vanilla collision context should treat this entity as descending.
    descending: bool,
}

impl EntityPhysicsState {
    /// Creates a new physics state with custom dimensions.
    #[must_use]
    pub fn with_dimensions(
        position: DVec3,
        dimensions: EntityDimensions,
        max_up_step: f32,
    ) -> Self {
        let bounding_box = Self::make_bounding_box(position, &dimensions);

        Self {
            position,
            bounding_box,
            max_up_step,
            backs_off_from_edge: false,
            on_ground: false,
            fall_distance: 0.0,
            descending: false,
        }
    }

    /// Creates a bounding box from position and dimensions.
    /// Box is centered on X/Z with Y at entity feet (vanilla behavior).
    #[must_use]
    fn make_bounding_box(position: DVec3, dimensions: &EntityDimensions) -> WorldAabb {
        let half_width = f64::from(dimensions.width) / 2.0;
        let height = f64::from(dimensions.height);

        WorldAabb::entity_box(position.x, position.y, position.z, half_width, height)
    }

    /// Returns the current bottom-center position.
    #[must_use]
    pub const fn position(self) -> DVec3 {
        self.position
    }

    /// Returns the current world-space bounding box.
    #[must_use]
    pub const fn bounding_box(self) -> WorldAabb {
        self.bounding_box
    }

    /// Returns the maximum automatic step-up height.
    #[must_use]
    pub const fn max_up_step(self) -> f32 {
        self.max_up_step
    }

    /// Returns whether sneak-edge prevention should apply.
    #[must_use]
    pub const fn backs_off_from_edge(self) -> bool {
        self.backs_off_from_edge
    }

    /// Returns whether the entity was on ground before movement.
    #[must_use]
    pub const fn on_ground(self) -> bool {
        self.on_ground
    }

    /// Returns the accumulated fall distance before movement.
    #[must_use]
    pub const fn fall_distance(self) -> f64 {
        self.fall_distance
    }

    /// Returns whether collision context should treat the entity as descending.
    #[must_use]
    pub const fn descending(self) -> bool {
        self.descending
    }

    /// Returns a copy with the pre-movement ground flag set.
    #[must_use]
    pub const fn with_on_ground(mut self, on_ground: bool) -> Self {
        self.on_ground = on_ground;
        self
    }

    /// Returns a copy with sneak-edge prevention enabled or disabled.
    #[must_use]
    pub const fn with_backs_off_from_edge(mut self, backs_off_from_edge: bool) -> Self {
        self.backs_off_from_edge = backs_off_from_edge;
        self
    }

    /// Returns a copy with the accumulated fall distance set.
    #[must_use]
    pub const fn with_fall_distance(mut self, fall_distance: f64) -> Self {
        self.fall_distance = fall_distance;
        self
    }

    /// Returns a copy with the collision-context descending flag set.
    #[must_use]
    pub const fn with_descending(mut self, descending: bool) -> Self {
        self.descending = descending;
        self
    }
}
