//! Chest block behavior (`ChestBlock`).

use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_macros::block_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, ChestType, Direction, EnumProperty,
};
use steel_registry::fluid::FluidState;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_entity_types;
use steel_registry::vanilla_custom_stats;
use steel_utils::{BlockPos, BlockStateId, Downcast as _, translations};

use crate::behavior::block::PickupResult;
use crate::behavior::block::{
    pickup_waterlogged_block, place_simple_waterlogged_liquid, schedule_water_tick_if_waterlogged,
};
use crate::behavior::{
    BLOCK_BEHAVIORS, BlockBehavior, BlockEntityCreation, BlockHitResult, BlockPlaceContext,
    InteractionResult, InventoryAccess,
};
use crate::block_entity::BLOCK_ENTITIES;
use crate::block_entity::SharedBlockEntity;
use crate::block_entity::entities::ChestBlockEntity;
use crate::entity::ai::path::PathComputationType;
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::inventory::menu::kinds::{chest as chest_menu, double_chest};
use crate::player::Player;
use crate::world::{LevelAccessor, LevelReader, ScheduledTickAccess, World};
use text_components::TextComponent;

/// Behavior for vanilla chest blocks.
#[block_behavior]
pub struct ChestBlock {
    block: BlockRef,
    #[json_arg(sound_events, json = "open_sound")]
    open_sound: SoundEventRef,
    #[json_arg(sound_events, json = "close_sound")]
    close_sound: SoundEventRef,
}

const HORIZONTAL_FACING: &EnumProperty<Direction> = &BlockStateProperties::HORIZONTAL_FACING;
const CHEST_TYPE: &EnumProperty<ChestType> = &BlockStateProperties::CHEST_TYPE;
const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;

const OPEN_SOUND_VOLUME: f32 = 0.5;
const OPEN_SOUND_PITCH_BASE: f32 = 0.9;
const OPEN_SOUND_PITCH_VARIANCE: f32 = 0.1;

enum CombinedChests {
    None,
    Single(SharedBlockEntity),
    Double {
        first: SharedBlockEntity,
        second: SharedBlockEntity,
    },
}

impl ChestBlock {
    /// Creates a new chest block behavior.
    #[must_use]
    pub const fn new(
        block: BlockRef,
        open_sound: SoundEventRef,
        close_sound: SoundEventRef,
    ) -> Self {
        Self {
            block,
            open_sound,
            close_sound,
        }
    }

    /// Vanilla `ChestBlock.getConnectedDirection`.
    #[must_use]
    pub fn get_connected_direction(state: BlockStateId) -> Direction {
        let facing = state.get_value(HORIZONTAL_FACING);
        if state.get_value(CHEST_TYPE) == ChestType::Left {
            facing.rotate_y_clockwise()
        } else {
            facing.rotate_y_counter_clockwise()
        }
    }

    fn chest_can_connect_to(&self, state: BlockStateId) -> bool {
        state.get_block() == self.block
    }

    fn candidate_partner_facing(
        &self,
        world: &dyn LevelReader,
        pos: BlockPos,
        neighbour_direction: Direction,
    ) -> Option<Direction> {
        let state = world.get_block_state(pos.relative(neighbour_direction));
        (self.chest_can_connect_to(state) && state.get_value(CHEST_TYPE) == ChestType::Single)
            .then(|| state.get_value(HORIZONTAL_FACING))
    }

    fn chest_type_for_placement(
        &self,
        world: &dyn LevelReader,
        pos: BlockPos,
        facing: Direction,
    ) -> ChestType {
        if Some(facing) == self.candidate_partner_facing(world, pos, facing.rotate_y_clockwise()) {
            ChestType::Left
        } else if Some(facing)
            == self.candidate_partner_facing(world, pos, facing.rotate_y_counter_clockwise())
        {
            ChestType::Right
        } else {
            ChestType::Single
        }
    }

