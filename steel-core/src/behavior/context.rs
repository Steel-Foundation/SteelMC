//! Context types and results for block and item interactions.

use glam::DVec3;
use std::sync::Arc;
use steel_registry::REGISTRY;
use steel_registry::blocks::properties::Direction;
use steel_registry::item_stack::ItemStack;
use steel_utils::BlockPos;
use steel_utils::types::InteractionHand;

use crate::fluid::FluidStateExt;
use crate::inventory::lock::{ContainerId, ContainerLockGuard};
use crate::player::Player;
use crate::player::player_inventory::PlayerInventory;
use crate::world::World;
pub use steel_registry::items::item::BlockHitResult;

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
    pub const fn consumes_action(&self) -> bool {
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
    pub click_location: DVec3,
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
    pub world: &'a Arc<World>,
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
            if let Some(index) = directions.iter().position(|&d| d == clicked_opposite)
                && index > 0
            {
                directions.copy_within(0..index, 1);
                directions[0] = clicked_opposite;
            }
        }

        directions
    }

    /// Returns true if the block at the relative position is a water source
    #[must_use]
    pub fn is_water_source(&self) -> bool {
        use crate::fluid::get_fluid_state;
        let fluid_state = get_fluid_state(self.world, self.relative_pos);
        fluid_state.is_source() && fluid_state.is_water()
    }
}

/// Context for using an item on a block.
///
/// Access the hand item via `item()` and the player inventory via `inventory()`.
/// The compiler prevents holding both simultaneously, avoiding aliased mutation.
pub struct UseOnContext<'a> {
    /// The player using the item.
    pub player: &'a Player,
    /// Which hand the item is in.
    pub hand: InteractionHand,
    /// Information about where the block was hit.
    pub hit_result: BlockHitResult,
    /// The world where the interaction is happening.
    pub world: &'a Arc<World>,
    inv_guard: &'a mut ContainerLockGuard,
    inv_id: ContainerId,
}

impl<'a> UseOnContext<'a> {
    /// Creates a new `UseOnContext`.
    #[must_use]
    pub const fn new(
        player: &'a Player,
        hand: InteractionHand,
        hit_result: BlockHitResult,
        world: &'a Arc<World>,
        inv_guard: &'a mut ContainerLockGuard,
        inv_id: ContainerId,
    ) -> Self {
        Self {
            player,
            hand,
            hit_result,
            world,
            inv_guard,
            inv_id,
        }
    }

    /// Returns a mutable reference to the item in the player's hand.
    ///
    /// Cannot be held simultaneously with `inventory()` or `guard()`.
    #[allow(clippy::missing_panics_doc)] // Panic is unreachable when context is correctly constructed
    pub fn item(&mut self) -> &mut ItemStack {
        self.inv_guard
            .get_player_inventory_mut(self.inv_id)
            .expect("player inventory must be locked")
            .get_item_in_hand_mut(self.hand)
    }

    /// Returns a mutable reference to the player's inventory.
    #[allow(clippy::missing_panics_doc)]
    pub fn inventory(&mut self) -> &mut PlayerInventory {
        self.inv_guard
            .get_player_inventory_mut(self.inv_id)
            .expect("player inventory must be locked")
    }

    /// Returns a mutable reference to the container lock guard.
    pub const fn guard(&mut self) -> &mut ContainerLockGuard {
        self.inv_guard
    }

    /// Builds a [`BlockPlaceContext`] from this interaction context.
    ///
    /// Returns `None` if placement is invalid (out of bounds or non-replaceable target).
    /// This is the common prefix of vanilla's `BlockItem.useOn`.
    #[must_use]
    pub fn build_place_context(&self) -> Option<BlockPlaceContext<'a>> {
        let clicked_pos = self.hit_result.block_pos;
        let clicked_state = self.world.get_block_state(clicked_pos);
        let clicked_block = REGISTRY.blocks.by_state_id(clicked_state);
        let clicked_replaceable = clicked_block.is_some_and(|b| b.config.replaceable);

        let (place_pos, replace_clicked) = if clicked_replaceable {
            (clicked_pos, true)
        } else {
            (self.hit_result.direction.relative(clicked_pos), false)
        };

        if !self.world.is_in_valid_bounds(place_pos) {
            return None;
        }

        let existing_state = self.world.get_block_state(place_pos);
        let existing_block = REGISTRY.blocks.by_state_id(existing_state);
        if !existing_block.is_some_and(|b| b.config.replaceable) {
            return None;
        }

        let (yaw, pitch) = self.player.rotation.load();

        Some(BlockPlaceContext {
            clicked_pos,
            clicked_face: self.hit_result.direction,
            click_location: self.hit_result.location,
            inside: self.hit_result.inside,
            relative_pos: place_pos,
            replace_clicked,
            horizontal_direction: Direction::from_yaw(yaw),
            rotation: yaw,
            pitch,
            world: self.world,
        })
    }
}

/// Context for using an item (general usage).
///
/// Same mediated-access pattern as `UseOnContext`. Call `item()` for the hand item,
/// `inventory()` for the player inventory, or `guard()` for the full lock guard.
pub struct UseItemContext<'a> {
    /// The player using the item.
    pub player: &'a Player,
    /// Which hand the item is in.
    pub hand: InteractionHand,
    /// The world where the interaction is happening.
    pub world: &'a Arc<World>,
    inv_guard: &'a mut ContainerLockGuard,
    inv_id: ContainerId,
}

impl<'a> UseItemContext<'a> {
    /// Creates a new `UseItemContext`.
    #[must_use]
    pub const fn new(
        player: &'a Player,
        hand: InteractionHand,
        world: &'a Arc<World>,
        inv_guard: &'a mut ContainerLockGuard,
        inv_id: ContainerId,
    ) -> Self {
        Self {
            player,
            hand,
            world,
            inv_guard,
            inv_id,
        }
    }

    /// Returns a mutable reference to the item in the player's hand.
    ///
    /// Cannot be held simultaneously with `inventory()` or `guard()`.
    #[allow(clippy::missing_panics_doc)]
    pub fn item(&mut self) -> &mut ItemStack {
        self.inv_guard
            .get_player_inventory_mut(self.inv_id)
            .expect("player inventory must be locked")
            .get_item_in_hand_mut(self.hand)
    }

    /// Returns a mutable reference to the player's inventory.
    #[allow(clippy::missing_panics_doc)]
    pub fn inventory(&mut self) -> &mut PlayerInventory {
        self.inv_guard
            .get_player_inventory_mut(self.inv_id)
            .expect("player inventory must be locked")
    }

    /// Returns a mutable reference to the container lock guard.
    pub const fn guard(&mut self) -> &mut ContainerLockGuard {
        self.inv_guard
    }
}
