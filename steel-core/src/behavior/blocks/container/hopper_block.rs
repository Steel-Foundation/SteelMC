//! Vanilla hopper block behavior.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{
    Axis, BlockStateProperties, BoolProperty, Direction, EnumProperty,
};
use steel_registry::vanilla_block_entity_types;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Downcast as _, translations};
use text_components::TextComponent;

use crate::behavior::{
    BlockBehavior, BlockEntityCreation, BlockHitResult, BlockPlaceContext, InteractionResult,
    InventoryAccess,
};
use crate::block_entity::entities::HopperBlockEntity;
use crate::block_entity::{BLOCK_ENTITIES, BlockEntityTicker};
use crate::entity::{Entity, InsideBlockEffectCollector};
use crate::inventory::container::calculate_redstone_signal_from_container;
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::inventory::menu::kinds::hopper;
use crate::player::Player;
use crate::world::{LevelReader, SignalGetter as _, World};

/// Vanilla `HopperBlock`, including its redstone-driven `enabled` lock.
#[block_behavior]
pub struct HopperBlock {
    block: BlockRef,
}

const FACING: &EnumProperty<Direction> = &BlockStateProperties::FACING_HOPPER;
const ENABLED: &BoolProperty = &BlockStateProperties::ENABLED;

impl HopperBlock {
    /// Creates hopper behavior for `block`.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn check_powered_state(state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let should_be_on = !world.has_neighbor_signal(pos);
        if should_be_on == state.get_value(ENABLED) {
            return;
        }
        world.set_block(
            pos,
            state.set_value(ENABLED, should_be_on),
            UpdateFlags::UPDATE_CLIENTS,
        );
    }
}

