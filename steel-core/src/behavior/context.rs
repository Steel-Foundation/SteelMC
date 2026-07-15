//! Context types and results for block and item interactions.

use glam::DVec3;
use std::sync::Arc;
use steel_registry::blocks::properties::Direction;
use steel_registry::item_stack::ItemStack;
use steel_utils::BlockPos;
use steel_utils::types::InteractionHand;

use crate::behavior::BlockStateBehaviorExt;
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

    /// Returns true for vanilla `InteractionResult.Success` variants that run
    /// item-use side effects such as `minecraft:use_cooldown`.
    #[must_use]
    pub const fn should_apply_item_use_side_effects(self) -> bool {
        matches!(
            self,
            InteractionResult::Success
                | InteractionResult::SuccessServer
                | InteractionResult::Consume
        )
    }
}

/// Context for placing a block.
///
/// Vanilla porting map:
/// - `UseOnContext.getClickedPos()` is [`Self::hit_pos`].
/// - `BlockPlaceContext.getClickedPos()` is [`Self::place_pos`].
/// - `BlockPlaceContext.replacingClickedOnBlock()` is
///   [`Self::replaces_clicked_block`].
///
/// When translating vanilla block placement code, do not map
/// `BlockPlaceContext.getClickedPos()` to [`Self::hit_pos`].
pub struct BlockPlaceContext<'a> {
    /// Raw block position from the hit result.
    ///
    /// Vanilla equivalent: `UseOnContext.getClickedPos()`.
    hit_pos: BlockPos,
    /// The face of the block that was clicked.
    clicked_face: Direction,
    /// The exact location where the click occurred.
    click_location: DVec3,
    /// Whether the click was inside the block.
    inside: bool,
    /// Position where the block will be placed.
    ///
    /// Vanilla equivalent: `BlockPlaceContext.getClickedPos()`. Vanilla returns
    /// the raw hit position only when replacing the clicked block; otherwise it
    /// returns the adjacent block position in the clicked-face direction.
    place_pos: BlockPos,
    /// Whether placement replaces the hit block itself.
    ///
    /// Vanilla equivalent: `BlockPlaceContext.replacingClickedOnBlock()`.
    replaces_clicked_block: bool,
    /// The world where the block is being placed.
    pub world: &'a Arc<World>,
    source: PlacementSource<'a>,
    mode: PlacementMode,
}

impl<'a> BlockPlaceContext<'a> {
    /// Creates a placement context from a source and effective hit result.
    #[must_use]
    pub fn new(
        world: &'a Arc<World>,
        source: PlacementSource<'a>,
        hit_result: &BlockHitResult,
    ) -> Self {
        Self::with_mode(world, source, hit_result, PlacementMode::Standard)
    }

    fn with_mode(
        world: &'a Arc<World>,
        source: PlacementSource<'a>,
        hit_result: &BlockHitResult,
        mode: PlacementMode,
    ) -> Self {
        let hit_pos = hit_result.block_pos;
        let mut context = Self {
            hit_pos,
            clicked_face: hit_result.direction,
            click_location: hit_result.location,
            inside: hit_result.inside,
            place_pos: hit_pos,
            replaces_clicked_block: true,
            world,
            source,
            mode,
        };
        context.resolve_placement_geometry();
        context
    }

