//! Context types and results for block and item interactions.

use glam::DVec3;
use std::sync::Arc;
use steel_registry::REGISTRY;
use steel_registry::blocks::properties::Direction;
use steel_registry::item_stack::ItemStack;
use steel_utils::BlockPos;
use steel_utils::types::InteractionHand;

use crate::entity::Entity;
use crate::fluid::FluidStateExt;
use crate::inventory::lock::{ContainerLockGuard, ContainerRef, SyncPlayerInv};
use crate::player::Player;
use crate::player::player_inventory::PlayerInventory;
use crate::world::World;
pub use steel_registry::items::item::BlockHitResult;

/// Result of an interaction (item use, block use, etc.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionResult {
    /// The interaction succeeded and consumed the action.
    Success,
    /// The interaction succeeded and the server should broadcast the swing.
    SuccessServer,
    /// The interaction consumed the action without swinging.
    Consume,
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
    pub const fn consumes_action(self) -> bool {
        matches!(
            self,
            InteractionResult::Success
                | InteractionResult::SuccessServer
                | InteractionResult::Consume
                | InteractionResult::Fail
        )
    }

    /// Returns true when vanilla requests the server to broadcast the swing.
    #[must_use]
    pub const fn should_swing_server(self) -> bool {
        matches!(self, InteractionResult::SuccessServer)
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
    /// Whether the player is using the secondary action, normally sneaking.
    pub is_secondary_use_active: bool,
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

    /// Returns the vertical direction the player is looking toward.
    ///
    /// Based on Java's `BlockPlaceContext.getNearestLookingVerticalDirection()`.
    #[must_use]
    pub const fn get_nearest_looking_vertical_direction(&self) -> Direction {
        if self.pitch < 0.0 {
            Direction::Up
        } else {
            Direction::Down
        }
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

    /// Returns true if the block at the relative position contains full water.
    #[must_use]
    pub fn is_full_water(&self) -> bool {
        use crate::fluid::get_fluid_state;
        let fluid_state = get_fluid_state(self.world, self.relative_pos);
        fluid_state.is_full() && fluid_state.is_water()
    }
}

/// Access to the player's inventory.
///
/// This handle does not hold the inventory lock by itself. Use the closure
/// methods to keep lock scopes short and avoid carrying an inventory guard
/// through block behavior, world mutation, or menu opening.
pub struct InventoryAccess {
    inventory: SyncPlayerInv,
    hand: InteractionHand,
}

impl InventoryAccess {
    /// Creates a new `InventoryAccess` instance.
    pub const fn new(inventory: SyncPlayerInv, hand: InteractionHand) -> Self {
        Self { inventory, hand }
    }

    /// Runs `f` with mutable access to the item in the player's hand.
    pub fn with_item<R>(&self, f: impl FnOnce(&mut ItemStack) -> R) -> R {
        let mut inventory = self.inventory.lock();
        f(inventory.get_item_in_hand_mut(self.hand))
    }

    /// Runs `f` with mutable access to the player's inventory.
    pub fn with_inventory<R>(&self, f: impl FnOnce(&mut PlayerInventory) -> R) -> R {
        let mut inventory = self.inventory.lock();
        f(&mut inventory)
    }

    /// Runs `f` with a container guard containing the player's inventory.
    ///
    /// Prefer [`Self::with_item`] or [`Self::with_inventory`] unless an operation
    /// must interoperate with APIs that require `ContainerLockGuard`.
    pub fn with_guard<R>(&self, f: impl FnOnce(&mut ContainerLockGuard) -> R) -> R {
        let inv_ref = ContainerRef::PlayerInventory(self.inventory.clone());
        let mut guard = ContainerLockGuard::lock_all(&[&inv_ref]);
        f(&mut guard)
    }
}

/// Context for using an item on a block.
///
/// Immutable fields (`player`, `hand`, `world`, `hit_result`) can be accessed
/// freely while `inv` is mutably borrowed — the borrow checker tracks them as
/// disjoint fields.
pub struct UseOnContext<'a> {
    /// The player using the item.
    pub player: &'a Player,
    /// Which hand the item is in.
    pub hand: InteractionHand,
    /// Information about where the block was hit.
    pub hit_result: BlockHitResult,
    /// The world where the interaction is happening.
    pub world: &'a Arc<World>,
    /// Mutable inventory access.
    pub inv: InventoryAccess,
}

impl<'a> UseOnContext<'a> {
    /// Creates a new `UseOnContext`.
    #[must_use]
    pub const fn new(
        player: &'a Player,
        hand: InteractionHand,
        hit_result: BlockHitResult,
        world: &'a Arc<World>,
        inventory: SyncPlayerInv,
    ) -> Self {
        Self {
            player,
            hand,
            hit_result,
            world,
            inv: InventoryAccess::new(inventory, hand),
        }
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

        let (yaw, pitch) = self.player.rotation();

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
            is_secondary_use_active: self.player.is_secondary_use_active(),
            world: self.world,
        })
    }
}

/// Context for using an item (general usage).
///
/// Immutable fields (`player`, `hand`, `world`) can be accessed freely while
/// `inv` is mutably borrowed.
pub struct UseItemContext<'a> {
    /// The player using the item.
    pub player: &'a Player,
    /// Which hand the item is in.
    pub hand: InteractionHand,
    /// The world where the interaction is happening.
    pub world: &'a Arc<World>,
    /// Mutable inventory access.
    pub inv: InventoryAccess,
}

impl<'a> UseItemContext<'a> {
    /// Creates a new `UseItemContext`.
    #[must_use]
    pub const fn new(
        player: &'a Player,
        hand: InteractionHand,
        world: &'a Arc<World>,
        inventory: SyncPlayerInv,
    ) -> Self {
        Self {
            player,
            hand,
            world,
            inv: InventoryAccess::new(inventory, hand),
        }
    }
}