    fn is_chest_blocked_at(world: &dyn LevelReader, pos: BlockPos) -> bool {
        // Vanilla also rejects a sitting cat on top of the chest. Steel has no Cat
        // entity yet, so only the redstone-conductor occupancy check is live.
        let above = pos.above();
        let above_state = world.get_block_state(above);
        BLOCK_BEHAVIORS
            .get_behavior(above_state.get_block())
            .is_redstone_conductor(above_state, world, above)
    }

    fn combine(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        ignore_being_blocked: bool,
    ) -> CombinedChests {
        let blocked =
            |check_pos| !ignore_being_blocked && Self::is_chest_blocked_at(world, check_pos);
        let Some(block_entity) = chest_entity_at(world, pos) else {
            return CombinedChests::None;
        };
        if blocked(pos) {
            return CombinedChests::None;
        }

        let chest_type = state.get_value(CHEST_TYPE);
        if chest_type == ChestType::Single {
            return CombinedChests::Single(block_entity);
        }

        let neighbor_pos = pos.relative(Self::get_connected_direction(state));
        let neighbor_state = world.get_block_state(neighbor_pos);
        if !self.chest_can_connect_to(neighbor_state) {
            return CombinedChests::Single(block_entity);
        }

        let neighbor_type = neighbor_state.get_value(CHEST_TYPE);
        if neighbor_type == ChestType::Single
            || neighbor_type == chest_type
            || neighbor_state.get_value(HORIZONTAL_FACING) != state.get_value(HORIZONTAL_FACING)
        {
            return CombinedChests::Single(block_entity);
        }
        if blocked(neighbor_pos) {
            return CombinedChests::None;
        }

        let Some(neighbor) = chest_entity_at(world, neighbor_pos) else {
            return CombinedChests::Single(block_entity);
        };

        let is_first = chest_type == ChestType::Right;
        if is_first {
            CombinedChests::Double {
                first: block_entity,
                second: neighbor,
            }
        } else {
            CombinedChests::Double {
                first: neighbor,
                second: block_entity,
            }
        }
    }

    fn play_opened_sound(&self, world: &World, pos: BlockPos, state: BlockStateId) {
        if state.get_value(CHEST_TYPE) == ChestType::Left {
            return;
        }

        let mut sound_pos = DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + 0.5,
            f64::from(pos.z()) + 0.5,
        );
        if state.get_value(CHEST_TYPE) == ChestType::Right {
            let connected = Self::get_connected_direction(state);
            let (dx, _, dz) = connected.offset();
            sound_pos.x += f64::from(dx) * 0.5;
            sound_pos.z += f64::from(dz) * 0.5;
        }

        let pitch = rand::random::<f32>() * OPEN_SOUND_PITCH_VARIANCE + OPEN_SOUND_PITCH_BASE;
        world.play_sound_at(
            self.open_sound,
            SoundSource::Blocks,
            sound_pos,
            OPEN_SOUND_VOLUME,
            pitch,
            None,
        );
        let _ = self.close_sound;
    }
}

fn chest_entity_at(world: &dyn LevelReader, pos: BlockPos) -> Option<SharedBlockEntity> {
    let entity = world.get_block_entity(pos)?;
    entity.downcast_ref::<ChestBlockEntity>()?;
    Some(entity)
}

fn as_chest(entity: &SharedBlockEntity) -> Option<&ChestBlockEntity> {
    entity.downcast_ref::<ChestBlockEntity>()
}