    /// Creates vanilla's playerless `DirectionalPlaceContext` equivalent.
    #[must_use]
    pub fn directional(
        world: &'a Arc<World>,
        pos: BlockPos,
        direction: Direction,
        stack: &'a mut ItemStack,
        clicked_face: Direction,
    ) -> Self {
        let hit_result = BlockHitResult {
            location: DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()),
                f64::from(pos.z()) + 0.5,
            ),
            direction: clicked_face,
            block_pos: pos,
            miss: false,
            inside: false,
            world_border_hit: false,
        };
        let source = PlacementSource::direct(
            None,
            InteractionHand::MainHand,
            stack,
            PlacementOrientation::Directional { direction },
            false,
        );
        Self::with_mode(world, source, &hit_result, PlacementMode::Directional)
    }

    fn resolve_placement_geometry(&mut self) {
        self.place_pos = self.hit_pos;
        self.replaces_clicked_block = true;
        self.replaces_clicked_block = self
            .world
            .get_block_state(self.hit_pos)
            .can_be_replaced(self);
        if self.mode == PlacementMode::Standard && !self.replaces_clicked_block {
            self.place_pos = self.clicked_face.relative(self.hit_pos);
        }
    }

    /// Returns whether the effective placement position can be replaced.
    #[must_use]
    pub fn can_place(&self) -> bool {
        if self.mode == PlacementMode::Directional {
            return self
                .world
                .get_block_state(self.hit_pos)
                .can_be_replaced(self);
        }

        self.replaces_clicked_block
            || self
                .world
                .get_block_state(self.place_pos)
                .can_be_replaced(self)
    }

    /// Creates the shifted context used by vanilla `BlockPlaceContext.at`.
    #[must_use]
    pub fn at(mut self, pos: BlockPos, direction: Direction) -> Self {
        let (step_x, step_y, step_z) = direction.offset();
        self.hit_pos = pos;
        self.clicked_face = direction;
        self.click_location = DVec3::new(
            f64::from(pos.x()) + 0.5 + f64::from(step_x) * 0.5,
            f64::from(pos.y()) + 0.5 + f64::from(step_y) * 0.5,
            f64::from(pos.z()) + 0.5 + f64::from(step_z) * 0.5,
        );
        self.inside = false;
        self.mode = PlacementMode::Standard;
        self.resolve_placement_geometry();
        self
    }

    /// Returns the raw block position from the hit result.
    #[must_use]
    pub const fn hit_pos(&self) -> BlockPos {
        self.hit_pos
    }

    /// Returns the face from the effective hit result.
    #[must_use]
    pub const fn clicked_face(&self) -> Direction {
        self.clicked_face
    }

    /// Returns the exact effective hit location.
    #[must_use]
    pub const fn click_location(&self) -> DVec3 {
        self.click_location
    }

    /// Returns whether the effective hit location is inside the hit block.
    #[must_use]
    pub const fn is_inside(&self) -> bool {
        self.inside
    }

    /// Returns the effective block placement position.
    #[must_use]
    pub const fn place_pos(&self) -> BlockPos {
        self.place_pos
    }

    /// Returns whether placement replaces the originally hit block.
    #[must_use]
    pub fn replaces_clicked_block(&self) -> bool {
        if self.mode == PlacementMode::Directional {
            self.can_place()
        } else {
            self.replaces_clicked_block
        }
    }

    /// Returns the player associated with this placement, if any.
    #[must_use]
    pub const fn player(&self) -> Option<&Player> {
        self.source.player()
    }

    /// Returns the interaction hand associated with this placement.
    #[must_use]
    pub const fn hand(&self) -> InteractionHand {
        self.source.hand()
    }

    /// Runs `f` with read access to the current placement stack.
    pub fn with_item<R>(&self, f: impl FnOnce(&ItemStack) -> R) -> R {
        self.source.with_item(f)
    }

    /// Runs `f` with mutable access to the current placement stack.
    pub fn with_item_mut<R>(&mut self, f: impl FnOnce(&mut ItemStack) -> R) -> R {
        self.source.with_item_mut(f)
    }

    /// Returns the placement source used by block placement callbacks.
    #[must_use]
    pub const fn source(&self) -> &PlacementSource<'a> {
        &self.source
    }

    /// Returns the horizontal placement direction.
    #[must_use]
    pub fn horizontal_direction(&self) -> Direction {
        self.source.orientation.horizontal_direction()
    }

    /// Returns the placement rotation.
    #[must_use]
    pub const fn rotation(&self) -> f32 {
        self.source.orientation.rotation()
    }

    /// Returns whether secondary use is active for this placement.
    #[must_use]
    pub const fn is_secondary_use_active(&self) -> bool {
        self.source.is_secondary_use_active
    }

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
        self.source.orientation.nearest_vertical_direction()
    }

    /// Returns all 6 directions ordered by how closely the player is looking at them.
    ///
    /// Based on Java's `BlockPlaceContext.getNearestLookingDirections()`.
    /// When not replacing the clicked block, the opposite of the clicked face
    /// is moved to the front of the array.
    #[must_use]
    pub fn get_nearest_looking_directions(&self) -> [Direction; 6] {
        let (mut directions, adjust_for_replacement) = self.source.orientation.directions();

        // If not replacing the clicked block, prioritize the opposite of clicked face
        if adjust_for_replacement && !self.replaces_clicked_block {
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

    /// Returns true if the block at the placement position is a water source.
    #[must_use]
    pub fn is_water_source(&self) -> bool {
        use crate::fluid::get_fluid_state;
        let fluid_state = get_fluid_state(self.world, self.place_pos);
        fluid_state.is_source() && fluid_state.is_water()
    }

    /// Returns true if the block at the placement position contains full water.
    #[must_use]
    pub fn is_full_water(&self) -> bool {
        use crate::fluid::get_fluid_state;
        let fluid_state = get_fluid_state(self.world, self.place_pos);
        fluid_state.is_full() && fluid_state.is_water()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlacementMode {
    Standard,
    Directional,
}

/// Placement direction behavior derived from either a player or a synthetic caller.
#[derive(Clone, Copy, Debug)]
pub enum PlacementOrientation {
    /// Vanilla player-derived yaw and pitch.
    Player {
        /// Player yaw in degrees.
        rotation: f32,
        /// Player pitch in degrees.
        pitch: f32,
    },
    /// Vanilla `DirectionalPlaceContext` orientation.
    Directional {
        /// Synthetic placement direction.
        direction: Direction,
    },
}

impl PlacementOrientation {
    fn horizontal_direction(self) -> Direction {
        match self {
            Self::Player { rotation, .. } => Direction::from_yaw(rotation),
            Self::Directional { direction } if direction.is_horizontal() => direction,
            Self::Directional { .. } => Direction::North,
        }
    }

    const fn rotation(self) -> f32 {
        match self {
            Self::Player { rotation, .. } => rotation,
            Self::Directional { direction } => match direction {
                Direction::Down | Direction::Up => -90.0,
                Direction::South => 0.0,
                Direction::West => 90.0,
                Direction::North => 180.0,
                Direction::East => 270.0,
            },
        }
    }

    const fn nearest_vertical_direction(self) -> Direction {
        match self {
            Self::Player { pitch, .. } if pitch < 0.0 => Direction::Up,
            Self::Player { .. } | Self::Directional { .. } => Direction::Down,
        }
    }

    fn directions(self) -> ([Direction; 6], bool) {
        match self {
            Self::Player { rotation, pitch } => {
                (Direction::ordered_by_nearest(rotation, pitch), true)
            }
            Self::Directional { direction } => (directional_placement_directions(direction), false),
        }
    }
}

const fn directional_placement_directions(direction: Direction) -> [Direction; 6] {
    match direction {
        Direction::Down => [
            Direction::Down,
            Direction::North,
            Direction::East,
            Direction::South,
            Direction::West,
            Direction::Up,
        ],
        Direction::Up => [
            Direction::Down,
            Direction::Up,
            Direction::North,
            Direction::East,
            Direction::South,
            Direction::West,
        ],
        Direction::North => [
            Direction::Down,
            Direction::North,
            Direction::East,
            Direction::West,
            Direction::Up,
            Direction::South,
        ],
        Direction::South => [
            Direction::Down,
            Direction::South,
            Direction::East,
            Direction::West,
            Direction::Up,
            Direction::North,
        ],
        Direction::West => [
            Direction::Down,
            Direction::West,
            Direction::South,
            Direction::Up,
            Direction::North,
            Direction::East,
        ],
        Direction::East => [
            Direction::Down,
            Direction::East,
            Direction::South,
            Direction::Up,
            Direction::North,
            Direction::West,
        ],
    }
}

enum PlacementItemSource<'a> {
    PlayerHand(InventoryAccess),
    Direct(&'a mut ItemStack),
}

/// Player and item data retained across one block placement operation.
///
/// Player-backed access deliberately re-resolves the selected hand under short
/// inventory locks. The stack may therefore change between accesses; retaining
/// one inventory guard across behavior dispatch could deadlock callbacks which
/// open a menu. Direct access retains the exact borrowed stack for the whole
/// operation.
pub struct PlacementSource<'a> {
    player: Option<&'a Player>,
    hand: InteractionHand,
    item: PlacementItemSource<'a>,
    orientation: PlacementOrientation,
    is_secondary_use_active: bool,
}

impl<'a> PlacementSource<'a> {
    /// Creates a placement source backed by a player's live hand.
    #[must_use]
    pub fn player_hand(player: &'a Player, inv: InventoryAccess) -> Self {
        let (rotation, pitch) = player.rotation();
        let hand = inv.hand;
        Self {
            player: Some(player),
            hand,
            item: PlacementItemSource::PlayerHand(inv),
            orientation: PlacementOrientation::Player { rotation, pitch },
            is_secondary_use_active: player.is_secondary_use_active(),
        }
    }

    /// Creates a source backed by a directly borrowed stack.
    #[must_use]
    pub const fn direct(
        player: Option<&'a Player>,
        hand: InteractionHand,
        stack: &'a mut ItemStack,
        orientation: PlacementOrientation,
        is_secondary_use_active: bool,
    ) -> Self {
        Self {
            player,
            hand,
            item: PlacementItemSource::Direct(stack),
            orientation,
            is_secondary_use_active,
        }
    }

    /// Returns the player associated with this source, if any.
    #[must_use]
    pub const fn player(&self) -> Option<&Player> {
        self.player
    }

    /// Returns the interaction hand associated with this source.
    #[must_use]
    pub const fn hand(&self) -> InteractionHand {
        self.hand
    }

    /// Runs `f` with read access to this source's current stack.
    pub fn with_item<R>(&self, f: impl FnOnce(&ItemStack) -> R) -> R {
        match &self.item {
            PlacementItemSource::PlayerHand(inv) => inv.with_item(|item| f(item)),
            PlacementItemSource::Direct(item) => f(item),
        }
    }

    /// Runs `f` with mutable access to this source's current stack.
    pub fn with_item_mut<R>(&mut self, f: impl FnOnce(&mut ItemStack) -> R) -> R {
        match &mut self.item {
            PlacementItemSource::PlayerHand(inv) => inv.with_item(f),
            PlacementItemSource::Direct(item) => f(item),
        }
    }
}

/// Access to the player's inventory.
///
/// This handle does not hold the inventory lock by itself. Use the closure
/// methods to keep lock scopes short and avoid carrying an inventory guard
/// through block behavior, world mutation, or menu opening.
#[derive(Clone)]
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
        let inv_ref = ContainerRef::from(self.inventory.clone());
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
    #[must_use]
    pub fn build_place_context(&self) -> BlockPlaceContext<'a> {
        BlockPlaceContext::new(
            self.world,
            PlacementSource::player_hand(self.player, self.inv.clone()),
            &self.hit_result,
        )
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use steel_registry::data_components::vanilla_components::BLOCK_STATE;
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_items;
    use steel_utils::locks::SyncMutex;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::inventory::container::Container;
    use crate::player::player_inventory::PlayerInventory;
    use crate::test_support::test_world;

    #[test]
    fn item_use_side_effects_apply_to_all_success_variants() {
        assert!(InteractionResult::Success.should_apply_item_use_side_effects());
        assert!(InteractionResult::SuccessServer.should_apply_item_use_side_effects());
        assert!(InteractionResult::Consume.should_apply_item_use_side_effects());
        assert!(!InteractionResult::Fail.should_apply_item_use_side_effects());
        assert!(!InteractionResult::Pass.should_apply_item_use_side_effects());
        assert!(!InteractionResult::TryEmptyHandInteraction.should_apply_item_use_side_effects());
    }

    #[test]
    fn player_hand_source_reads_current_components_and_mutates_the_hand() {
        init_test_registry();

        let inventory = Arc::new(SyncMutex::new(PlayerInventory::new(Weak::new())));
        inventory
            .lock()
            .set_item(0, ItemStack::with_count(&vanilla_items::LIGHT, 2));
        let access = InventoryAccess::new(inventory.clone(), InteractionHand::MainHand);
        let mut source = PlacementSource {
            player: None,
            hand: InteractionHand::MainHand,
            item: PlacementItemSource::PlayerHand(access),
            orientation: PlacementOrientation::Player {
                rotation: 0.0,
                pitch: 0.0,
            },
            is_secondary_use_active: false,
        };

        assert!(source.with_item(|item| item.get(BLOCK_STATE).is_some()));
        source.with_item_mut(|item| item.shrink(1));
        assert_eq!(
            inventory
                .lock()
                .get_item_in_hand(InteractionHand::MainHand)
                .count(),
            1
        );
    }

    #[test]
    fn replacement_dispatch_does_not_hold_the_inventory_lock() {
        init_test_registry();
        init_behaviors();

        let inventory = Arc::new(SyncMutex::new(PlayerInventory::new(Weak::new())));
        inventory
            .lock()
            .set_item(0, ItemStack::new(&vanilla_items::STONE));
        let source = PlacementSource {
            player: None,
            hand: InteractionHand::MainHand,
            item: PlacementItemSource::PlayerHand(InventoryAccess::new(
                inventory,
                InteractionHand::MainHand,
            )),
            orientation: PlacementOrientation::Player {
                rotation: 0.0,
                pitch: 0.0,
            },
            is_secondary_use_active: false,
        };
        let hit_result = BlockHitResult {
            location: DVec3::ZERO,
            direction: Direction::Up,
            block_pos: BlockPos::new(0, 80, 0),
            miss: false,
            inside: false,
            world_border_hit: false,
        };

        // Air replacement dispatch reads the live hand. Construction can only
        // complete if no inventory guard is held across behavior dispatch.
        let context = BlockPlaceContext::new(test_world(), source, &hit_result);
        assert!(context.replaces_clicked_block());
    }

    #[test]
    fn direct_source_mutates_the_callers_exact_stack() {
        init_test_registry();

        let mut stack = ItemStack::with_count(&vanilla_items::LIGHT, 2);
        {
            let mut source = PlacementSource::direct(
                None,
                InteractionHand::MainHand,
                &mut stack,
                PlacementOrientation::Directional {
                    direction: Direction::North,
                },
                false,
            );
            assert!(source.with_item(|item| item.get(BLOCK_STATE).is_some()));
            source.with_item_mut(|item| item.shrink(1));
        }
        assert_eq!(stack.count(), 1);
    }

    #[test]
    fn at_changes_geometry_and_retains_the_direct_source() {
        init_test_registry();
        init_behaviors();

        let mut stack = ItemStack::new(&vanilla_items::STONE);
        let hit_result = BlockHitResult {
            location: DVec3::ZERO,
            direction: Direction::Up,
            block_pos: BlockPos::new(0, 80, 0),
            miss: false,
            inside: true,
            world_border_hit: false,
        };
        let source = PlacementSource::direct(
            None,
            InteractionHand::MainHand,
            &mut stack,
            PlacementOrientation::Player {
                rotation: 0.0,
                pitch: 0.0,
            },
            false,
        );
        let context = BlockPlaceContext::new(test_world(), source, &hit_result);
        let shifted_pos = BlockPos::new(4, 90, 7);
        let mut shifted = context.at(shifted_pos, Direction::East);

        assert_eq!(shifted.hit_pos(), shifted_pos);
        assert_eq!(shifted.place_pos(), shifted_pos);
        assert_eq!(shifted.clicked_face(), Direction::East);
        assert_eq!(shifted.click_location(), DVec3::new(5.0, 90.5, 7.5));
        assert!(!shifted.is_inside());
        assert!(shifted.with_item(|item| item.is(&vanilla_items::STONE)));
        shifted.with_item_mut(|item| item.shrink(1));
        drop(shifted);
        assert!(stack.is_empty());
    }

    #[test]
    fn directional_context_uses_vanilla_direction_order() {
        init_test_registry();
        init_behaviors();

        let mut stack = ItemStack::new(&vanilla_items::STONE);
        let context = BlockPlaceContext::directional(
            test_world(),
            BlockPos::new(2, 80, 3),
            Direction::West,
            &mut stack,
            Direction::Up,
        );

        assert!(context.player().is_none());
        assert_eq!(context.horizontal_direction(), Direction::West);
        assert_eq!(
            context.get_nearest_looking_directions(),
            [
                Direction::Down,
                Direction::West,
                Direction::South,
                Direction::Up,
                Direction::North,
                Direction::East,
            ]
        );
    }
}
