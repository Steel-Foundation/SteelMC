use glam::DVec3;
use steel_utils::BlockPos;

/// A vanilla movement segment used by block-contact effects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityMovement {
    from: DVec3,
    to: DVec3,
    axis_dependent_original_movement: Option<DVec3>,
}

impl EntityMovement {
    /// Creates a movement segment without axis-dependent original movement.
    #[must_use]
    pub const fn new(from: DVec3, to: DVec3) -> Self {
        Self {
            from,
            to,
            axis_dependent_original_movement: None,
        }
    }

    /// Creates a movement segment with the original requested movement.
    #[must_use]
    pub const fn with_axis_dependent_original_movement(
        from: DVec3,
        to: DVec3,
        axis_dependent_original_movement: DVec3,
    ) -> Self {
        Self {
            from,
            to,
            axis_dependent_original_movement: Some(axis_dependent_original_movement),
        }
    }

    /// Returns the segment start position.
    #[must_use]
    pub const fn from(self) -> DVec3 {
        self.from
    }

    /// Returns the segment end position.
    #[must_use]
    pub const fn to(self) -> DVec3 {
        self.to
    }

    /// Returns the requested movement used for vanilla axis-ordered scans.
    #[must_use]
    pub const fn axis_dependent_original_movement(self) -> Option<DVec3> {
        self.axis_dependent_original_movement
    }
}

/// Vanilla server-driven gate for vertical collision and ground-contact updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityVerticalMovementStateUpdate {
    /// Preserve the existing vertical collision and ground-contact state.
    Preserve,
    /// Refresh vertical collision and ground-contact state from the movement result.
    Refresh,
}

impl EntityVerticalMovementStateUpdate {
    /// Returns the vanilla update behavior for a completed movement request.
    #[must_use]
    pub fn for_move(requested_delta: DVec3, server_driven_movement: bool) -> Self {
        if requested_delta.y.abs() > 0.0 || server_driven_movement {
            Self::Refresh
        } else {
            Self::Preserve
        }
    }

    /// Returns whether vertical collision and ground contact should be refreshed.
    #[inline]
    #[must_use]
    pub const fn refreshes_state(self) -> bool {
        matches!(self, Self::Refresh)
    }
}

/// Vanilla collision and ground-contact flags updated by `Entity.move`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityMovementFlags {
    on_ground: bool,
    horizontal_collision: bool,
    vertical_collision: bool,
    vertical_collision_below: bool,
}

impl EntityMovementFlags {
    /// Creates movement flags for an entity that has not moved yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            on_ground: false,
            horizontal_collision: false,
            vertical_collision: false,
            vertical_collision_below: false,
        }
    }

    /// Creates movement flags from a completed movement pass.
    #[must_use]
    pub fn after_move(
        on_ground: bool,
        horizontal_collision: bool,
        vertical_collision: bool,
        requested_delta: DVec3,
    ) -> Self {
        Self {
            on_ground,
            horizontal_collision,
            vertical_collision,
            vertical_collision_below: vertical_collision && requested_delta.y < 0.0,
        }
    }

    /// Creates movement flags from a completed movement pass while preserving
    /// vertical/ground state when vanilla skips that update.
    #[must_use]
    pub fn after_move_with_previous(
        previous: Self,
        vertical_state_update: EntityVerticalMovementStateUpdate,
        on_ground: bool,
        horizontal_collision: bool,
        vertical_collision: bool,
        requested_delta: DVec3,
    ) -> Self {
        let mut next = previous.with_horizontal_collision(horizontal_collision);
        if vertical_state_update.refreshes_state() {
            next.on_ground = on_ground;
            next.vertical_collision = vertical_collision;
            next.vertical_collision_below = vertical_collision && requested_delta.y < 0.0;
        }
        next
    }

    /// Returns true if the entity is touching the ground.
    #[inline]
    #[must_use]
    pub const fn on_ground(self) -> bool {
        self.on_ground
    }

    /// Returns true if the last movement was clipped horizontally.
    #[inline]
    #[must_use]
    pub const fn horizontal_collision(self) -> bool {
        self.horizontal_collision
    }

    /// Returns true if the last movement was clipped vertically.
    #[inline]
    #[must_use]
    pub const fn vertical_collision(self) -> bool {
        self.vertical_collision
    }

    /// Returns true if the last vertical collision was below the entity.
    #[inline]
    #[must_use]
    pub const fn vertical_collision_below(self) -> bool {
        self.vertical_collision_below
    }

    /// Returns the same flags with a new ground-contact value.
    #[must_use]
    pub const fn with_on_ground(mut self, on_ground: bool) -> Self {
        self.on_ground = on_ground;
        self
    }

    /// Returns the same flags with a new horizontal-collision value.
    #[must_use]
    pub const fn with_horizontal_collision(mut self, horizontal_collision: bool) -> Self {
        self.horizontal_collision = horizontal_collision;
        self
    }

    /// Returns the same ground state with collision flags cleared.
    #[must_use]
    pub const fn without_collisions(mut self) -> Self {
        self.horizontal_collision = false;
        self.vertical_collision = false;
        self.vertical_collision_below = false;
        self
    }
}

