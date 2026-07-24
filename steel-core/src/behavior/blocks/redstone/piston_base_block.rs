//! Vanilla piston and sticky-piston behavior.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, PistonType};
use steel_registry::{sound_events, vanilla_blocks, vanilla_game_events};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Downcast as _};

use super::java_hash;
use super::piston_structure_resolver::{PistonLevel, PistonStructureResolver};
use crate::behavior::{BLOCK_BEHAVIORS, BlockBehavior, BlockPlaceContext, PlacementSource};
use crate::block_entity::SharedBlockEntity;
use crate::block_entity::entities::PistonMovingBlockEntity;
use crate::entity::ai::path::PathComputationType;
use crate::world::game_event_context::GameEventContext;
use crate::world::{LevelReader, SignalGetter as _, World};

const UPDATE_RETRACT_BASE: UpdateFlags = UpdateFlags::UPDATE_INVISIBLE
    .union(UpdateFlags::UPDATE_KNOWN_SHAPE)
    .union(UpdateFlags::UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS);
const UPDATE_MOVING_BLOCK: UpdateFlags = UpdateFlags::UPDATE_INVISIBLE
    .union(UpdateFlags::UPDATE_MOVE_BY_PISTON)
    .union(UpdateFlags::UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS);
const UPDATE_DESTROYED_BLOCK: UpdateFlags =
    UpdateFlags::UPDATE_CLIENTS.union(UpdateFlags::UPDATE_KNOWN_SHAPE);
const UPDATE_CLEARED_MOVED_BLOCK: UpdateFlags = UpdateFlags::UPDATE_CLIENTS
    .union(UpdateFlags::UPDATE_KNOWN_SHAPE)
    .union(UpdateFlags::UPDATE_MOVE_BY_PISTON);

/// Vanilla `PistonBaseBlock` shared by normal and sticky pistons.
#[block_behavior]
pub struct PistonBaseBlock {
    block: BlockRef,
    #[json_arg(value, json = "is_sticky")]
    sticky: bool,
}

impl PistonBaseBlock {
    /// Creates normal or sticky piston behavior from extracted constructor data.
    #[must_use]
    pub const fn new(block: BlockRef, is_sticky: bool) -> Self {
        Self {
            block,
            sticky: is_sticky,
        }
    }

    const fn direction_from_legacy_id(id: i32) -> Direction {
        match id & 7 {
            1 => Direction::Up,
            2 => Direction::North,
            3 => Direction::South,
            4 => Direction::West,
            5 => Direction::East,
            _ => Direction::Down,
        }
    }

    const fn direction_legacy_id(direction: Direction) -> i32 {
        match direction {
            Direction::Down => 0,
            Direction::Up => 1,
            Direction::North => 2,
            Direction::South => 3,
            Direction::West => 4,
            Direction::East => 5,
        }
    }

    fn neighbor_signal(world: &World, pos: BlockPos, push_direction: Direction) -> bool {
        for direction in Direction::ALL {
            if direction != push_direction && world.has_signal(pos.relative(direction), direction) {
                return true;
            }
        }
        if world.has_signal(pos, Direction::Down) {
            return true;
        }

        let above = pos.above();
        for direction in Direction::ALL {
            if direction != Direction::Down
                && world.has_signal(above.relative(direction), direction)
            {
                return true;
            }
        }
        false
    }

    fn check_if_extend(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        let direction = state.get_value(&BlockStateProperties::FACING);
        let powered = Self::neighbor_signal(world, pos, direction);
        if powered && !state.get_value(&BlockStateProperties::EXTENDED) {
            let mut resolver = PistonStructureResolver::new(world.as_ref(), pos, direction, true);
            if resolver.resolve() {
                world.block_event(pos, self.block, 0, Self::direction_legacy_id(direction));
            }
            return;
        }
        if powered || !state.get_value(&BlockStateProperties::EXTENDED) {
            return;
        }

        let pushed_pos = pos.relative_n(direction, 2);
        let pushed_state = world.get_block_state(pushed_pos);
        let event = if pushed_state.get_block() == &vanilla_blocks::MOVING_PISTON
            && pushed_state.get_value(&BlockStateProperties::FACING) == direction
            && world
                .get_block_entity(pushed_pos)
                .is_some_and(|block_entity| {
                    block_entity
                        .downcast_ref::<PistonMovingBlockEntity>()
                        .is_some_and(|piston| {
                            piston.is_extending()
                                && (piston.progress(0.0) < 0.5
                                    || world.game_time() == piston.last_ticked()
                                    || world.is_handling_tick())
                        })
                }) {
            2
        } else {
            1
        };
        world.block_event(pos, self.block, event, Self::direction_legacy_id(direction));
    }

