//! Bell block behavior.
//!
//! Bells are face-attached or ceiling-attached blocks that can be rung by clicking
//! or by redstone power. Ringing plays a sound and sends a block event to clients.
//!
//! Vanilla equivalent: `BellBlock` + `FaceAttachedHorizontalDirectionalBlock`.
//!
//! TODO:
//! - [ ] Projectile hit ringing behavior (requires block-projectile collision hooks)
//! - [ ] Explosion hit ringing behavior (requires block-explosion hooks)
//! - [ ] Raider highlighting / glowing effect during raids (requires raid and villager systems)

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BellAttachType, BlockStateProperties, Direction};
use steel_registry::blocks::shapes::SupportType;
use steel_registry::{REGISTRY, sound_events, vanilla_blocks, vanilla_game_events};
use steel_utils::axis::Axis;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::InventoryAccess;
use crate::behavior::block::BlockBehavior;
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::entity::Entity;
use crate::player::Player;
use crate::world::game_event_context::GameEventContext;
use crate::world::{LevelReader, ScheduledTickAccess, World};

/// Behavior for the bell block.
///
/// # Differences from Vanilla
/// - `update_shape` optimizes support sturdiness checks by validating the given `neighbor_state`
///   directly instead of querying the world database again.
/// - Placement checks for double-wall attachments are short-circuited based on the clicked face
///   axis to avoid querying neighbors of the perpendicular axis.
#[block_behavior]
pub struct BellBlock {
    block: BlockRef,
}

impl BellBlock {
    /// Creates a new bell block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Returns the direction of the block supporting the bell.
    ///
    /// - Floor: support is Down.
    /// - Ceiling: support is Up.
    /// - Wall: support is in the horizontal facing direction.
    fn get_support_direction(state: BlockStateId) -> Direction {
        match state.get_value(&BlockStateProperties::BELL_ATTACHMENT) {
            BellAttachType::Floor => Direction::Down,
            BellAttachType::Ceiling => Direction::Up,
            _ => state.get_value(&BlockStateProperties::HORIZONTAL_FACING),
        }
    }

    /// Checks if a hit or click lands on a valid side of the bell to ring it.
    ///
    /// Vanilla equivalent: `BellBlock.isProperHit(BlockState, Direction, double)`.
    fn is_proper_hit(state: BlockStateId, clicked_direction: Direction, click_y: f64) -> bool {
        // 0.8124 represents the height of the bell hitbox (under 13/16ths of a block)
        // to ensure horizontal clicks hit the bell body rather than the support.
        if !clicked_direction.is_horizontal() || click_y > 0.8124 {
            return false;
        }

        let facing = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        let attachment = state.get_value(&BlockStateProperties::BELL_ATTACHMENT);

        match attachment {
            BellAttachType::Floor => facing.get_axis() == clicked_direction.get_axis(),
            BellAttachType::SingleWall | BellAttachType::DoubleWall => {
                facing.get_axis() != clicked_direction.get_axis()
            }
            BellAttachType::Ceiling => true,
        }
    }

    /// Helper to ring the bell, play the sound and send the block event.
    ///
    /// Returns `true` if the bell successfully rang.
    fn attempt_to_ring(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        direction: Option<Direction>,
        player: Option<&Player>,
    ) -> bool {
        if state.get_block() != self.block {
            return false;
        }

        let ring_direction =
            direction.unwrap_or(state.get_value(&BlockStateProperties::HORIZONTAL_FACING));

        // Play the ring sound
        world.play_block_sound(&sound_events::BLOCK_BELL_USE, pos, 2.0, 1.0, None);

        // Send CBlockEvent packet to trigger the swinging animation.
        // Down = 0, Up = 1, North = 2, South = 3, West = 4, East = 5.
        world.block_event(pos, self.block, 1, ring_direction as u8);

        // Dispatch vanilla game event
        world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            pos,
            &GameEventContext::new(player.map(|p| p as &dyn Entity), None),
        );

        true
    }

    const fn has_neighbor_signal<L: LevelReader + ?Sized>(_world: &L, _pos: BlockPos) -> bool {
        // TODO: Query redstone neighbor signal once Steel has redstone power propagation.
        false
    }

    /// Checks if a block state is sturdy enough to support a bell in the given connection direction.
    fn is_support_sturdy(
        neighbor_state: BlockStateId,
        neighbor_pos: BlockPos,
        support_dir: Direction,
    ) -> bool {
        let face = support_dir.opposite();
        if support_dir == Direction::Up {
            neighbor_state.is_face_sturdy_for_at(neighbor_pos, face, SupportType::Center)
        } else {
            neighbor_state.is_face_sturdy_at(neighbor_pos, face)
        }
    }
}

