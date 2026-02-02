//! Context types and results for block and item interactions.

use steel_registry::blocks::properties::Direction;
use steel_registry::item_stack::ItemStack;
use steel_utils::BlockPos;
use steel_utils::math::Vector3;
use steel_utils::types::InteractionHand;

// Re-export BlockHitResult from steel-registry since it's also used by steel-protocol
pub use steel_registry::items::item::BlockHitResult;

use crate::player::Player;
use crate::world::World;

/// Result of an interaction (item use, block use, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionResult {
    /// The interaction succeeded and consumed the action.
    Success,
    /// The interaction failed and consumed the action.
    Fail,
    /// The interaction did not apply; try the next handler.
    Pass,
    /// Try the empty-hand interaction on the block.
    TryEmptyHandInteraction,
}

impl InteractionResult {
    /// Returns true if this result consumes the action (Success or Fail).
    /// Pass and `TryEmptyHandInteraction` do not consume the action.
    #[must_use]
    pub fn consumes_action(&self) -> bool {
        matches!(self, InteractionResult::Success | InteractionResult::Fail)
    }
}

/// Context for placing a block.
pub struct BlockPlaceContext<'a> {
    /// The position that was clicked.
    pub clicked_pos: BlockPos,
    /// The face of the block that was clicked.
    pub clicked_face: Direction,
    /// The exact location where the click occurred.
    pub click_location: Vector3<f64>,
    /// Whether the click was inside the block.
    pub inside: bool,
    /// The position where the block will be placed.
    pub relative_pos: BlockPos,
    /// Whether the clicked block is being replaced.
    pub replace_clicked: bool,
    /// The horizontal direction the player is facing.
    pub horizontal_direction: Direction,
    /// The player's rotation (yaw).
    pub rotation: f32,
    /// The player's pitch (vertical look angle).
    pub pitch: f32,
    /// The world where the block is being placed.
    pub world: &'a World,
}

impl BlockPlaceContext<'_> {
    /// Returns the direction the player is looking at most directly.
    ///
    /// This considers both yaw and pitch to determine the nearest direction
    /// among all 6 directions (UP, DOWN, NORTH, SOUTH, EAST, WEST).
    ///
    /// Based on Java's `Direction.orderedByNearest(Entity)[0]`.
    #[must_use]
    pub fn get_nearest_looking_direction(&self) -> Direction {
        self.get_nearest_looking_directions()[0]
    }

    /// Returns all 6 directions ordered by how closely the player is looking at them.
    ///
    /// Based on Java's `BlockPlaceContext.getNearestLookingDirections()`.
    /// When not replacing the clicked block, the opposite of the clicked face
    /// is moved to the front of the array.
    #[must_use]
    pub fn get_nearest_looking_directions(&self) -> [Direction; 6] {
        let mut directions = Direction::ordered_by_nearest(self.rotation, self.pitch);

        // If not replacing the clicked block, prioritize the opposite of clicked face
        if !self.replace_clicked {
            let clicked_opposite = self.clicked_face.opposite();
            let mut index = 0;

            // Find the index of the opposite direction
            while index < directions.len() && directions[index] != clicked_opposite {
                index += 1;
            }

            // Move it to the front by shifting elements
            if index > 0 && index < directions.len() {
                // Shift elements [0..index] to [1..index+1] and put opposite at [0]
                directions.copy_within(0..index, 1);
                directions[0] = clicked_opposite;
            }
        }

        directions
    }
}

/// Context for using an item on a block.
pub struct UseOnContext<'a> {
    /// The player using the item.
    pub player: &'a Player,
    /// Which hand the item is in.
    pub hand: InteractionHand,
    /// Information about where the block was hit.
    pub hit_result: BlockHitResult,
    /// The world where the interaction is happening.
    pub world: &'a World,
    /// The item stack being used (mutable for consumption).
    pub item_stack: &'a mut ItemStack,
}
