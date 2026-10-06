//! Furnace, blast-furnace, and smoker block behavior.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, EnumProperty};
use steel_registry::stat::custom::CustomStatRef;
use steel_registry::{vanilla_block_entity_types, vanilla_custom_stats};
use steel_utils::{BlockPos, BlockStateId, translations};
use text_components::TextComponent;

use crate::behavior::block::{BlockBehavior, BlockEntityCreation};
use crate::behavior::{BlockHitResult, BlockPlaceContext, InteractionResult, InventoryAccess};
use crate::block_entity::entities::FurnaceKind;
use crate::block_entity::{BLOCK_ENTITIES, BlockEntityTicker};
use crate::inventory::container::calculate_redstone_signal_from_container;
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::inventory::menu::kinds::furnace_menu;
use crate::player::Player;
use crate::world::{LevelReader, World};

const FACING: &EnumProperty<Direction> = &BlockStateProperties::HORIZONTAL_FACING;

#[block_behavior]
/// Vanilla furnace block behavior.
pub struct FurnaceBlock {
    block: BlockRef,
}

#[block_behavior]
/// Vanilla blast-furnace block behavior.
pub struct BlastFurnaceBlock {
    block: BlockRef,
}

#[block_behavior]
/// Vanilla smoker block behavior.
pub struct SmokerBlock {
    block: BlockRef,
}

fn placement_state(block: BlockRef, context: &BlockPlaceContext<'_>) -> BlockStateId {
    block
        .default_state()
        .set_value(FACING, context.horizontal_direction().opposite())
}

fn open_furnace(
    world: &Arc<World>,
    pos: BlockPos,
    player: &Player,
    expected_type: BlockEntityTypeRef,
    kind: FurnaceKind,
    title: TextComponent,
    stat: CustomStatRef,
) -> InteractionResult {
    let Some(block_entity) = world.get_block_entity(pos) else {
        return InteractionResult::Success;
    };
    if block_entity.get_type() != expected_type {
        return InteractionResult::Success;
    }
    let Some(container) = ContainerRef::from_block_entity(block_entity) else {
        return InteractionResult::Success;
    };
    let inventory = player.inventory.clone();
    player.open_menu(title, move |context| {
        furnace_menu(inventory, context.container_id, container, kind)
    });
    player.award_custom_stat(stat);
    InteractionResult::Success
}

fn analog_output(world: &dyn LevelReader, pos: BlockPos) -> i32 {
    let Some(container) = world
        .get_block_entity(pos)
        .and_then(ContainerRef::from_block_entity)
    else {
        return 0;
    };
    let guard = ContainerLockGuard::lock_all(&[&container]);
    guard
        .get(container.container_id())
        .map_or(0, calculate_redstone_signal_from_container)
}

macro_rules! impl_furnace_block {
    ($name:ident, $entity_type:ident, $kind:ident, $title:ident, $stat:ident) => {
        impl $name {
            /// Creates this behavior for its registered block.
            #[must_use]
            pub const fn new(block: BlockRef) -> Self {
                Self { block }
            }
        }

        impl BlockBehavior for $name {
            fn get_state_for_placement(
                &self,
                context: &BlockPlaceContext<'_>,
            ) -> Option<BlockStateId> {
                Some(placement_state(self.block, context))
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
                open_furnace(
                    world,
                    pos,
                    player,
                    &vanilla_block_entity_types::$entity_type,
                    FurnaceKind::$kind,
                    TextComponent::translated(translations::$title.msg()),
                    &vanilla_custom_stats::$stat,
                )
            }

            fn new_block_entity(
                &self,
                level: Weak<World>,
                pos: BlockPos,
                state: BlockStateId,
            ) -> BlockEntityCreation {
                BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
                    &vanilla_block_entity_types::$entity_type,
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
                BlockEntityTicker::for_matching_entity_tick(
                    block_entity_type,
                    &vanilla_block_entity_types::$entity_type,
                )
            }

            fn affect_neighbors_after_removal(
                &self,
                _state: BlockStateId,
                world: &Arc<World>,
                pos: BlockPos,
                _moved_by_piston: bool,
            ) {
                world.update_neighbor_for_output_signal(pos, self.block);
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
                analog_output(world, pos)
            }
        }
    };
}

impl_furnace_block!(
    FurnaceBlock,
    FURNACE,
    Furnace,
    CONTAINER_FURNACE,
    INTERACT_WITH_FURNACE
);
impl_furnace_block!(
    BlastFurnaceBlock,
    BLAST_FURNACE,
    BlastFurnace,
    CONTAINER_BLAST_FURNACE,
    INTERACT_WITH_BLAST_FURNACE
);
impl_furnace_block!(
    SmokerBlock,
    SMOKER,
    Smoker,
    CONTAINER_SMOKER,
    INTERACT_WITH_SMOKER
);
