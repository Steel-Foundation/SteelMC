//! Entity physics state representation.

use glam::DVec3;
#[cfg(test)]
use steel_registry::entity_type::EntityDimensions;
use steel_utils::WorldAabb;

use crate::behavior::BlockCollisionContext;

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

    /// Whether vanilla lets this entity walk on powder snow.
    can_walk_on_powder_snow: bool,

    /// Whether vanilla collision context should treat this entity as a falling block.
    is_falling_block: bool,
}

impl EntityPhysicsState {
    /// Creates a new physics state from the current entity bounding box.
    ///
    /// Vanilla `Entity.move` resolves movement from the entity's actual
    /// `boundingBox`, not a box reconstructed from dimensions.
    #[must_use]
    pub const fn new(position: DVec3, bounding_box: WorldAabb, max_up_step: f32) -> Self {
        Self {
            position,
            bounding_box,
            max_up_step,
            backs_off_from_edge: false,
            on_ground: false,
            fall_distance: 0.0,
            descending: false,
            can_walk_on_powder_snow: false,
            is_falling_block: false,
        }
    }

    /// Creates a new physics state with custom dimensions.
    #[cfg(test)]
    #[must_use]
    pub fn with_dimensions(
        position: DVec3,
        dimensions: EntityDimensions,
        max_up_step: f32,
    ) -> Self {
        let bounding_box = Self::make_bounding_box(position, &dimensions);

        Self::new(position, bounding_box, max_up_step)
    }

    /// Creates a bounding box from position and dimensions.
    /// Box is centered on X/Z with Y at entity feet (vanilla behavior).
    #[cfg(test)]
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

    /// Returns the vanilla block collision context for this movement snapshot.
    #[must_use]
    pub const fn block_collision_context(self) -> BlockCollisionContext {
        BlockCollisionContext::entity(self.position.y, self.descending)
            .with_fall_distance(self.fall_distance)
            .with_can_walk_on_powder_snow(self.can_walk_on_powder_snow)
            .with_falling_block(self.is_falling_block)
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

    /// Returns a copy with powder-snow walkability set.
    #[must_use]
    pub const fn with_can_walk_on_powder_snow(mut self, can_walk_on_powder_snow: bool) -> Self {
        self.can_walk_on_powder_snow = can_walk_on_powder_snow;
        self
    }

    /// Returns a copy with falling-block collision context set.
    #[must_use]
    pub const fn with_falling_block(mut self, is_falling_block: bool) -> Self {
        self.is_falling_block = is_falling_block;
        self
    }
}
