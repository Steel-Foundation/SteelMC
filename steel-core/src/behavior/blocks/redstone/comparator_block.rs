//! Vanilla redstone comparator behavior.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, ComparatorMode, Direction};
use steel_registry::{REGISTRY, sound_events, vanilla_blocks};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Downcast as _, WorldAabb};

use super::diode_block::DiodeBlock;
use crate::behavior::{
    BLOCK_BEHAVIORS, BlockBehavior, BlockEntityCreation, BlockHitResult, BlockPlaceContext,
    InteractionResult, InventoryAccess, PlacementSource,
};
use crate::block_entity::entities::ComparatorBlockEntity;
use crate::entity::{Entity, ItemFrame};
use crate::player::Player;
use crate::world::tick_scheduler::TickPriority;
use crate::world::{
    LevelReader, ScheduledTickAccess, SignalQueryContext, World, is_redstone_conductor,
};

const DELAY: i32 = 2;

/// Vanilla `ComparatorBlock`, including persisted output and item-frame input.
#[block_behavior]
pub struct ComparatorBlock {
    diode: DiodeBlock,
}

impl ComparatorBlock {
    /// Creates comparator behavior for `block`.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            diode: DiodeBlock::new(block),
        }
    }

    fn output_signal(level: &dyn LevelReader, pos: BlockPos) -> i32 {
        let Some(block_entity) = level.get_block_entity(pos) else {
            return 0;
        };
        block_entity
            .downcast_ref::<ComparatorBlockEntity>()
            .map_or(0, ComparatorBlockEntity::output_signal)
    }

    fn set_output_signal(world: &Arc<World>, pos: BlockPos, output_signal: i32) -> i32 {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return 0;
        };
        let Some(comparator) = block_entity.downcast_ref::<ComparatorBlockEntity>() else {
            return 0;
        };
        let old_output = comparator.output_signal();
        comparator.set_output_signal(output_signal);
        old_output
    }

    fn item_frame_signal(world: &World, direction: Direction, pos: BlockPos) -> Option<i32> {
        let bounds = WorldAabb::new(
            f64::from(pos.x()),
            f64::from(pos.y()),
            f64::from(pos.z()),
            f64::from(pos.x() + 1),
            f64::from(pos.y() + 1),
            f64::from(pos.z() + 1),
        );
        let frames = world.get_entities_in_aabb_matching(&bounds, |entity| {
            entity
                .as_item_frame()
                .is_some_and(|frame| frame.direction() == direction)
        });
        if frames.len() != 1 {
            return None;
        }
        frames[0]
            .as_ref()
            .as_item_frame()
            .map(ItemFrame::analog_output)
    }

    fn get_input_signal(world: &Arc<World>, pos: BlockPos, state: BlockStateId) -> i32 {
        let mut result = DiodeBlock::get_input_signal(world.as_ref(), pos, state);
        let direction = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        let mut target_pos = pos.relative(direction);
        let mut target_state = world.get_block_state(target_pos);
        let mut target_behavior = BLOCK_BEHAVIORS.get_behavior(target_state.get_block());
        if target_behavior.has_analog_output_signal(target_state) {
            return target_behavior.get_analog_output_signal(
                target_state,
                world.as_ref(),
                target_pos,
                direction.opposite(),
            );
        }

        if result >= 15 || !is_redstone_conductor(world.as_ref(), target_state, target_pos) {
            return result;
        }

        target_pos = target_pos.relative(direction);
        target_state = world.get_block_state(target_pos);
        target_behavior = BLOCK_BEHAVIORS.get_behavior(target_state.get_block());
        let frame_signal = Self::item_frame_signal(world.as_ref(), direction, target_pos);
        let block_signal = target_behavior
            .has_analog_output_signal(target_state)
            .then(|| {
                target_behavior.get_analog_output_signal(
                    target_state,
                    world.as_ref(),
                    target_pos,
                    direction.opposite(),
                )
            });
        if let Some(analog_signal) = match (frame_signal, block_signal) {
            (Some(frame), Some(block)) => Some(frame.max(block)),
            (Some(frame), None) => Some(frame),
            (None, Some(block)) => Some(block),
            (None, None) => None,
        } {
            result = analog_signal;
        }
        result
    }

    const fn calculate_output_signal(input: i32, alternate: i32, mode: ComparatorMode) -> i32 {
        if input == 0 || alternate > input {
            return 0;
        }
        match mode {
            ComparatorMode::Compare => input,
            ComparatorMode::Subtract => input - alternate,
        }
    }

    fn calculate_output(world: &Arc<World>, pos: BlockPos, state: BlockStateId) -> i32 {
        let input = Self::get_input_signal(world, pos, state);
        let alternate = DiodeBlock::get_alternate_signal(world.as_ref(), pos, state, false);
        Self::calculate_output_signal(
            input,
            alternate,
            state.get_value(&BlockStateProperties::MODE_COMPARATOR),
        )
    }

    const fn should_turn_on_from_signals(input: i32, alternate: i32, mode: ComparatorMode) -> bool {
        input != 0
            && (input > alternate
                || (input == alternate && matches!(mode, ComparatorMode::Compare)))
    }

    fn should_turn_on(world: &Arc<World>, pos: BlockPos, state: BlockStateId) -> bool {
        let input = Self::get_input_signal(world, pos, state);
        let alternate = DiodeBlock::get_alternate_signal(world.as_ref(), pos, state, false);
        Self::should_turn_on_from_signals(
            input,
            alternate,
            state.get_value(&BlockStateProperties::MODE_COMPARATOR),
        )
    }

    fn check_tick_on_neighbor(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        if world.will_tick_block_this_tick(pos, self.diode.block) {
            return;
        }
        let output = Self::calculate_output(world, pos, state);
        if output == Self::output_signal(world.as_ref(), pos)
            && state.get_value(&BlockStateProperties::POWERED)
                == Self::should_turn_on(world, pos, state)
        {
            return;
        }
        let priority = if DiodeBlock::should_prioritize(world.as_ref(), pos, state) {
            TickPriority::High
        } else {
            TickPriority::Normal
        };
        world.schedule_block_tick(pos, self.diode.block, DELAY, priority);
    }

    fn refresh_output_state(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        let output = Self::calculate_output(world, pos, state);
        let old_output = Self::set_output_signal(world, pos, output);
        if old_output == output
            && state.get_value(&BlockStateProperties::MODE_COMPARATOR) != ComparatorMode::Compare
        {
            return;
        }

        let should_turn_on = Self::should_turn_on(world, pos, state);
        let powered = state.get_value(&BlockStateProperties::POWERED);
        if powered != should_turn_on {
            world.set_block(
                pos,
                state.set_value(&BlockStateProperties::POWERED, should_turn_on),
                UpdateFlags::UPDATE_CLIENTS,
            );
        }
        self.diode.update_neighbors_in_front(world, pos, state);
    }
}