impl Default for EntityMovementFlags {
    fn default() -> Self {
        Self::new()
    }
}

/// Vanilla ground-support state updated by `Entity.checkSupportingBlock`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EntityGroundContact {
    supporting_block: Option<BlockPos>,
    on_ground_no_blocks: bool,
}

impl EntityGroundContact {
    /// Creates airborne ground-contact state.
    #[must_use]
    pub const fn airborne() -> Self {
        Self {
            supporting_block: None,
            on_ground_no_blocks: false,
        }
    }

    /// Creates grounded contact state from the support search result.
    #[must_use]
    pub const fn on_ground(supporting_block: Option<BlockPos>) -> Self {
        Self {
            supporting_block,
            on_ground_no_blocks: supporting_block.is_none(),
        }
    }

    /// Returns the supporting block selected by vanilla support rules.
    #[must_use]
    pub const fn supporting_block(self) -> Option<BlockPos> {
        self.supporting_block
    }

    /// Returns true when the entity is grounded but no block support was found.
    #[must_use]
    pub const fn on_ground_no_blocks(self) -> bool {
        self.on_ground_no_blocks
    }
}

/// Vanilla movement side effects emitted by `Entity.move`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityMovementEmission {
    /// Emit no movement sounds or game events.
    None,
    /// Emit movement sounds only.
    Sounds,
    /// Emit movement game events only.
    Events,
    /// Emit both movement sounds and game events.
    All,
}

impl EntityMovementEmission {
    /// Returns whether this movement emits any side effects.
    #[must_use]
    pub const fn emits_anything(self) -> bool {
        !matches!(self, Self::None)
    }

    /// Returns whether this movement emits game events.
    #[must_use]
    pub const fn emits_events(self) -> bool {
        matches!(self, Self::Events | Self::All)
    }

    /// Returns whether this movement emits sounds.
    #[must_use]
    pub const fn emits_sounds(self) -> bool {
        matches!(self, Self::Sounds | Self::All)
    }
}

/// Vanilla movement distance counters used by step, swim, and flap side effects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityMovementProgress {
    pub(super) move_dist: f32,
    pub(super) fly_dist: f32,
    pub(super) next_step: f32,
    pub(super) crystal_sound_intensity: f32,
    pub(super) last_crystal_sound_play_tick: i32,
}

impl EntityMovementProgress {
    /// Creates default vanilla movement progress state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            move_dist: 0.0,
            fly_dist: 0.0,
            next_step: 1.0,
            crystal_sound_intensity: 0.0,
            last_crystal_sound_play_tick: 0,
        }
    }

    /// Adds movement distance from a completed movement pass.
    pub fn add_movement(&mut self, clipped_movement: DVec3, climbing: bool) {
        let moved_distance = (clipped_movement.length() * 0.6) as f32;
        let horizontal_moved_distance = ((clipped_movement.x * clipped_movement.x
            + clipped_movement.z * clipped_movement.z)
            .sqrt()
            * 0.6) as f32;

        self.move_dist += if climbing {
            moved_distance
        } else {
            horizontal_moved_distance
        };
        self.fly_dist += moved_distance;
    }

    /// Returns vanilla `moveDist`.
    #[must_use]
    pub const fn move_dist(self) -> f32 {
        self.move_dist
    }

    /// Returns vanilla `flyDist`.
    #[must_use]
    pub const fn fly_dist(self) -> f32 {
        self.fly_dist
    }

    /// Returns vanilla `nextStep`.
    #[must_use]
    pub const fn next_step(self) -> f32 {
        self.next_step
    }

    /// Returns whether movement crossed the next step threshold.
    #[must_use]
    pub const fn crossed_next_step(self) -> bool {
        self.move_dist > self.next_step
    }
}

impl Default for EntityMovementProgress {
    fn default() -> Self {
        Self::new()
    }
}