impl BlockBehavior for BellBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let support_dir = Self::get_support_direction(state);
        let support_pos = pos.relative(support_dir);
        let support_state = world.get_block_state(support_pos);
        Self::is_support_sturdy(support_state, support_pos, support_dir)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let clicked_face = context.clicked_face;
        let pos = context.relative_pos;
        let world = context.world;
        let axis = clicked_face.get_axis();

        if axis.is_vertical() {
            let attachment = if clicked_face == Direction::Down {
                BellAttachType::Ceiling
            } else {
                BellAttachType::Floor
            };
            let state = self
                .block
                .default_state()
                .set_value(&BlockStateProperties::BELL_ATTACHMENT, attachment)
                .set_value(
                    &BlockStateProperties::HORIZONTAL_FACING,
                    context.horizontal_direction,
                );
            if self.can_survive(state, world, pos) {
                return Some(state);
            }
        } else {
            let double_attached = match axis {
                Axis::X => {
                    let west = pos.west();
                    let east = pos.east();
                    world
                        .get_block_state(west)
                        .is_face_sturdy_at(west, Direction::East)
                        && world
                            .get_block_state(east)
                            .is_face_sturdy_at(east, Direction::West)
                }
                Axis::Z => {
                    let north = pos.north();
                    let south = pos.south();
                    world
                        .get_block_state(north)
                        .is_face_sturdy_at(north, Direction::South)
                        && world
                            .get_block_state(south)
                            .is_face_sturdy_at(south, Direction::North)
                }
                Axis::Y => false,
            };

            let attachment = if double_attached {
                BellAttachType::DoubleWall
            } else {
                BellAttachType::SingleWall
            };

            let mut state = self
                .block
                .default_state()
                .set_value(
                    &BlockStateProperties::HORIZONTAL_FACING,
                    clicked_face.opposite(),
                )
                .set_value(&BlockStateProperties::BELL_ATTACHMENT, attachment);

            if self.can_survive(state, world, pos) {
                return Some(state);
            }

            // Fallback: try floor/ceiling attachment on wall placement if direct wall attachment fails
            let below_pos = pos.below();
            let can_attach_below = world
                .get_block_state(below_pos)
                .is_face_sturdy_at(below_pos, Direction::Up);
            let fallback_attachment = if can_attach_below {
                BellAttachType::Floor
            } else {
                BellAttachType::Ceiling
            };

            state = state.set_value(&BlockStateProperties::BELL_ATTACHMENT, fallback_attachment);
            if self.can_survive(state, world, pos) {
                return Some(state);
            }
        }

        None
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        _world: &dyn ScheduledTickAccess,
        _pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let attachment = state.get_value(&BlockStateProperties::BELL_ATTACHMENT);
        let support_dir = Self::get_support_direction(state);

        if support_dir == direction
            && attachment != BellAttachType::DoubleWall
            && !Self::is_support_sturdy(neighbor_state, neighbor_pos, support_dir)
        {
            return REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        }

        let facing = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        if direction.get_axis() == facing.get_axis() {
            if attachment == BellAttachType::DoubleWall
                && !neighbor_state.is_face_sturdy_at(neighbor_pos, direction)
            {
                return state
                    .set_value(
                        &BlockStateProperties::BELL_ATTACHMENT,
                        BellAttachType::SingleWall,
                    )
                    .set_value(
                        &BlockStateProperties::HORIZONTAL_FACING,
                        direction.opposite(),
                    );
            }

            if attachment == BellAttachType::SingleWall
                && support_dir.opposite() == direction
                && neighbor_state.is_face_sturdy_at(neighbor_pos, facing)
            {
                return state.set_value(
                    &BlockStateProperties::BELL_ATTACHMENT,
                    BellAttachType::DoubleWall,
                );
            }
        }

        state
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        if source_block == self.block {
            return;
        }

        let signal = Self::has_neighbor_signal(world, pos);
        let powered = state.get_value(&BlockStateProperties::POWERED);
        if signal != powered {
            if signal {
                self.attempt_to_ring(state, world, pos, None, None);
            }
            let new_state = state.set_value(&BlockStateProperties::POWERED, signal);
            world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
        }
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let click_y = hit_result.location.y - f64::from(hit_result.block_pos.y());

        if Self::is_proper_hit(state, hit_result.direction, click_y)
            && self.attempt_to_ring(state, world, pos, Some(hit_result.direction), Some(player))
        {
            return InteractionResult::Success;
        }

        InteractionResult::Pass
    }
}

#[cfg(test)]
mod tests {
    use rustc_hash::FxHashMap;

    use steel_registry::blocks::properties::BellAttachType;
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_blocks;

    use super::*;

    struct TestLevel {
        blocks: FxHashMap<BlockPos, BlockStateId>,
    }