    fn finish_moving_block_entity(world: &Arc<World>, pos: BlockPos) -> bool {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return false;
        };
        let Some(piston) = block_entity.downcast_ref::<PistonMovingBlockEntity>() else {
            return false;
        };
        piston.final_tick(world);
        true
    }

    fn moving_block_entity(
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        moved_state: BlockStateId,
        direction: Direction,
        extending: bool,
        source: bool,
    ) -> SharedBlockEntity {
        Arc::new(PistonMovingBlockEntity::new_moving(
            Arc::downgrade(world),
            pos,
            state,
            moved_state,
            direction,
            extending,
            source,
        ))
    }

    #[expect(
        clippy::float_cmp,
        reason = "vanilla uses -1.0 as the exact unbreakable destroy-time sentinel"
    )]
    pub(super) fn is_pushable(
        state: BlockStateId,
        world: &dyn PistonLevel,
        pos: BlockPos,
        direction: Direction,
        allow_destroyable: bool,
        connection_direction: Direction,
    ) -> bool {
        if world.is_outside_build_height(pos.y()) || !world.is_within_world_border(pos) {
            return false;
        }
        if state.is_air() {
            return true;
        }

        let block = state.get_block();
        if block == &vanilla_blocks::OBSIDIAN
            || block == &vanilla_blocks::CRYING_OBSIDIAN
            || block == &vanilla_blocks::RESPAWN_ANCHOR
            || block == &vanilla_blocks::REINFORCED_DEEPSLATE
        {
            return false;
        }
        if (direction == Direction::Down && pos.y() == world.min_y())
            || (direction == Direction::Up && pos.y() == world.max_y_exclusive() - 1)
        {
            return false;
        }

        let behavior = BLOCK_BEHAVIORS.get_behavior(block);
        if !behavior.is_piston_base() {
            if block.config.destroy_time == -1.0 {
                return false;
            }
            match block.config.push_reaction {
                PushReaction::Block => return false,
                PushReaction::Destroy => return allow_destroyable,
                PushReaction::PushOnly => return direction == connection_direction,
                PushReaction::Normal | PushReaction::Ignore => {}
            }
        } else if state.get_value(&BlockStateProperties::EXTENDED) {
            return false;
        }

        !state.has_block_entity()
    }

    #[expect(
        clippy::too_many_lines,
        reason = "keeping vanilla's ordered piston mutation sequence together makes parity auditable"
    )]
    fn move_blocks(
        &self,
        world: &Arc<World>,
        piston_pos: BlockPos,
        direction: Direction,
        extending: bool,
    ) -> bool {
        let arm_pos = piston_pos.relative(direction);
        if !extending && world.get_block_state(arm_pos).get_block() == &vanilla_blocks::PISTON_HEAD
        {
            world.set_block(
                arm_pos,
                vanilla_blocks::AIR.default_state(),
                UPDATE_RETRACT_BASE,
            );
        }

        let mut resolver =
            PistonStructureResolver::new(world.as_ref(), piston_pos, direction, extending);
        if !resolver.resolve() {
            return false;
        }

        let to_push = resolver.to_push().to_vec();
        let to_destroy = resolver.to_destroy().to_vec();
        let push_direction = resolver.push_direction();
        let mut delete_after_move = Vec::with_capacity(to_push.len());
        let mut pushed_states = Vec::with_capacity(to_push.len());
        for &pos in &to_push {
            let state = world.get_block_state(pos);
            pushed_states.push(state);
            delete_after_move.push((pos, state));
        }

        let mut to_update = Vec::with_capacity(to_push.len() + to_destroy.len());
        for &pos in to_destroy.iter().rev() {
            let state = world.get_block_state(pos);
            // TODO: Pass the block entity to loot evaluation once block-entity components and
            // post-refactor container item slices are available, as Vanilla does here.
            world.drop_resources(state, pos);
            world.set_block(
                pos,
                vanilla_blocks::AIR.default_state(),
                UPDATE_DESTROYED_BLOCK,
            );
            world.game_event(
                &vanilla_game_events::BLOCK_DESTROY,
                pos,
                &GameEventContext::new(None, Some(state)),
            );
            to_update.push(state);
        }

        for (index, &pos) in to_push.iter().enumerate().rev() {
            let state = world.get_block_state(pos);
            let destination = pos.relative(push_direction);
            delete_after_move.retain(|(delete_pos, _)| *delete_pos != destination);
            let moving_state = vanilla_blocks::MOVING_PISTON
                .default_state()
                .set_value(&BlockStateProperties::FACING, direction);
            world.set_block(destination, moving_state, UPDATE_MOVING_BLOCK);
            world.set_block_entity(Self::moving_block_entity(
                world,
                destination,
                moving_state,
                pushed_states[index],
                direction,
                extending,
                false,
            ));
            to_update.push(state);
        }

        if extending {
            let head_state = vanilla_blocks::PISTON_HEAD
                .default_state()
                .set_value(&BlockStateProperties::FACING, direction)
                .set_value(
                    &BlockStateProperties::PISTON_TYPE,
                    if self.sticky {
                        PistonType::Sticky
                    } else {
                        PistonType::Normal
                    },
                );
            let moving_state = vanilla_blocks::MOVING_PISTON
                .default_state()
                .set_value(&BlockStateProperties::FACING, direction)
                .set_value(
                    &BlockStateProperties::PISTON_TYPE,
                    if self.sticky {
                        PistonType::Sticky
                    } else {
                        PistonType::Normal
                    },
                );
            delete_after_move.retain(|(delete_pos, _)| *delete_pos != arm_pos);
            world.set_block(arm_pos, moving_state, UPDATE_MOVING_BLOCK);
            world.set_block_entity(Self::moving_block_entity(
                world,
                arm_pos,
                moving_state,
                head_state,
                direction,
                true,
                true,
            ));
        }

        // Java's HashMap table stays at 16 buckets for the at-most-twelve entries.
        delete_after_move.sort_by_key(|(pos, _)| java_hash::bucket(*pos));
        let air = vanilla_blocks::AIR.default_state();
        for &(pos, _) in &delete_after_move {
            world.set_block(pos, air, UPDATE_CLEARED_MOVED_BLOCK);
        }
        for &(pos, old_state) in &delete_after_move {
            BLOCK_BEHAVIORS
                .get_behavior(old_state.get_block())
                .update_indirect_neighbour_shapes(
                    old_state,
                    world,
                    pos,
                    UpdateFlags::UPDATE_CLIENTS,
                    512,
                );
            world.update_neighbour_shapes(air, pos, UpdateFlags::UPDATE_CLIENTS, 512);
            BLOCK_BEHAVIORS
                .get_behavior(air.get_block())
                .update_indirect_neighbour_shapes(
                    air,
                    world,
                    pos,
                    UpdateFlags::UPDATE_CLIENTS,
                    512,
                );
        }

        // The project intentionally omits experimental redstone orientations.
        let mut update_index = 0;
        for &pos in to_destroy.iter().rev() {
            let state = to_update[update_index];
            update_index += 1;
            BLOCK_BEHAVIORS
                .get_behavior(state.get_block())
                .affect_neighbors_after_removal(state, world, pos, false);
            BLOCK_BEHAVIORS
                .get_behavior(state.get_block())
                .update_indirect_neighbour_shapes(
                    state,
                    world,
                    pos,
                    UpdateFlags::UPDATE_CLIENTS,
                    512,
                );
            world.update_neighbors_at(pos, state.get_block());
        }
        for &pos in to_push.iter().rev() {
            let state = to_update[update_index];
            update_index += 1;
            world.update_neighbors_at(pos, state.get_block());
        }
        if extending {
            world.update_neighbors_at(arm_pos, &vanilla_blocks::PISTON_HEAD);
        }
        true
    }

    fn trigger_retraction(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        direction: Direction,
        event: i32,
        event_direction: i32,
    ) {
        Self::finish_moving_block_entity(world, pos.relative(direction));

        let piston_type = if self.sticky {
            PistonType::Sticky
        } else {
            PistonType::Normal
        };
        let moving_state = vanilla_blocks::MOVING_PISTON
            .default_state()
            .set_value(&BlockStateProperties::FACING, direction)
            .set_value(&BlockStateProperties::PISTON_TYPE, piston_type);
        world.set_block(pos, moving_state, UPDATE_RETRACT_BASE);
        let moved_state = self.block.default_state().set_value(
            &BlockStateProperties::FACING,
            Self::direction_from_legacy_id(event_direction),
        );
        world.set_block_entity(Self::moving_block_entity(
            world,
            pos,
            moving_state,
            moved_state,
            direction,
            false,
            true,
        ));
        world.update_neighbors_at(pos, moving_state.get_block());
        world.update_neighbour_shapes(moving_state, pos, UpdateFlags::UPDATE_CLIENTS, 512);

        let arm_pos = pos.relative(direction);
        if self.sticky {
            let two_pos = pos.relative_n(direction, 2);
            let two_state = world.get_block_state(two_pos);
            let piston_piece = if two_state.get_block() == &vanilla_blocks::MOVING_PISTON {
                let matches = world.get_block_entity(two_pos).is_some_and(|block_entity| {
                    block_entity
                        .downcast_ref::<PistonMovingBlockEntity>()
                        .is_some_and(|piston| {
                            piston.direction() == direction && piston.is_extending()
                        })
                });
                matches && Self::finish_moving_block_entity(world, two_pos)
            } else {
                false
            };

            if !piston_piece {
                let reaction = two_state.get_block().config.push_reaction;
                let piston = BLOCK_BEHAVIORS
                    .get_behavior(two_state.get_block())
                    .is_piston_base();
                if event != 1
                    || two_state.is_air()
                    || !Self::is_pushable(
                        two_state,
                        world.as_ref(),
                        two_pos,
                        direction.opposite(),
                        false,
                        direction,
                    )
                    || (reaction != PushReaction::Normal && !piston)
                {
                    world.remove_block(arm_pos, false);
                } else {
                    self.move_blocks(world, pos, direction, false);
                }
            }
        } else {
            world.remove_block(arm_pos, false);
        }

        world.play_sound(
            &sound_events::BLOCK_PISTON_CONTRACT,
            SoundSource::Blocks,
            pos,
            0.5,
            rand::random::<f32>().mul_add(0.15, 0.6),
            None,
        );
        world.game_event(
            &vanilla_game_events::BLOCK_DEACTIVATE,
            pos,
            &GameEventContext::new(None, Some(moving_state)),
        );
    }
}