impl BlockBehavior for HopperBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // `facing_hopper` has no up value, so any vertical click points the spout down.
        let direction = context.clicked_face().opposite();
        let facing = if direction.get_axis() == Axis::Y {
            Direction::Down
        } else {
            direction
        };

        Some(
            self.block
                .default_state()
                .set_value(FACING, facing)
                .set_value(ENABLED, true),
        )
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        if old_state.get_block() != state.get_block() {
            Self::check_powered_state(state, world, pos);
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
        Self::check_powered_state(state, world, pos);
    }

    fn use_without_item(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return InteractionResult::Pass;
        };

        // Vanilla `RandomizableContainerBlockEntity.createMenu` unpacks the loot table
        // before the menu opens.
        if let Some(hopper_entity) = block_entity.downcast_ref::<HopperBlockEntity>() {
            hopper_entity.unpack_loot_table();
        }

        let Some(container_ref) = ContainerRef::from_block_entity(block_entity) else {
            return InteractionResult::Pass;
        };

        let inventory = player.inventory.clone();
        player.open_menu(
            TextComponent::translated(translations::CONTAINER_HOPPER.msg()),
            move |context| hopper(inventory, context.container_id, container_ref),
        );

        // TODO: Award stat INSPECT_HOPPER

        InteractionResult::Success
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::HOPPER,
            level,
            pos,
            state,
        ))
    }

    fn get_block_entity_ticker(
        &self,
        _world: &Arc<World>,
        _state: BlockStateId,
        block_entity_type: BlockEntityTypeRef,
    ) -> Option<BlockEntityTicker> {
        // Vanilla gates only on the client check; `enabled` is read inside the block entity tick.
        BlockEntityTicker::for_matching_filtered_entity_tick(
            block_entity_type,
            &vanilla_block_entity_types::HOPPER,
            HopperBlockEntity::should_tick,
        )
    }

    fn entity_inside(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
        _effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        if let Some(block_entity) = world.get_block_entity(pos)
            && let Some(hopper_entity) = block_entity.downcast_ref::<HopperBlockEntity>()
        {
            hopper_entity.entity_inside(world, pos, state, entity);
        }
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        let Some(container_ref) = world
            .get_block_entity(pos)
            .and_then(ContainerRef::from_block_entity)
        else {
            return 0;
        };
        let guard = ContainerLockGuard::lock_all(&[&container_ref]);
        guard
            .get(container_ref.container_id())
            .map_or(0, |container| {
                calculate_redstone_signal_from_container(container)
            })
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{vanilla_blocks, vanilla_items};
    use steel_utils::ChunkPos;
    use steel_utils::types::InteractionHand;

    use super::*;
    use crate::behavior::{PlacementOrientation, PlacementSource};
    use crate::bootstrap::init_globals_once;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    const TEST_POS: BlockPos = BlockPos::new(8, 64, 8);
    const ARBITRARY_COMPARATOR_QUERY_DIRECTION: Direction = Direction::North;

    fn hopper_hit(clicked_face: Direction) -> BlockHitResult {
        BlockHitResult {
            location: DVec3::new(
                f64::from(TEST_POS.x()) + 0.5,
                f64::from(TEST_POS.y()) + 0.5,
                f64::from(TEST_POS.z()) + 0.5,
            ),
            direction: clicked_face,
            block_pos: TEST_POS,
            miss: false,
            inside: false,
            world_border_hit: false,
        }
    }

    fn placed_state(world: &Arc<World>, clicked_face: Direction) -> BlockStateId {
        let mut stack = ItemStack::new(&vanilla_items::HOPPER);
        let source = PlacementSource::direct(
            None,
            InteractionHand::MainHand,
            &mut stack,
            PlacementOrientation::Directional {
                direction: clicked_face,
            },
            false,
        );
        let context = BlockPlaceContext::new(world, source, &hopper_hit(clicked_face));
        HopperBlock::new(&vanilla_blocks::HOPPER)
            .get_state_for_placement(&context)
            .expect("hopper always has a placement state")
    }

    #[test]
    fn placement_points_the_spout_away_from_the_clicked_face() {
        init_globals_once();
        let world = fresh_test_world("hopper_placement_facing");

        let from_top = placed_state(&world, Direction::Up);
        assert_eq!(from_top.get_value(FACING), Direction::Down);
        assert!(from_top.get_value(ENABLED));

        let from_bottom = placed_state(&world, Direction::Down);
        assert_eq!(from_bottom.get_value(FACING), Direction::Down);

        let from_north = placed_state(&world, Direction::North);
        assert_eq!(from_north.get_value(FACING), Direction::South);
        assert!(from_north.get_value(ENABLED));
    }

    #[test]
    fn neighbor_signal_toggles_enabled_without_disabling_the_ticker() {
        init_globals_once();
        let world = fresh_test_world("hopper_powered_state");
        let power_pos = TEST_POS.west();
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(TEST_POS));
        assert!(world.set_block(
            power_pos,
            vanilla_blocks::REDSTONE_BLOCK.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        // `on_place` corrects the placement default of `enabled = true`.
        assert!(world.set_block(
            TEST_POS,
            vanilla_blocks::HOPPER.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        let powered = world.get_block_state(TEST_POS);
        assert!(!powered.get_value(ENABLED));

        let behavior = HopperBlock::new(&vanilla_blocks::HOPPER);
        assert!(
            behavior
                .get_block_entity_ticker(&world, powered, &vanilla_block_entity_types::HOPPER)
                .is_some()
        );
        assert!(
            behavior
                .get_block_entity_ticker(&world, powered, &vanilla_block_entity_types::BARREL)
                .is_none()
        );

        assert!(world.remove_block(power_pos, false));
        assert!(world.get_block_state(TEST_POS).get_value(ENABLED));
    }

    #[test]
    fn entity_inside_hook_feeds_the_hopper_block_entity() {
        init_globals_once();
        let world = fresh_test_world("hopper_entity_inside");
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(TEST_POS));
        assert!(world.set_block(
            TEST_POS,
            vanilla_blocks::HOPPER.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        let item_entity = world
            .spawn_item_with_velocity(
                DVec3::new(8.5, 64.9, 8.5),
                ItemStack::new(&vanilla_items::STONE),
                DVec3::ZERO,
            )
            .expect("item entity should spawn");

        let behavior = HopperBlock::new(&vanilla_blocks::HOPPER);
        let mut effects = InsideBlockEffectCollector::new();
        behavior.entity_inside(
            world.get_block_state(TEST_POS),
            &world,
            TEST_POS,
            &*item_entity,
            &mut effects,
            false,
        );

        assert!(item_entity.is_removed());
        let container_ref = world
            .get_block_entity(TEST_POS)
            .and_then(ContainerRef::from_block_entity)
            .expect("placed hopper should expose its container");
        let guard = ContainerLockGuard::lock_all(&[&container_ref]);
        let absorbed = guard
            .get(container_ref.container_id())
            .is_some_and(|container| !container.get_item(0).is_empty());
        assert!(absorbed);
    }

    #[test]
    fn comparator_reads_the_hopper_container() {
        init_globals_once();
        let world = fresh_test_world("hopper_analog_output");
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(TEST_POS));
        assert!(world.set_block(
            TEST_POS,
            vanilla_blocks::HOPPER.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));

        let behavior = HopperBlock::new(&vanilla_blocks::HOPPER);
        let state = world.get_block_state(TEST_POS);
        assert!(behavior.has_analog_output_signal(state));
        assert_eq!(
            behavior.get_analog_output_signal(
                state,
                &world,
                TEST_POS,
                ARBITRARY_COMPARATOR_QUERY_DIRECTION,
            ),
            0,
        );

        let container_ref = world
            .get_block_entity(TEST_POS)
            .and_then(ContainerRef::from_block_entity)
            .expect("placed hopper should expose its container");
        let mut guard = ContainerLockGuard::lock_all(&[&container_ref]);
        assert!(guard.set_item(
            container_ref.container_id(),
            0,
            ItemStack::new(&vanilla_items::STONE),
        ));
        drop(guard);

        assert!(
            behavior.get_analog_output_signal(
                state,
                &world,
                TEST_POS,
                ARBITRARY_COMPARATOR_QUERY_DIRECTION,
            ) > 0
        );
    }
}
