//! Chest block behavior implementation.
//!
//! Chests pair with a horizontal neighbour into a double chest, open a 3- or
//! 6-row menu, and track viewers so the lid animates and the open sound plays.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, ChestType, Direction, EnumProperty,
};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::stat::custom::CustomStatRef;
use steel_registry::{vanilla_block_entity_types, vanilla_custom_stats};
use steel_utils::{BlockPos, BlockStateId, Downcast as _, translations};
use text_components::TextComponent;

use crate::behavior::InventoryAccess;
use crate::behavior::block::{
    BlockBehavior, BlockEntityCreation, schedule_water_tick_if_waterlogged,
};
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::block_entity::entities::ChestBlockEntity;
use crate::block_entity::{BLOCK_ENTITIES, SharedBlockEntity};
use crate::entity::LivingEntity as _;
use crate::entity::ai::path::PathComputationType;
use crate::inventory::container::{Container, calculate_redstone_signal_from_containers};
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::inventory::menu::kinds::chest_for_block_entities;
use crate::player::Player;
use crate::world::{LevelReader, ScheduledTickAccess, World, is_redstone_conductor};

const FACING: &EnumProperty<Direction> = &BlockStateProperties::HORIZONTAL_FACING;
const TYPE: &EnumProperty<ChestType> = &BlockStateProperties::CHEST_TYPE;
const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;

/// The open and close sounds a chest-like block plays through its block entity.
///
/// Mirrors Vanilla `ChestBlock.getOpenChestSound`/`getCloseChestSound`, which
/// `ChestBlockEntity` reaches through an `instanceof ChestBlock` check.
pub trait ChestBehavior {
    /// Vanilla `ChestBlock.getOpenChestSound`.
    fn open_sound(&self) -> SoundEventRef;

    /// Vanilla `ChestBlock.getCloseChestSound`.
    fn close_sound(&self) -> SoundEventRef;
}

/// Vanilla `ChestBlock.getConnectedDirection`.
#[must_use]
pub fn connected_chest_direction(state: BlockStateId) -> Direction {
    let facing = state.get_value(FACING);
    if state.get_value(TYPE) == ChestType::Left {
        facing.rotate_y_clockwise()
    } else {
        facing.rotate_y_counter_clockwise()
    }
}

/// Vanilla `ChestBlock.getConnectedBlockPos`.
#[must_use]
pub fn connected_chest_pos(pos: BlockPos, state: BlockStateId) -> Option<BlockPos> {
    (state.try_get_value(TYPE)? != ChestType::Single)
        .then(|| connected_chest_direction(state).relative(pos))
}

/// Vanilla `DoubleBlockCombiner.NeighborCombineResult` for chests.
pub enum ChestCombineResult {
    /// Vanilla `acceptNone`: no reachable block entity, or the chest is blocked.
    None,
    /// Vanilla `acceptSingle`.
    Single(SharedBlockEntity),
    /// Vanilla `acceptDouble`, in vanilla's first/second slot order.
    Double(SharedBlockEntity, SharedBlockEntity),
}

impl ChestCombineResult {
    /// The block entities backing this result, in menu slot order.
    #[must_use]
    pub fn block_entities(&self) -> Vec<SharedBlockEntity> {
        match self {
            Self::None => Vec::new(),
            Self::Single(single) => vec![Arc::clone(single)],
            Self::Double(first, second) => vec![Arc::clone(first), Arc::clone(second)],
        }
    }

    fn container_refs(&self) -> Vec<ContainerRef> {
        self.block_entities()
            .into_iter()
            .filter_map(ContainerRef::from_block_entity)
            .collect()
    }
}

/// Behavior for chest blocks.
#[block_behavior]
pub struct ChestBlock {
    block: BlockRef,
    #[json_arg(sound_events, json = "open_sound")]
    open_sound: SoundEventRef,
    #[json_arg(sound_events, json = "close_sound")]
    close_sound: SoundEventRef,
    /// Vanilla `ChestBlock.getOpenChestStat`, overridden by trapped chests.
    open_chest_stat: CustomStatRef,
}