impl BlockBehavior for ComparatorBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        DiodeBlock::can_survive(world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.diode.state_for_placement(context))
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        _pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if direction == Direction::Down
            && !DiodeBlock::can_survive_on(world, neighbor_pos, neighbor_state)
        {
            REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR)
        } else {
            state
        }
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
        if !player.abilities.lock().may_build {
            return InteractionResult::Pass;
        }

        let mode = state.get_value(&BlockStateProperties::MODE_COMPARATOR);
        let next_mode = if mode == ComparatorMode::Compare {
            ComparatorMode::Subtract
        } else {
            ComparatorMode::Compare
        };
        let pitch = if next_mode == ComparatorMode::Subtract {
            0.55
        } else {
            0.5
        };
        let next_state = state.set_value(&BlockStateProperties::MODE_COMPARATOR, next_mode);
        world.play_block_sound(
            &sound_events::BLOCK_COMPARATOR_CLICK,
            pos,
            0.3,
            pitch,
            Some(player.id()),
        );
        world.set_block(pos, next_state, UpdateFlags::UPDATE_CLIENTS);
        if world.get_block_state(pos).get_block() == self.diode.block {
            self.refresh_output_state(world, pos, next_state);
        }
        InteractionResult::Success
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        self.diode.handle_neighbor_changed(state, world, pos, || {
            self.check_tick_on_neighbor(world, pos, state);
        });
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.refresh_output_state(world, pos, state);
    }

    fn set_placed_by(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source: &PlacementSource<'_>,
    ) {
        self.diode
            .set_placed_by(world, pos, Self::should_turn_on(world, pos, state));
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        self.diode.on_place(state, world, pos);
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        self.diode
            .affect_neighbors_after_removal(state, world, pos, moved_by_piston);
    }

    fn is_signal_source(&self, _state: BlockStateId, _context: SignalQueryContext) -> bool {
        true
    }

    fn is_diode(&self) -> bool {
        true
    }

    fn get_own_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        _context: SignalQueryContext,
    ) -> i32 {
        DiodeBlock::own_signal(state, Self::output_signal(world, pos))
    }

    fn get_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
        _context: SignalQueryContext,
    ) -> i32 {
        DiodeBlock::signal(state, direction, Self::output_signal(world, pos))
    }

    fn get_direct_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
        context: SignalQueryContext,
    ) -> i32 {
        self.get_signal(state, world, pos, direction, context)
    }

    fn trigger_event(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        param_a: i32,
        param_b: i32,
    ) -> bool {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return false;
        };
        block_entity.trigger_event(param_a, param_b)
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::Created(Arc::new(ComparatorBlockEntity::new(level, pos, state)))
    }

    // `animateTick` emits client-local dust particles only.
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_registry::entity_type::EntityTypeRef;
    use steel_registry::test_support::init_test_registry;
    use steel_registry::{vanilla_blocks, vanilla_entities};
    use steel_utils::ChunkPos;

    use super::*;
    use crate::entity::{EntityBase, SharedEntity};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    struct TestItemFrame {
        base: EntityBase,
        direction: Direction,
        analog_output: i32,
    }

    crate::entity::impl_test_downcast_type!(TestItemFrame);

    impl Entity for TestItemFrame {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::ITEM_FRAME
        }
    }

    impl ItemFrame for TestItemFrame {
        fn direction(&self) -> Direction {
            self.direction
        }

        fn analog_output(&self) -> i32 {
            self.analog_output
        }
    }

    #[test]
    fn output_calculation_matches_compare_and_subtract_modes() {
        assert_eq!(
            ComparatorBlock::calculate_output_signal(10, 6, ComparatorMode::Compare),
            10
        );
        assert_eq!(
            ComparatorBlock::calculate_output_signal(10, 6, ComparatorMode::Subtract),
            4
        );
        assert_eq!(
            ComparatorBlock::calculate_output_signal(6, 10, ComparatorMode::Subtract),
            0
        );
        assert_eq!(
            ComparatorBlock::calculate_output_signal(0, 0, ComparatorMode::Compare),
            0
        );
    }

    #[test]
    fn equality_powers_only_compare_mode() {
        assert!(ComparatorBlock::should_turn_on_from_signals(
            7,
            7,
            ComparatorMode::Compare
        ));
        assert!(!ComparatorBlock::should_turn_on_from_signals(
            7,
            7,
            ComparatorMode::Subtract
        ));
        assert!(!ComparatorBlock::should_turn_on_from_signals(
            0,
            0,
            ComparatorMode::Compare
        ));
    }

    #[test]
    fn comparator_creates_typed_output_storage() {
        init_test_registry();
        let behavior = ComparatorBlock::new(&vanilla_blocks::COMPARATOR);
        let entity = behavior
            .new_block_entity(
                Weak::new(),
                BlockPos::new(0, 64, 0),
                vanilla_blocks::COMPARATOR.default_state(),
            )
            .into_created()
            .expect("comparator should create its block entity");
        assert!(entity.downcast_ref::<ComparatorBlockEntity>().is_some());
    }

    #[test]
    fn item_frame_signal_uses_item_frame_capability() {
        init_test_registry();
        let world = fresh_test_world("comparator_item_frame_capability");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        let frame: SharedEntity = Arc::new(TestItemFrame {
            base: EntityBase::new(
                9_001,
                DVec3::new(8.5, 64.25, 8.5),
                vanilla_entities::ITEM_FRAME.dimensions,
                Arc::downgrade(&world),
            ),
            direction: Direction::North,
            analog_output: 6,
        });
        world
            .try_add_entity(frame)
            .expect("test item frame should enter loaded chunk");

        assert_eq!(
            ComparatorBlock::item_frame_signal(world.as_ref(), Direction::North, pos),
            Some(6)
        );
    }
}
