use std::sync::{Arc, Weak};

use glam::DVec3;
use smallvec::{SmallVec, smallvec};
use steel_macros::block_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt as _,
        properties::{BlockStateProperties, ChestType},
    },
    sound_event::SoundEventRef,
    sound_events, vanilla_block_entity_types, vanilla_custom_stats,
};
use steel_utils::{BlockPos, BlockStateId, Direction, Downcast as _, translations};
use text_components::TextComponent;

use crate::{
    behavior::{
        BlockBehavior, BlockHitResult, BlockPlaceContext, InteractionResult, InventoryAccess,
        block::{BlockEntityCreation, schedule_water_tick_if_waterlogged},
    },
    block_entity::{BLOCK_ENTITIES, SharedBlockEntity, entities::ChestBlockEntity},
    entity::{Entity as _, ai::path::PathComputationType},
    inventory::{
        container::calculate_redstone_signal_from_containers,
        lock::{ContainerLockGuard, ContainerRef},
        menu::kinds::block_chest,
    },
    nameable::Nameable,
    player::Player,
    world::{LevelReader, ScheduledTickAccess, World, is_redstone_conductor},
};

/// Normal single/double chest behavior. Sitting-cat blocking and piglin anger
/// are explicitly deferred until their entity foundations exist.
#[block_behavior]
pub struct ChestBlock {
    block: BlockRef,
    #[json_arg(sound_events)]
    pub(crate) open_sound: SoundEventRef,
    #[json_arg(sound_events)]
    pub(crate) close_sound: SoundEventRef,
}

/// A chest's inventories in vanilla slot order, retaining their real owners.
pub struct ChestContainer {
    pub(crate) owners: SmallVec<[SharedBlockEntity; 2]>,
    /// Right half first for doubles; each half remains independently lockable.
    pub containers: SmallVec<[ContainerRef; 2]>,
}

impl ChestBlock {
    /// Creates behavior using extracted chest sounds.
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

    /// Direction from a chest half to its partner.
    #[must_use]
    pub fn connected_direction(state: BlockStateId) -> Direction {
        let facing = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        if state.get_value(&BlockStateProperties::CHEST_TYPE) == ChestType::Left {
            facing.rotate_y_clockwise()
        } else {
            facing.rotate_y_counter_clockwise()
        }
    }

    fn candidate_partner_facing(
        &self,
        world: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
    ) -> Option<Direction> {
        let state = world.get_block_state(pos.relative(direction));
        (state.get_block() == self.block
            && state.get_value(&BlockStateProperties::CHEST_TYPE) == ChestType::Single)
            .then(|| state.get_value(&BlockStateProperties::HORIZONTAL_FACING))
    }

    fn is_blocked(world: &dyn LevelReader, pos: BlockPos) -> bool {
        let above = pos.above();
        is_redstone_conductor(world, world.get_block_state(above), above)
    }

    /// Combines compatible halves; automation can request ignoring lid obstruction.
    #[must_use]
    pub fn get_container(
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        ignore_blocked: bool,
    ) -> Option<ChestContainer> {
        let first = world.get_block_entity(pos)?;
        if first.get_type() != &vanilla_block_entity_types::CHEST
            || (!ignore_blocked && Self::is_blocked(world, pos))
        {
            return None;
        }
        let chest_type = state.get_value(&BlockStateProperties::CHEST_TYPE);
        let mut owners: SmallVec<[SharedBlockEntity; 2]> = smallvec![first];
        if chest_type != ChestType::Single {
            let neighbor_pos = pos.relative(Self::connected_direction(state));
            let neighbor = world.get_block_state(neighbor_pos);
            if neighbor.get_block() == state.get_block()
                && neighbor.get_value(&BlockStateProperties::CHEST_TYPE) != ChestType::Single
                && neighbor.get_value(&BlockStateProperties::CHEST_TYPE) != chest_type
                && neighbor.get_value(&BlockStateProperties::HORIZONTAL_FACING)
                    == state.get_value(&BlockStateProperties::HORIZONTAL_FACING)
            {
                if !ignore_blocked && Self::is_blocked(world, neighbor_pos) {
                    return None;
                }
                if let Some(other) = world.get_block_entity(neighbor_pos)
                    && other.get_type() == &vanilla_block_entity_types::CHEST
                {
                    if chest_type == ChestType::Right {
                        owners.push(other);
                    } else {
                        owners.insert(0, other);
                    }
                }
            }
        }
        let containers = owners
            .iter()
            .map(|owner| owner.container_ref())
            .collect::<Option<_>>()?;
        Some(ChestContainer { owners, containers })
    }

    fn display_name(chest: &ChestContainer) -> TextComponent {
        for owner in &chest.owners {
            if let Some(name) = owner.as_nameable().and_then(Nameable::custom_name) {
                return name;
            }
        }
        if chest.owners.len() == 2 {
            translations::CONTAINER_CHEST_DOUBLE.msg().component()
        } else {
            translations::CONTAINER_CHEST.msg().component()
        }
    }
}