impl ChestBlock {
    /// Creates a new chest block behavior.
    #[must_use]
    pub const fn new(
        block: BlockRef,
        open_sound: SoundEventRef,
        close_sound: SoundEventRef,
    ) -> Self {
        Self::with_open_chest_stat(
            block,
            open_sound,
            close_sound,
            &vanilla_custom_stats::OPEN_CHEST,
        )
    }

    /// Creates a chest behavior that awards a different open statistic.
    #[must_use]
    pub(super) const fn with_open_chest_stat(
        block: BlockRef,
        open_sound: SoundEventRef,
        close_sound: SoundEventRef,
        open_chest_stat: CustomStatRef,
    ) -> Self {
        Self {
            block,
            open_sound,
            close_sound,
            open_chest_stat,
        }
    }

    /// Vanilla `ChestBlock.chestCanConnectTo`.
    fn chest_can_connect_to(&self, state: BlockStateId) -> bool {
        state.get_block() == self.block
    }

    /// Vanilla `ChestBlock.candidatePartnerFacing`.
    fn candidate_partner_facing(
        &self,
        level: &dyn LevelReader,
        pos: BlockPos,
        neighbour_direction: Direction,
    ) -> Option<Direction> {
        let state = level.get_block_state(neighbour_direction.relative(pos));
        (self.chest_can_connect_to(state) && state.get_value(TYPE) == ChestType::Single)
            .then(|| state.get_value(FACING))
    }

    /// Vanilla `ChestBlock.getChestType`.
    fn chest_type_for_placement(
        &self,
        level: &dyn LevelReader,
        pos: BlockPos,
        facing: Direction,
    ) -> ChestType {
        if self.candidate_partner_facing(level, pos, facing.rotate_y_clockwise()) == Some(facing) {
            ChestType::Left
        } else if self.candidate_partner_facing(level, pos, facing.rotate_y_counter_clockwise())
            == Some(facing)
        {
            ChestType::Right
        } else {
            ChestType::Single
        }
    }

    /// Vanilla `ChestBlock.isChestBlockedAt`.
    ///
    /// Vanilla also refuses to open under a sitting cat. Steel has no cat
    /// entity yet, so only the conducting-block check applies.
    fn is_chest_blocked_at(level: &dyn LevelReader, pos: BlockPos) -> bool {
        let above = Direction::Up.relative(pos);
        is_redstone_conductor(level, level.get_block_state(above), above)
    }

    /// Vanilla `ChestBlock.combine`.
    ///
    /// Vanilla resolves the partner through the block's own block-entity type;
    /// comparing the neighbour's block is equivalent and needs no instance.
    #[must_use]
    pub fn combine(
        state: BlockStateId,
        level: &dyn LevelReader,
        pos: BlockPos,
        ignore_being_blocked: bool,
    ) -> ChestCombineResult {
        let blocked =
            |pos: BlockPos| !ignore_being_blocked && Self::is_chest_blocked_at(level, pos);

        let Some(block_entity) = level.get_block_entity(pos) else {
            return ChestCombineResult::None;
        };
        if blocked(pos) {
            return ChestCombineResult::None;
        }

        let chest_type = state.get_value(TYPE);
        if chest_type == ChestType::Single {
            return ChestCombineResult::Single(block_entity);
        }

        let neighbour_pos = connected_chest_direction(state).relative(pos);
        let neighbour_state = level.get_block_state(neighbour_pos);
        if neighbour_state.get_block() != state.get_block() {
            return ChestCombineResult::Single(block_entity);
        }

        let neighbour_type = neighbour_state.get_value(TYPE);
        if neighbour_type == ChestType::Single
            || neighbour_type == chest_type
            || neighbour_state.get_value(FACING) != state.get_value(FACING)
        {
            return ChestCombineResult::Single(block_entity);
        }
        if blocked(neighbour_pos) {
            return ChestCombineResult::None;
        }
        let Some(neighbour) = level.get_block_entity(neighbour_pos) else {
            return ChestCombineResult::Single(block_entity);
        };

        // Vanilla maps RIGHT to the first and LEFT to the second menu section.
        if chest_type == ChestType::Right {
            ChestCombineResult::Double(block_entity, neighbour)
        } else {
            ChestCombineResult::Double(neighbour, block_entity)
        }
    }
}