impl BlockBehavior for PistonBaseBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(
            self.block
                .default_state()
                .set_value(
                    &BlockStateProperties::FACING,
                    context.get_nearest_looking_direction().opposite(),
                )
                .set_value(&BlockStateProperties::EXTENDED, false),
        )
    }

    fn set_placed_by(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source: &PlacementSource<'_>,
    ) {
        self.check_if_extend(world, pos, state);
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        if old_state.get_block() != state.get_block() && world.get_block_entity(pos).is_none() {
            self.check_if_extend(world, pos, state);
        }
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        self.check_if_extend(world, pos, state);
    }

    fn trigger_event(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        event: i32,
        event_direction: i32,
    ) -> bool {
        let direction = state.get_value(&BlockStateProperties::FACING);
        let extended_state = state.set_value(&BlockStateProperties::EXTENDED, true);
        let powered = Self::neighbor_signal(world, pos, direction);
        if powered && matches!(event, 1 | 2) {
            world.set_block(pos, extended_state, UpdateFlags::UPDATE_CLIENTS);
            return false;
        }
        if !powered && event == 0 {
            return false;
        }

        if event == 0 {
            if !self.move_blocks(world, pos, direction, true) {
                return false;
            }
            world.set_block(
                pos,
                extended_state,
                UpdateFlags::UPDATE_ALL | UpdateFlags::UPDATE_MOVE_BY_PISTON,
            );
            world.play_sound(
                &sound_events::BLOCK_PISTON_EXTEND,
                SoundSource::Blocks,
                pos,
                0.5,
                rand::random::<f32>().mul_add(0.25, 0.6),
                None,
            );
            world.game_event(
                &vanilla_game_events::BLOCK_ACTIVATE,
                pos,
                &GameEventContext::new(None, Some(extended_state)),
            );
        } else if matches!(event, 1 | 2) {
            self.trigger_retraction(world, pos, direction, event, event_direction);
        }
        true
    }

    fn is_piston_base(&self) -> bool {
        true
    }

    fn is_pathfindable(&self, _state: BlockStateId, _type: PathComputationType) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::blocks::properties::AttachFace;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_items;
    use steel_utils::{ChunkPos, types::InteractionHand};

    use super::*;
    use crate::behavior::{BlockHitResult, BlockLootContext, PlacementOrientation, init_behaviors};
    use crate::chunk::chunk_holder::ChunkHolder;
    use crate::test_support::{TestLevel, fresh_test_world, insert_ready_full_chunk};

    fn tick_block_entities(world: &Arc<World>, ticks: usize) {
        for _ in 0..ticks {
            world.block_entity_tickers().tick(world, true);
        }
    }

    fn powered_piston_world(
        key: &'static str,
        piston: BlockRef,
    ) -> (Arc<World>, Arc<ChunkHolder>, BlockPos, BlockPos) {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world(key);
        let piston_pos = BlockPos::new(8, 64, 8);
        let power_pos = piston_pos.west();
        let holder = insert_ready_full_chunk(&world, ChunkPos::from_block_pos(piston_pos));
        let piston_state = piston
            .default_state()
            .set_value(&BlockStateProperties::FACING, Direction::East)
            .set_value(&BlockStateProperties::EXTENDED, false);
        assert!(world.set_block(
            piston_pos.east(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        assert!(world.set_block(piston_pos, piston_state, UpdateFlags::UPDATE_NONE));
        assert!(world.set_block(
            power_pos,
            vanilla_blocks::REDSTONE_BLOCK.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        world.run_block_events();
        (world, holder, piston_pos, power_pos)
    }

    #[test]
    fn pushability_honors_bounds_reactions_and_block_entities() {
        init_test_registry();
        init_behaviors();
        let level = TestLevel::default();
        let pos = BlockPos::new(0, 64, 0);

        assert!(PistonBaseBlock::is_pushable(
            vanilla_blocks::STONE.default_state(),
            &level,
            pos,
            Direction::East,
            false,
            Direction::East,
        ));
        assert!(!PistonBaseBlock::is_pushable(
            vanilla_blocks::OBSIDIAN.default_state(),
            &level,
            pos,
            Direction::East,
            false,
            Direction::East,
        ));
        assert!(!PistonBaseBlock::is_pushable(
            vanilla_blocks::CHEST.default_state(),
            &level,
            pos,
            Direction::East,
            false,
            Direction::East,
        ));
    }

    #[test]
    fn placement_uses_player_look_direction_not_clicked_face() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("piston_look_placement");
        let support_pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(support_pos));
        assert!(world.set_block(
            support_pos,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        let mut stack = ItemStack::new(&vanilla_items::PISTON);
        let source = PlacementSource::direct(
            None,
            InteractionHand::MainHand,
            &mut stack,
            PlacementOrientation::Player {
                rotation: 0.0,
                pitch: 80.0,
            },
            false,
        );
        let context = BlockPlaceContext::new(
            &world,
            source,
            &BlockHitResult {
                location: DVec3::new(9.0, 64.5, 8.5),
                direction: Direction::East,
                block_pos: support_pos,
                miss: false,
                inside: false,
                world_border_hit: false,
            },
        );

        let state = PistonBaseBlock::new(&vanilla_blocks::PISTON, false)
            .get_state_for_placement(&context)
            .expect("piston placement should produce a state");
        assert_eq!(context.clicked_face(), Direction::East);
        assert_eq!(
            state.get_value(&BlockStateProperties::FACING),
            Direction::Up,
        );
    }

    #[test]
    fn moving_piston_delegates_loot_to_carried_state() {
        let (world, _holder, piston_pos, _power_pos) =
            powered_piston_world("moving_piston_loot", &vanilla_blocks::PISTON);
        let moving_pos = piston_pos.relative_n(Direction::East, 2);
        let moving_state = world.get_block_state(moving_pos);
        let tool = ItemStack::new(&vanilla_items::IRON_PICKAXE);
        let drops = BlockLootContext::new(&world, moving_pos)
            .with_tool(&tool)
            .get_drops(moving_state);

        assert_eq!(drops.len(), 1);
        assert_eq!(drops[0].item(), &*vanilla_items::COBBLESTONE);
    }

    #[test]
    fn normal_piston_extends_settles_and_retracts_without_pulling() {
        let (world, _holder, piston_pos, power_pos) =
            powered_piston_world("normal_piston_cycle", &vanilla_blocks::PISTON);
        assert!(
            world
                .get_block_state(piston_pos)
                .get_value(&BlockStateProperties::EXTENDED)
        );
        assert_eq!(
            world.get_block_state(piston_pos.east()).get_block(),
            &vanilla_blocks::MOVING_PISTON
        );
        assert_eq!(
            world
                .get_block_state(piston_pos.relative_n(Direction::East, 2))
                .get_block(),
            &vanilla_blocks::MOVING_PISTON
        );

        tick_block_entities(&world, 3);
        assert_eq!(
            world.get_block_state(piston_pos.east()).get_block(),
            &vanilla_blocks::PISTON_HEAD
        );
        assert_eq!(
            world
                .get_block_state(piston_pos.relative_n(Direction::East, 2))
                .get_block(),
            &vanilla_blocks::STONE
        );

        assert!(world.remove_block(power_pos, false));
        world.run_block_events();
        assert_eq!(
            world.get_block_state(piston_pos).get_block(),
            &vanilla_blocks::MOVING_PISTON
        );
        tick_block_entities(&world, 3);
        let base = world.get_block_state(piston_pos);
        assert_eq!(base.get_block(), &vanilla_blocks::PISTON);
        assert!(!base.get_value(&BlockStateProperties::EXTENDED));
        assert!(world.get_block_state(piston_pos.east()).is_air());
        assert_eq!(
            world
                .get_block_state(piston_pos.relative_n(Direction::East, 2))
                .get_block(),
            &vanilla_blocks::STONE
        );
    }

    #[test]
    fn retracting_piston_keeps_rear_face_attachments_supported() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("piston_rear_face_support");
        let piston_pos = BlockPos::new(8, 64, 8);
        let button_pos = piston_pos.west();
        let power_pos = piston_pos.north();
        let _holder = insert_ready_full_chunk(&world, ChunkPos::from_block_pos(piston_pos));
        let piston_state = vanilla_blocks::PISTON
            .default_state()
            .set_value(&BlockStateProperties::FACING, Direction::East)
            .set_value(&BlockStateProperties::EXTENDED, false);
        let button_state = vanilla_blocks::OAK_BUTTON
            .default_state()
            .set_value(&BlockStateProperties::ATTACH_FACE, AttachFace::Wall)
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, Direction::West);

        assert!(world.set_block(piston_pos, piston_state, UpdateFlags::UPDATE_NONE));
        assert!(world.set_block(button_pos, button_state, UpdateFlags::UPDATE_NONE));
        assert!(world.set_block(
            power_pos,
            vanilla_blocks::REDSTONE_BLOCK.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        world.run_block_events();
        assert_eq!(
            world.get_block_state(button_pos).get_block(),
            &vanilla_blocks::OAK_BUTTON
        );

        tick_block_entities(&world, 3);
        assert!(world.remove_block(power_pos, false));
        world.run_block_events();

        let moving = world.get_block_state(piston_pos);
        assert_eq!(moving.get_block(), &vanilla_blocks::MOVING_PISTON);
        assert!(world.is_face_sturdy(moving, piston_pos, Direction::West));
        assert!(!world.is_face_sturdy(moving, piston_pos, Direction::East));
        assert!(!world.is_face_sturdy(moving, piston_pos, Direction::Up));
        assert_eq!(
            world.get_block_state(button_pos).get_block(),
            &vanilla_blocks::OAK_BUTTON
        );

        tick_block_entities(&world, 3);
        assert_eq!(
            world.get_block_state(button_pos).get_block(),
            &vanilla_blocks::OAK_BUTTON
        );
    }

    #[test]
    fn sticky_piston_pulls_settled_normal_block() {
        let (world, _holder, piston_pos, power_pos) =
            powered_piston_world("sticky_piston_cycle", &vanilla_blocks::STICKY_PISTON);
        tick_block_entities(&world, 3);

        assert!(world.remove_block(power_pos, false));
        world.run_block_events();
        assert_eq!(
            world.get_block_state(piston_pos.east()).get_block(),
            &vanilla_blocks::MOVING_PISTON
        );
        tick_block_entities(&world, 3);

        let base = world.get_block_state(piston_pos);
        assert_eq!(base.get_block(), &vanilla_blocks::STICKY_PISTON);
        assert!(!base.get_value(&BlockStateProperties::EXTENDED));
        assert_eq!(
            world.get_block_state(piston_pos.east()).get_block(),
            &vanilla_blocks::STONE
        );
        assert!(
            world
                .get_block_state(piston_pos.relative_n(Direction::East, 2))
                .is_air()
        );
    }
}