impl BlockBehavior for ChestBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let mut chest_type = ChestType::Single;
        let mut facing = context.horizontal_direction().opposite();
        let secondary_use = context.is_secondary_use_active();
        let clicked_face = context.clicked_face();

        if clicked_face.get_axis().is_horizontal() && secondary_use {
            let neighbour_facing = self.candidate_partner_facing(
                context.world,
                context.place_pos(),
                clicked_face.opposite(),
            );
            if let Some(neighbour_facing) = neighbour_facing
                && neighbour_facing.get_axis() != clicked_face.get_axis()
            {
                facing = neighbour_facing;
                chest_type = if facing.rotate_y_counter_clockwise() == clicked_face.opposite() {
                    ChestType::Right
                } else {
                    ChestType::Left
                };
            }
        }

        if chest_type == ChestType::Single && !secondary_use {
            chest_type = self.chest_type_for_placement(context.world, context.place_pos(), facing);
        }

        Some(
            self.block
                .default_state()
                .set_value(HORIZONTAL_FACING, facing)
                .set_value(CHEST_TYPE, chest_type)
                .set_value(WATERLOGGED, context.is_water_source()),
        )
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);

        if self.chest_can_connect_to(neighbor_state) && direction.get_axis().is_horizontal() {
            let neighbour_type = neighbor_state.get_value(CHEST_TYPE);
            if state.get_value(CHEST_TYPE) == ChestType::Single
                && neighbour_type != ChestType::Single
                && state.get_value(HORIZONTAL_FACING) == neighbor_state.get_value(HORIZONTAL_FACING)
                && Self::get_connected_direction(neighbor_state) == direction.opposite()
            {
                return state.set_value(CHEST_TYPE, neighbour_type.opposite());
            }
        } else if Self::get_connected_direction(state) == direction {
            return state.set_value(CHEST_TYPE, ChestType::Single);
        }

        state
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        match self.combine(state, world.as_ref(), pos, false) {
            CombinedChests::None => {}
            CombinedChests::Single(entity) => {
                let Some(chest) = as_chest(&entity) else {
                    return InteractionResult::Success;
                };
                if !chest.can_open(player) {
                    return InteractionResult::Success;
                }
                chest.unpack_loot_table(Some(player));
                let Some(container_ref) = entity.container_ref() else {
                    return InteractionResult::Success;
                };
                self.play_opened_sound(world, pos, state);
                let inventory = player.inventory.clone();
                player.open_menu(
                    TextComponent::translated(translations::CONTAINER_CHEST.msg()),
                    move |context| chest_menu(inventory, context.container_id, container_ref, 3),
                );
                player.award_custom_stat(&vanilla_custom_stats::OPEN_CHEST);
                // TODO: Anger nearby piglins (PiglinAi.angerNearbyPiglins).
                // TODO: ContainerOpenersCounter for close sounds, lid block events, and recheckOpen.
            }
            CombinedChests::Double { first, second } => {
                let Some(first_chest) = as_chest(&first) else {
                    return InteractionResult::Success;
                };
                let Some(second_chest) = as_chest(&second) else {
                    return InteractionResult::Success;
                };
                if !first_chest.can_open(player) || !second_chest.can_open(player) {
                    return InteractionResult::Success;
                }
                first_chest.unpack_loot_table(Some(player));
                second_chest.unpack_loot_table(Some(player));
                let Some(first_ref) = first.container_ref() else {
                    return InteractionResult::Success;
                };
                let Some(second_ref) = second.container_ref() else {
                    return InteractionResult::Success;
                };
                self.play_opened_sound(world, first.get_block_pos(), first.get_block_state());
                let inventory = player.inventory.clone();
                player.open_menu(
                    TextComponent::translated(translations::CONTAINER_CHEST_DOUBLE.msg()),
                    move |context| {
                        double_chest(inventory, context.container_id, first_ref, second_ref)
                    },
                );
                player.award_custom_stat(&vanilla_custom_stats::OPEN_CHEST);
                // TODO: Anger nearby piglins (PiglinAi.angerNearbyPiglins).
                // TODO: ContainerOpenersCounter for close sounds, lid block events, and recheckOpen.
            }
        }

        InteractionResult::Success
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::CHEST,
            level,
            pos,
            state,
        ))
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        match self.combine(state, world, pos, false) {
            CombinedChests::None => 0,
            CombinedChests::Single(entity) => {
                if let Some(chest) = as_chest(&entity) {
                    chest.unpack_loot_table(None);
                }
                analog_signal(&[&entity])
            }
            CombinedChests::Double { first, second } => {
                if let Some(chest) = as_chest(&first) {
                    chest.unpack_loot_table(None);
                }
                if let Some(chest) = as_chest(&second) {
                    chest.unpack_loot_table(None);
                }
                analog_signal(&[&first, &second])
            }
        }
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _moved_by_piston: bool,
    ) {
        world.update_neighbor_for_output_signal(pos, state.get_block());
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }

    fn pickup_block(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        player: Option<&Player>,
    ) -> Option<PickupResult> {
        pickup_waterlogged_block(self, world, pos, state, player)
    }

    fn place_liquid(
        &self,
        level: &dyn LevelAccessor,
        pos: BlockPos,
        state: BlockStateId,
        fluid_state: FluidState,
    ) -> bool {
        place_simple_waterlogged_liquid(level, pos, state, fluid_state)
    }
}