impl ChestBehavior for ChestBlock {
    fn open_sound(&self) -> SoundEventRef {
        self.open_sound
    }

    fn close_sound(&self) -> SoundEventRef {
        self.close_sound
    }
}

impl BlockBehavior for ChestBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let level = context.world.as_ref();
        let pos = context.place_pos();
        let secondary_use = context.is_secondary_use_active();
        let clicked_face = context.clicked_face();

        let mut facing = context.horizontal_direction().opposite();
        let mut chest_type = ChestType::Single;

        if clicked_face.is_horizontal() && secondary_use {
            let partner_facing = self.candidate_partner_facing(level, pos, clicked_face.opposite());
            if let Some(partner_facing) = partner_facing
                && partner_facing.axis() != clicked_face.axis()
            {
                facing = partner_facing;
                chest_type = if facing.rotate_y_counter_clockwise() == clicked_face.opposite() {
                    ChestType::Right
                } else {
                    ChestType::Left
                };
            }
        }

        if chest_type == ChestType::Single && !secondary_use {
            chest_type = self.chest_type_for_placement(level, pos, facing);
        }

        Some(
            self.block
                .default_state()
                .set_value(FACING, facing)
                .set_value(TYPE, chest_type)
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

        if self.chest_can_connect_to(neighbor_state) && direction.is_horizontal() {
            let neighbour_type = neighbor_state.get_value(TYPE);
            if state.get_value(TYPE) == ChestType::Single
                && neighbour_type != ChestType::Single
                && state.get_value(FACING) == neighbor_state.get_value(FACING)
                && connected_chest_direction(neighbor_state) == direction.opposite()
            {
                return state.set_value(TYPE, neighbour_type.get_opposite());
            }
        } else if connected_chest_direction(state) == direction {
            return state.set_value(TYPE, ChestType::Single);
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
        let combined = Self::combine(state, world.as_ref(), pos, false);
        let block_entities = combined.block_entities();
        if block_entities.is_empty() {
            // Vanilla's blocked chest has no menu provider and opens nothing.
            return InteractionResult::Success;
        }

        // Vanilla unpacks each half's worldgen loot table before building the menu.
        for block_entity in &block_entities {
            if let Some(chest) = block_entity.downcast_ref::<ChestBlockEntity>() {
                chest.unpack_loot_table(player.get_luck());
            }
        }

        let title = if block_entities.len() > 1 {
            translations::CONTAINER_CHEST_DOUBLE.msg()
        } else {
            translations::CONTAINER_CHEST.msg()
        };
        let inventory = player.inventory.clone();
        player.open_menu(TextComponent::translated(title), move |context| {
            chest_for_block_entities(inventory, context.container_id, block_entities)
        });
        player.award_custom_stat(self.open_chest_stat);

        // TODO: Anger nearby piglins once Steel implements `PiglinAi.angerNearbyPiglins`.

        InteractionResult::Success
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

    fn tick(&self, _state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return;
        };
        let Some(chest) = block_entity.downcast_ref::<ChestBlockEntity>() else {
            return;
        };
        chest.recheck_open(world);
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

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
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
        let combined = Self::combine(state, world, pos, false);
        // Vanilla's comparator reads through `RandomizableContainerBlockEntity.getItem`,
        // which unpacks a pending loot table first.
        for block_entity in combined.block_entities() {
            if let Some(chest) = block_entity.downcast_ref::<ChestBlockEntity>() {
                chest.unpack_loot_table(0.0);
            }
        }

        let containers = combined.container_refs();
        if containers.is_empty() {
            return 0;
        }

        let guard = ContainerLockGuard::lock_all(&containers);
        let locked: Vec<&dyn Container> = containers
            .iter()
            .filter_map(|container| guard.get(container.container_id()))
            .collect();
        calculate_redstone_signal_from_containers(&locked)
    }

    fn as_chest(&self) -> Option<&dyn ChestBehavior> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, sound_events, vanilla_blocks};

    use super::*;
    use crate::test_support::TestLevel;

    fn chest_block() -> ChestBlock {
        ChestBlock::new(
            &vanilla_blocks::CHEST,
            &sound_events::BLOCK_CHEST_OPEN,
            &sound_events::BLOCK_CHEST_CLOSE,
        )
    }

    fn chest_state(facing: Direction, chest_type: ChestType) -> BlockStateId {
        vanilla_blocks::CHEST
            .default_state()
            .set_value(FACING, facing)
            .set_value(TYPE, chest_type)
    }

    /// Vanilla checks the clockwise neighbour first, so an otherwise symmetric
    /// pair of partners resolves to different halves depending on the side.
    #[test]
    fn placement_half_follows_the_side_the_partner_sits_on() {
        init_vanilla_registry();
        let block = chest_block();
        let pos = BlockPos::new(4, 70, 9);
        let partner = chest_state(Direction::North, ChestType::Single);

        let clockwise = TestLevel::default()
            .with_block(Direction::North.rotate_y_clockwise().relative(pos), partner);
        assert_eq!(
            block.chest_type_for_placement(&clockwise, pos, Direction::North),
            ChestType::Left
        );

        let counter_clockwise = TestLevel::default().with_block(
            Direction::North.rotate_y_counter_clockwise().relative(pos),
            partner,
        );
        assert_eq!(
            block.chest_type_for_placement(&counter_clockwise, pos, Direction::North),
            ChestType::Right
        );
    }

    #[test]
    fn placement_ignores_a_neighbour_that_faces_elsewhere() {
        init_vanilla_registry();
        let block = chest_block();
        let pos = BlockPos::new(4, 70, 9);
        let level = TestLevel::default().with_block(
            Direction::North.rotate_y_clockwise().relative(pos),
            chest_state(Direction::South, ChestType::Single),
        );

        assert_eq!(
            block.chest_type_for_placement(&level, pos, Direction::North),
            ChestType::Single
        );
    }

    /// A single chest only joins a neighbour whose free side points back at it.
    #[test]
    fn shape_update_connects_only_to_a_neighbour_pointing_back() {
        init_vanilla_registry();
        let block = chest_block();
        let pos = BlockPos::new(4, 70, 9);
        let single = chest_state(Direction::North, ChestType::Single);
        let level = TestLevel::default();

        let pointing_back = chest_state(Direction::North, ChestType::Right);
        assert_eq!(
            block.update_shape(
                single,
                &level,
                pos,
                Direction::East,
                Direction::East.relative(pos),
                pointing_back,
            ),
            chest_state(Direction::North, ChestType::Left)
        );

        let pointing_away = chest_state(Direction::North, ChestType::Left);
        assert_eq!(
            block.update_shape(
                single,
                &level,
                pos,
                Direction::East,
                Direction::East.relative(pos),
                pointing_away,
            ),
            single
        );
    }

    #[test]
    fn shape_update_splits_when_the_partner_disappears() {
        init_vanilla_registry();
        let block = chest_block();
        let pos = BlockPos::new(4, 70, 9);
        let left = chest_state(Direction::North, ChestType::Left);
        let level = TestLevel::default();

        assert_eq!(
            block.update_shape(
                left,
                &level,
                pos,
                connected_chest_direction(left),
                connected_chest_direction(left).relative(pos),
                vanilla_blocks::AIR.default_state(),
            ),
            chest_state(Direction::North, ChestType::Single)
        );
    }
}