impl BlockBehavior for ChestBlock {
    fn chest_sounds(&self) -> Option<(SoundEventRef, SoundEventRef)> {
        Some((self.open_sound, self.close_sound))
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let pos = context.place_pos();
        let world = context.world.as_ref();
        let mut facing = context.horizontal_direction().opposite();
        let mut chest_type = ChestType::Single;
        let clicked = context.clicked_face();
        let secondary = context.is_secondary_use_active();
        if clicked.get_axis().is_horizontal()
            && secondary
            && let Some(neighbor_facing) =
                self.candidate_partner_facing(world, pos, clicked.opposite())
            && neighbor_facing.get_axis() != clicked.get_axis()
        {
            facing = neighbor_facing;
            chest_type = if facing.rotate_y_counter_clockwise() == clicked.opposite() {
                ChestType::Right
            } else {
                ChestType::Left
            };
        }
        if chest_type == ChestType::Single && !secondary {
            if self.candidate_partner_facing(world, pos, facing.rotate_y_clockwise())
                == Some(facing)
            {
                chest_type = ChestType::Left;
            } else if self.candidate_partner_facing(world, pos, facing.rotate_y_counter_clockwise())
                == Some(facing)
            {
                chest_type = ChestType::Right;
            }
        }
        Some(
            self.block
                .default_state()
                .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing)
                .set_value(&BlockStateProperties::CHEST_TYPE, chest_type)
                .set_value(
                    &BlockStateProperties::WATERLOGGED,
                    context.is_water_source(),
                ),
        )
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);
        if neighbor.get_block() == self.block && direction.get_axis().is_horizontal() {
            let neighbor_type = neighbor.get_value(&BlockStateProperties::CHEST_TYPE);
            if state.get_value(&BlockStateProperties::CHEST_TYPE) == ChestType::Single
                && neighbor_type != ChestType::Single
                && state.get_value(&BlockStateProperties::HORIZONTAL_FACING)
                    == neighbor.get_value(&BlockStateProperties::HORIZONTAL_FACING)
                && Self::connected_direction(neighbor) == direction.opposite()
            {
                return state.set_value(
                    &BlockStateProperties::CHEST_TYPE,
                    if neighbor_type == ChestType::Left {
                        ChestType::Right
                    } else {
                        ChestType::Left
                    },
                );
            }
        } else if Self::connected_direction(state) == direction {
            return state.set_value(&BlockStateProperties::CHEST_TYPE, ChestType::Single);
        }
        state
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let Some(chest) = Self::get_container(state, world.as_ref(), pos, false) else {
            return InteractionResult::Success;
        };
        let title = Self::display_name(&chest);
        player.award_custom_stat(&vanilla_custom_stats::OPEN_CHEST);
        if !chest.owners.iter().all(|owner| {
            owner
                .downcast_ref::<ChestBlockEntity>()
                .is_some_and(|chest| chest.can_open(player))
        }) {
            if !player.is_spectator() {
                player.send_overlay_message(
                    &translations::CONTAINER_IS_LOCKED
                        .message([title])
                        .component(),
                );
                let mut center = DVec3::new(
                    f64::from(pos.x()) + 0.5,
                    f64::from(pos.y()) + 0.5,
                    f64::from(pos.z()) + 0.5,
                );
                if chest.owners.len() == 2 {
                    center += DVec3::from(Self::connected_direction(state).offset_vec()) * 0.5;
                }
                world.play_sound_at(
                    &sound_events::BLOCK_CHEST_LOCKED,
                    SoundSource::Blocks,
                    center,
                    1.0,
                    1.0,
                    None,
                );
            }
            return InteractionResult::Success;
        }
        for owner in &chest.owners {
            if let Some(randomizable) = owner.as_randomizable_container()
                && let Err(error) = randomizable.unpack_loot_table(Some(player))
            {
                log::error!(
                    "Could not unpack chest loot at {:?}: {error}",
                    owner.get_block_pos()
                );
                return InteractionResult::Success;
            }
        }
        let inventory = Arc::clone(&player.inventory);
        player.open_menu(title, move |context| {
            block_chest(
                inventory,
                context.container_id,
                chest.owners,
                chest.containers,
            )
        });
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
        let Some(chest) = Self::get_container(state, world, pos, false) else {
            return 0;
        };
        let Ok(guard) = ContainerLockGuard::lock_all(&chest.containers) else {
            // TODO: Preserve vanilla access ordering so loot-generation contention
            // cannot become a false zero comparator signal. Returning zero here
            // also hides generation failures; distinguish them from empty contents.
            return 0;
        };
        calculate_redstone_signal_from_containers(
            chest
                .containers
                .iter()
                .filter_map(|container| guard.get(container.container_id())),
        )
    }
    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _moved: bool,
    ) {
        world.update_neighbor_for_output_signal(pos, state.get_block());
    }
    fn tick(&self, _state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if let Some(entity) = world.get_block_entity(pos)
            && let Some(chest) = entity.downcast_ref::<ChestBlockEntity>()
        {
            chest.recheck_open();
        }
    }
    fn trigger_event(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        a: i32,
        b: i32,
    ) -> bool {
        world
            .get_block_entity(pos)
            .is_some_and(|entity| entity.trigger_event(a, b))
    }
    fn is_pathfindable(&self, _state: BlockStateId, _kind: PathComputationType) -> bool {
        false
    }
}

#[cfg(test)]
mod tests;