fn analog_signal(entities: &[&SharedBlockEntity]) -> i32 {
    let refs: Vec<ContainerRef> = entities
        .iter()
        .filter_map(|entity| entity.container_ref())
        .collect();
    if refs.is_empty() {
        return 0;
    }
    let guard = ContainerLockGuard::lock_all(&refs);
    analog_signal_from_locked(&guard, &refs)
}

fn analog_signal_from_locked(guard: &ContainerLockGuard, refs: &[ContainerRef]) -> i32 {
    let mut size = 0usize;
    let mut total_percent = 0.0f32;
    for container_ref in refs {
        let Some(container) = guard.get(container_ref.container_id()) else {
            continue;
        };
        size += container.get_container_size();
        for slot in 0..container.get_container_size() {
            let item = container.get_item(slot);
            if !item.is_empty() {
                let max_stack = container.get_max_stack_size_for_item(item);
                total_percent += item.count() as f32 / max_stack as f32;
            }
        }
    }
    if size == 0 {
        return 0;
    }
    total_percent /= size as f32;
    (total_percent * 14.0).floor() as i32 + i32::from(total_percent > 0.0)
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_registry::init_vanilla_registry;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::sound_events;
    use steel_registry::vanilla_blocks;
    use steel_registry::vanilla_items;
    use steel_utils::types::{InteractionHand, UpdateFlags};

    use super::*;
    use crate::behavior::{PlacementOrientation, PlacementSource, init_behaviors};
    use crate::bootstrap::init_globals_once;
    use crate::entity::ai::path::PathComputationType;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk, test_world};
    use steel_utils::ChunkPos;

    fn chest_behavior() -> ChestBlock {
        ChestBlock::new(
            &vanilla_blocks::CHEST,
            &sound_events::BLOCK_CHEST_OPEN,
            &sound_events::BLOCK_CHEST_CLOSE,
        )
    }

    fn place_context<'a>(
        world: &'a Arc<World>,
        pos: BlockPos,
        facing: Direction,
        secondary_use: bool,
        stack: &'a mut ItemStack,
    ) -> BlockPlaceContext<'a> {
        let (x, y, z) = pos.get_center();
        let hit_result = BlockHitResult {
            location: DVec3::new(x, y - 0.5, z),
            direction: Direction::Up,
            block_pos: pos.below(),
            miss: false,
            inside: false,
            world_border_hit: false,
        };
        let source = PlacementSource::direct(
            None,
            InteractionHand::MainHand,
            stack,
            PlacementOrientation::Directional { direction: facing },
            secondary_use,
        );
        BlockPlaceContext::new(world, source, &hit_result)
    }

    #[test]
    fn connected_direction_matches_vanilla_left_and_right() {
        init_vanilla_registry();
        let north_left = vanilla_blocks::CHEST
            .default_state()
            .set_value(HORIZONTAL_FACING, Direction::North)
            .set_value(CHEST_TYPE, ChestType::Left);
        let north_right = vanilla_blocks::CHEST
            .default_state()
            .set_value(HORIZONTAL_FACING, Direction::North)
            .set_value(CHEST_TYPE, ChestType::Right);

        assert_eq!(
            ChestBlock::get_connected_direction(north_left),
            Direction::East
        );
        assert_eq!(
            ChestBlock::get_connected_direction(north_right),
            Direction::West
        );
    }

    #[test]
    fn placement_faces_the_player_and_stays_single_without_a_partner() {
        init_globals_once();
        let world = fresh_test_world("chest_single_placement");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let behavior = chest_behavior();
        let pos = BlockPos::new(1, 64, 1);
        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        let mut stack = ItemStack::new(&vanilla_items::CHEST);
        let context = place_context(&world, pos, Direction::South, false, &mut stack);
        let placed = behavior
            .get_state_for_placement(&context)
            .expect("chest always has a placement state");

        assert_eq!(placed.get_value(HORIZONTAL_FACING), Direction::North);
        assert_eq!(placed.get_value(CHEST_TYPE), ChestType::Single);
        assert!(!placed.get_value(WATERLOGGED));
    }

    #[test]
    fn placement_connects_to_an_adjacent_single_chest() {
        init_globals_once();
        let world = fresh_test_world("chest_double_placement");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let behavior = chest_behavior();
        let partner = BlockPos::new(1, 64, 1);
        let pos = BlockPos::new(2, 64, 1);
        assert!(world.set_block(
            pos.below(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        assert!(
            world.set_block(
                partner,
                vanilla_blocks::CHEST
                    .default_state()
                    .set_value(HORIZONTAL_FACING, Direction::North)
                    .set_value(CHEST_TYPE, ChestType::Single),
                UpdateFlags::UPDATE_ALL,
            )
        );

        let mut stack = ItemStack::new(&vanilla_items::CHEST);
        let context = place_context(&world, pos, Direction::South, false, &mut stack);
        let placed = behavior
            .get_state_for_placement(&context)
            .expect("chest always has a placement state");

        assert_eq!(placed.get_value(HORIZONTAL_FACING), Direction::North);
        assert_eq!(placed.get_value(CHEST_TYPE), ChestType::Right);
    }

    #[test]
    fn update_shape_forms_and_breaks_double_chests() {
        init_vanilla_registry();
        init_behaviors();
        let behavior = chest_behavior();
        let single = vanilla_blocks::CHEST
            .default_state()
            .set_value(HORIZONTAL_FACING, Direction::North)
            .set_value(CHEST_TYPE, ChestType::Single);
        let right = single.set_value(CHEST_TYPE, ChestType::Right);
        let world = test_world();

        let connected = behavior.update_shape(
            single,
            world,
            BlockPos::new(1, 64, 1),
            Direction::East,
            BlockPos::new(2, 64, 1),
            right,
        );
        assert_eq!(connected.get_value(CHEST_TYPE), ChestType::Left);

        let disconnected = behavior.update_shape(
            connected,
            world,
            BlockPos::new(1, 64, 1),
            Direction::East,
            BlockPos::new(2, 64, 1),
            vanilla_blocks::AIR.default_state(),
        );
        assert_eq!(disconnected.get_value(CHEST_TYPE), ChestType::Single);
    }

    #[test]
    fn is_pathfindable_returns_false_for_all_types() {
        init_vanilla_registry();
        let block = chest_behavior();
        let state = vanilla_blocks::CHEST.default_state();
        assert!(!block.is_pathfindable(state, PathComputationType::Land));
        assert!(!block.is_pathfindable(state, PathComputationType::Water));
        assert!(!block.is_pathfindable(state, PathComputationType::Air));
    }
}