    impl TestLevel {
        fn new() -> Self {
            Self {
                blocks: FxHashMap::default(),
            }
        }

        fn set_block(&mut self, pos: BlockPos, state: BlockStateId) {
            self.blocks.insert(pos, state);
        }
    }

    impl LevelReader for TestLevel {
        fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
            self.blocks
                .get(&pos)
                .copied()
                .unwrap_or_else(|| vanilla_blocks::AIR.default_state())
        }

        fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
            0
        }

        fn min_y(&self) -> i32 {
            -64
        }

        fn height(&self) -> i32 {
            384
        }
    }

    #[test]
    fn test_bell_support_direction() {
        init_test_registry();
        let bell = vanilla_blocks::BELL.default_state();

        // Floor attachment -> support is Down
        let floor_bell = bell.set_value(
            &BlockStateProperties::BELL_ATTACHMENT,
            BellAttachType::Floor,
        );
        assert_eq!(
            BellBlock::get_support_direction(floor_bell),
            Direction::Down
        );

        // Ceiling attachment -> support is Up
        let ceiling_bell = bell.set_value(
            &BlockStateProperties::BELL_ATTACHMENT,
            BellAttachType::Ceiling,
        );
        assert_eq!(
            BellBlock::get_support_direction(ceiling_bell),
            Direction::Up
        );

        // Wall attachment -> support is horizontal facing direction
        let wall_bell = bell
            .set_value(
                &BlockStateProperties::BELL_ATTACHMENT,
                BellAttachType::SingleWall,
            )
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, Direction::North);
        assert_eq!(
            BellBlock::get_support_direction(wall_bell),
            Direction::North
        );
    }

    #[test]
    fn test_bell_is_proper_hit() {
        init_test_registry();
        let bell = vanilla_blocks::BELL
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, Direction::North);

        // Y axis click is always improper
        assert!(!BellBlock::is_proper_hit(bell, Direction::Up, 0.5));
        assert!(!BellBlock::is_proper_hit(bell, Direction::Down, 0.5));

        // Click above bell body (height > 0.8124) is improper
        assert!(!BellBlock::is_proper_hit(bell, Direction::North, 0.9));

        // Floor attachment: click axis must match facing axis (North/South vs East/West)
        let floor_bell = bell.set_value(
            &BlockStateProperties::BELL_ATTACHMENT,
            BellAttachType::Floor,
        );
        assert!(BellBlock::is_proper_hit(floor_bell, Direction::North, 0.5));
        assert!(BellBlock::is_proper_hit(floor_bell, Direction::South, 0.5));
        assert!(!BellBlock::is_proper_hit(floor_bell, Direction::East, 0.5));

        // Ceiling attachment: click from any horizontal direction is proper
        let ceiling_bell = bell.set_value(
            &BlockStateProperties::BELL_ATTACHMENT,
            BellAttachType::Ceiling,
        );
        assert!(BellBlock::is_proper_hit(
            ceiling_bell,
            Direction::North,
            0.5
        ));
        assert!(BellBlock::is_proper_hit(ceiling_bell, Direction::East, 0.5));

        // Wall attachment: click axis must NOT match facing axis
        let wall_bell = bell.set_value(
            &BlockStateProperties::BELL_ATTACHMENT,
            BellAttachType::SingleWall,
        );
        assert!(!BellBlock::is_proper_hit(wall_bell, Direction::North, 0.5));
        assert!(BellBlock::is_proper_hit(wall_bell, Direction::East, 0.5));
    }

    #[test]
    fn test_bell_can_survive() {
        init_test_registry();
        let behavior = BellBlock::new(&vanilla_blocks::BELL);
        let pos = BlockPos::new(0, 0, 0);

        // Floor support
        let floor_bell = vanilla_blocks::BELL.default_state().set_value(
            &BlockStateProperties::BELL_ATTACHMENT,
            BellAttachType::Floor,
        );

        let mut level = TestLevel::new();
        // Air below -> cannot survive
        assert!(!behavior.can_survive(floor_bell, &level, pos));

        // Stone below (sturdy on top) -> can survive
        level.set_block(pos.below(), vanilla_blocks::STONE.default_state());
        assert!(behavior.can_survive(floor_bell, &level, pos));

        // Ceiling support
        let ceiling_bell = vanilla_blocks::BELL.default_state().set_value(
            &BlockStateProperties::BELL_ATTACHMENT,
            BellAttachType::Ceiling,
        );

        let mut level = TestLevel::new();
        // Air above -> cannot survive
        assert!(!behavior.can_survive(ceiling_bell, &level, pos));

        // Stone above -> can survive
        level.set_block(pos.above(), vanilla_blocks::STONE.default_state());
        assert!(behavior.can_survive(ceiling_bell, &level, pos));
    }
}
