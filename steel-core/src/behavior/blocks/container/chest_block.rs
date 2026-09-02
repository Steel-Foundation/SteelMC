//! Standard single- and double-chest block behavior.

use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, ChestType, Direction};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId, Downcast as _, translations::CONTAINER_CHEST_DOUBLE};
use text_components::TextComponent;

use crate::behavior::BLOCK_BEHAVIORS;
use crate::behavior::block::{
    BlockBehavior, BlockEntityCreation, schedule_water_tick_if_waterlogged,
};
use crate::behavior::context::{
    BlockHitResult, BlockPlaceContext, InteractionResult, InventoryAccess,
};
use crate::block_entity::base_container::BaseContainer;
use crate::block_entity::entities::{CHEST_SLOTS, ChestBlockEntity};
use crate::block_entity::{BLOCK_ENTITIES, SharedBlockEntity};
use crate::entity::Entity as _;
use crate::entity::ai::path::PathComputationType;
use crate::inventory::container::{
    ContainerAccessResult, ContainerReadiness, calculate_redstone_signal_from_containers,
};
use crate::inventory::lock::{ContainerId, ContainerLockGuard, ContainerRef};
use crate::inventory::menu::kinds::chest_with_openers;
use crate::inventory::menu::{MenuCreation, MenuProvider};
use crate::player::Player;
use crate::server::jobs::{JobPoll, ServerJob, ServerJobContext};
use crate::world::{LevelReader, ScheduledTickAccess, World};

struct ChestCombination {
    entities: Vec<SharedBlockEntity>,
}

impl ChestCombination {
    fn container_refs(&self) -> Option<Vec<ContainerRef>> {
        self.entities
            .iter()
            .map(|entity| entity.container_ref())
            .collect()
    }

    fn can_open(&self, player: &Player) -> bool {
        self.entities.iter().all(|entity| {
            entity
                .downcast_ref::<ChestBlockEntity>()
                .is_some_and(|chest| chest.can_open(player))
        })
    }

    fn title(&self) -> Option<TextComponent> {
        let first = self.entities.first()?.downcast_ref::<ChestBlockEntity>()?;
        if self.entities.len() == 1 {
            return Some(first.display_name());
        }
        if first.has_custom_name() {
            return Some(first.display_name());
        }
        let second = self.entities.get(1)?.downcast_ref::<ChestBlockEntity>()?;
        if second.has_custom_name() {
            Some(second.display_name())
        } else {
            Some(TextComponent::translated(CONTAINER_CHEST_DOUBLE.msg()))
        }
    }

    /// Mirrors the locked-menu feedback of Vanilla's chest menu providers.
    ///
    /// A single chest (`RandomizableContainerBlockEntity.createMenu`) only
    /// notifies non-spectators at its center; a double chest
    /// (`ChestBlock.MENU_PROVIDER_COMBINER`) notifies everyone between both halves.
    fn send_chest_locked_notifications(&self, world: &World, player: &Player) {
        let (Some(first), Some(title)) = (self.entities.first(), self.title()) else {
            return;
        };
        let pos = first.get_block_pos();
        let mut center = DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + 0.5,
            f64::from(pos.z()) + 0.5,
        );
        if self.entities.len() == 1 {
            if player.is_spectator() {
                return;
            }
        } else {
            let (offset_x, offset_z) =
                ChestBlock::connected_direction(first.get_block_state()).offset_xz();
            center.x += f64::from(offset_x) / 2.0;
            center.z += f64::from(offset_z) / 2.0;
        }
        BaseContainer::send_chest_locked_notifications(world, center, player, title);
    }
}

/// Behavior for the standard normal chest.
#[derive(Clone, Copy)]
#[block_behavior]
pub struct ChestBlock {
    block: BlockRef,
    #[json_arg(sound_events, json = "open_sound")]
    open_sound: SoundEventRef,
    #[json_arg(sound_events, json = "close_sound")]
    close_sound: SoundEventRef,
}

/// Mirrors the menu providers Vanilla's `ChestBlock.combine` produces for one or two halves.
struct ChestMenuProvider {
    block: ChestBlock,
    world: Arc<World>,
    pos: BlockPos,
    combination: ChestCombination,
}

impl MenuProvider for ChestMenuProvider {
    fn create_menu(self: Box<Self>, player: &Player) -> MenuCreation {
        let Self {
            block,
            world,
            pos,
            combination,
        } = *self;
        if !combination.can_open(player) {
            combination.send_chest_locked_notifications(&world, player);
            return MenuCreation::Unavailable;
        }
        let Some(containers) = combination.container_refs() else {
            return MenuCreation::Unavailable;
        };
        let results = containers
            .iter()
            .map(|container| container.prepare_access(Some(player)))
            .collect::<Vec<_>>();
        if results.contains(&ContainerAccessResult::Failed) {
            return MenuCreation::Unavailable;
        }
        if results.contains(&ContainerAccessResult::Pending) {
            block.defer_open(&world, pos, player, &containers);
            return MenuCreation::Deferred;
        }
        ChestBlock::open_combination(player, combination);
        MenuCreation::Opened
    }
}

struct DeferredChestOpenJob {
    world: Arc<World>,
    pos: BlockPos,
    behavior: ChestBlock,
    container_ids: Vec<ContainerId>,
    player: Weak<Player>,
    token: u64,
}

impl ChestBlock {
    /// Creates standard chest behavior from extracted class arguments.
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

    fn open_combination(player: &Player, combination: ChestCombination) {
        let Some(containers) = combination.container_refs() else {
            return;
        };
        let Some(title) = combination.title() else {
            return;
        };
        let rows = containers.len() * 3;
        let sections = containers
            .into_iter()
            .map(|container| (container, CHEST_SLOTS))
            .collect();
        let openers = combination.entities;
        let inventory = player.inventory.clone();
        player.open_menu(title, move |context| {
            chest_with_openers(inventory, context.container_id, sections, rows, openers)
        });
    }

    fn defer_open(
        self,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        containers: &[ContainerRef],
    ) {
        let Some(player) = player.shared_in_world(world) else {
            return;
        };
        let token = player.begin_deferred_container_open();
        let job = DeferredChestOpenJob {
            world: Arc::clone(world),
            pos,
            behavior: self,
            container_ids: containers.iter().map(ContainerRef::container_id).collect(),
            player: Arc::downgrade(&player),
            token,
        };
        if !world.spawn_server_job(job) {
            player.finish_deferred_container_open(token);
        }
    }

    /// Returns the direction from this half to its connected half.
    #[must_use]
    pub fn connected_direction(state: BlockStateId) -> Direction {
        let facing = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        if state.get_value(&BlockStateProperties::CHEST_TYPE) == ChestType::Left {
            facing.rotate_y_clockwise()
        } else {
            facing.rotate_y_counter_clockwise()
        }
    }

    const fn opposite_type(chest_type: ChestType) -> ChestType {
        match chest_type {
            ChestType::Single => ChestType::Single,
            ChestType::Left => ChestType::Right,
            ChestType::Right => ChestType::Left,
        }
    }

    fn candidate_partner_facing(
        &self,
        world: &dyn LevelReader,
        pos: BlockPos,
        neighbor_direction: Direction,
    ) -> Option<Direction> {
        let state = world.get_block_state(pos.relative(neighbor_direction));
        (state.get_block() == self.block
            && state.get_value(&BlockStateProperties::CHEST_TYPE) == ChestType::Single)
            .then(|| state.get_value(&BlockStateProperties::HORIZONTAL_FACING))
    }

    fn automatic_chest_type(
        &self,
        world: &dyn LevelReader,
        pos: BlockPos,
        facing: Direction,
    ) -> ChestType {
        if self.candidate_partner_facing(world, pos, facing.rotate_y_clockwise()) == Some(facing) {
            ChestType::Left
        } else if self.candidate_partner_facing(world, pos, facing.rotate_y_counter_clockwise())
            == Some(facing)
        {
            ChestType::Right
        } else {
            ChestType::Single
        }
    }

    fn is_blocked(world: &dyn LevelReader, pos: BlockPos) -> bool {
        let above = pos.above();
        let above_state = world.get_block_state(above);
        let blocked_by_block = BLOCK_BEHAVIORS
            .get_behavior(above_state.get_block())
            .is_redstone_conductor(above_state, world, above);
        // TODO: Include sitting cats once Steel exposes the concrete cat type
        // and its sitting-pose state to world entity queries.
        blocked_by_block
    }

    fn combination(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        ignore_blocked: bool,
    ) -> Option<ChestCombination> {
        let current = world.get_block_entity(pos)?;
        current.downcast_ref::<ChestBlockEntity>()?;
        if !ignore_blocked && Self::is_blocked(world, pos) {
            return None;
        }

        let chest_type = state.get_value(&BlockStateProperties::CHEST_TYPE);
        if chest_type == ChestType::Single {
            return Some(ChestCombination {
                entities: vec![current],
            });
        }

        let neighbor_pos = pos.relative(Self::connected_direction(state));
        let neighbor_state = world.get_block_state(neighbor_pos);
        let neighbor_type = neighbor_state.try_get_value(&BlockStateProperties::CHEST_TYPE);
        let valid_pair = neighbor_state.get_block() == self.block
            && neighbor_type.is_some_and(|neighbor_type| {
                neighbor_type != ChestType::Single && neighbor_type != chest_type
            })
            && neighbor_state.try_get_value(&BlockStateProperties::HORIZONTAL_FACING)
                == Some(state.get_value(&BlockStateProperties::HORIZONTAL_FACING));
        if !valid_pair {
            return Some(ChestCombination {
                entities: vec![current],
            });
        }
        if !ignore_blocked && Self::is_blocked(world, neighbor_pos) {
            return None;
        }
        let Some(neighbor) = world.get_block_entity(neighbor_pos) else {
            return Some(ChestCombination {
                entities: vec![current],
            });
        };
        if neighbor.downcast_ref::<ChestBlockEntity>().is_none() {
            return Some(ChestCombination {
                entities: vec![current],
            });
        }

        let entities = if chest_type == ChestType::Right {
            vec![current, neighbor]
        } else {
            vec![neighbor, current]
        };
        Some(ChestCombination { entities })
    }
}

impl ServerJob for DeferredChestOpenJob {
    fn poll(&mut self, _context: &mut ServerJobContext) -> JobPoll {
        let Some(player) = self.player.upgrade() else {
            return JobPoll::Finished;
        };
        if !player.has_deferred_container_open(self.token)
            || !Arc::ptr_eq(&player.world.load_full(), &self.world)
            || player.shared_in_world(&self.world).is_none()
            || player.has_container_open()
        {
            player.finish_deferred_container_open(self.token);
            return JobPoll::Finished;
        }
        let state = self.world.get_block_state(self.pos);
        if state.get_block() != self.behavior.block {
            player.finish_deferred_container_open(self.token);
            return JobPoll::Finished;
        }
        let Some(combination) =
            self.behavior
                .combination(state, self.world.as_ref(), self.pos, false)
        else {
            player.finish_deferred_container_open(self.token);
            return JobPoll::Finished;
        };
        if !combination.can_open(&player) {
            player.finish_deferred_container_open(self.token);
            return JobPoll::Finished;
        }
        let Some(containers) = combination.container_refs() else {
            player.finish_deferred_container_open(self.token);
            return JobPoll::Finished;
        };
        let container_ids = containers
            .iter()
            .map(ContainerRef::container_id)
            .collect::<Vec<_>>();
        if container_ids != self.container_ids
            || !containers
                .iter()
                .all(|container| container.still_valid(&player))
        {
            player.finish_deferred_container_open(self.token);
            return JobPoll::Finished;
        }
        let mut pending = false;
        for container in &containers {
            match container.preparation_readiness() {
                ContainerReadiness::Ready => {}
                ContainerReadiness::Pending => pending = true,
                ContainerReadiness::NeedsPreparation => {
                    player.finish_deferred_container_open(self.token);
                    return JobPoll::Finished;
                }
            }
        }
        if pending {
            return JobPoll::Pending;
        }
        if player.finish_deferred_container_open(self.token) {
            ChestBlock::open_combination(&player, combination);
        }
        JobPoll::Finished
    }

    fn cancel(&mut self) {
        if let Some(player) = self.player.upgrade() {
            player.finish_deferred_container_open(self.token);
        }
    }
}

impl BlockBehavior for ChestBlock {
    fn chest_open_sound(&self) -> Option<SoundEventRef> {
        Some(self.open_sound)
    }

    fn chest_close_sound(&self) -> Option<SoundEventRef> {
        Some(self.close_sound)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let mut chest_type = ChestType::Single;
        let mut facing = context.horizontal_direction().opposite();
        let secondary_use = context.is_secondary_use_active();
        let clicked_face = context.clicked_face();

        if clicked_face.is_horizontal() && secondary_use {
            let neighbor_direction = clicked_face.opposite();
            if let Some(neighbor_facing) = self.candidate_partner_facing(
                context.world,
                context.place_pos(),
                neighbor_direction,
            ) && neighbor_facing.axis() != clicked_face.axis()
            {
                facing = neighbor_facing;
                chest_type = if facing.rotate_y_counter_clockwise() == neighbor_direction {
                    ChestType::Right
                } else {
                    ChestType::Left
                };
            }
        }

        if chest_type == ChestType::Single && !secondary_use {
            chest_type = self.automatic_chest_type(context.world, context.place_pos(), facing);
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
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_water_tick_if_waterlogged(state, world, pos);
        if neighbor_state.get_block() == self.block && direction.is_horizontal() {
            let neighbor_type = neighbor_state.get_value(&BlockStateProperties::CHEST_TYPE);
            if state.get_value(&BlockStateProperties::CHEST_TYPE) == ChestType::Single
                && neighbor_type != ChestType::Single
                && state.get_value(&BlockStateProperties::HORIZONTAL_FACING)
                    == neighbor_state.get_value(&BlockStateProperties::HORIZONTAL_FACING)
                && Self::connected_direction(neighbor_state) == direction.opposite()
            {
                return state.set_value(
                    &BlockStateProperties::CHEST_TYPE,
                    Self::opposite_type(neighbor_type),
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
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        if let Some(provider) = self.get_menu_provider(state, world, pos) {
            player.open_menu_provider(provider);
            // TODO: Award OPEN_CHEST and anger nearby piglins once those systems exist.
        }
        InteractionResult::Success
    }

    fn get_menu_provider(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
    ) -> Option<Box<dyn MenuProvider>> {
        let combination = self.combination(state, world.as_ref(), pos, false)?;
        Some(Box::new(ChestMenuProvider {
            block: *self,
            world: Arc::clone(world),
            pos,
            combination,
        }))
    }

    fn tick(&self, _state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return;
        };
        if let Some(openers) = block_entity.container_openers() {
            openers.recheck_open();
        }
    }

    fn trigger_event(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        param_a: i32,
        param_b: i32,
    ) -> bool {
        world
            .get_block_entity(pos)
            .is_some_and(|block_entity| block_entity.trigger_event(param_a, param_b))
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

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }

    fn get_analog_output_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        let Some(combination) = self.combination(state, world, pos, false) else {
            return 0;
        };
        let Some(containers) = combination.container_refs() else {
            return 0;
        };
        if !containers
            .iter()
            .all(|container| container.prepare_access(None) == ContainerAccessResult::Ready)
        {
            return 0;
        }
        let guard = ContainerLockGuard::lock_all(&containers);
        let locked = containers
            .iter()
            .filter_map(|container| guard.get(container.container_id()))
            .collect::<Vec<_>>();
        if locked.len() != containers.len() {
            return 0;
        }
        calculate_redstone_signal_from_containers(&locked)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use glam::DVec3;
    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use simdnbt::owned::NbtCompound;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{
        sound_events, test_support::init_test_registry, vanilla_blocks, vanilla_items,
    };
    use steel_utils::ChunkPos;
    use steel_utils::types::{GameType, UpdateFlags};
    use uuid::Uuid;

    use crate::behavior::init_behaviors;
    use crate::block_entity::init_block_entities;
    use crate::player::ResetReason;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};

    use super::*;

    fn load_additional(block_entity: &SharedBlockEntity, nbt: &NbtCompound) {
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test NBT should reborrow");
        block_entity.load_additional(&borrowed);
    }

    #[test]
    fn locked_chest_opens_only_for_a_matching_key_while_spectators_bypass_locks_but_not_loot() {
        init_test_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("locked_chest_menu_provider");
        let pos = BlockPos::new(3, 64, 3);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let state = vanilla_blocks::CHEST.default_state();
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
        let Some(block_entity) = world.get_block_entity(pos) else {
            panic!("chest placement should create its block entity");
        };
        let mut lock = NbtCompound::new();
        lock.insert("items", "minecraft:tripwire_hook");
        let mut nbt = NbtCompound::new();
        nbt.insert("lock", lock);
        load_additional(&block_entity, &nbt);

        let player =
            TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(1), "Locksmith", 1).build();
        player.base().set_position_local(DVec3::new(3.5, 64.0, 3.5));
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let behavior = ChestBlock::new(
            &vanilla_blocks::CHEST,
            &sound_events::BLOCK_CHEST_OPEN,
            &sound_events::BLOCK_CHEST_CLOSE,
        );
        let provider = |behavior: &ChestBlock| {
            behavior
                .get_menu_provider(state, &world, pos)
                .unwrap_or_else(|| panic!("unblocked chest should provide a menu"))
        };

        assert_eq!(
            provider(&behavior).create_menu(&player),
            MenuCreation::Unavailable
        );
        assert!(!player.has_container_open());

        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::TRIPWIRE_HOOK));
        assert_eq!(
            provider(&behavior).create_menu(&player),
            MenuCreation::Opened
        );
        assert!(player.has_container_open());
        player.close_container();

        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::empty());
        player.restore_game_modes(GameType::Spectator, None);
        assert_eq!(
            provider(&behavior).create_menu(&player),
            MenuCreation::Opened
        );
        assert!(player.has_container_open());
        player.close_container();

        let mut loot_nbt = NbtCompound::new();
        loot_nbt.insert("LootTable", "minecraft:chests/simple_dungeon");
        load_additional(&block_entity, &loot_nbt);
        assert_eq!(
            provider(&behavior).create_menu(&player),
            MenuCreation::Unavailable
        );
        assert!(!player.has_container_open());
    }

    #[test]
    fn double_combination_keeps_right_half_first_and_checks_both_lids() {
        init_test_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("double_chest_combination");
        let right_pos = BlockPos::new(3, 64, 3);
        let base_state = vanilla_blocks::CHEST
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, Direction::North);
        let right_state = base_state.set_value(&BlockStateProperties::CHEST_TYPE, ChestType::Right);
        let left_state = base_state.set_value(&BlockStateProperties::CHEST_TYPE, ChestType::Left);
        let left_pos = right_pos.relative(ChestBlock::connected_direction(right_state));
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(right_pos));
        assert!(world.set_block(right_pos, right_state, UpdateFlags::UPDATE_NONE));
        assert!(world.set_block(left_pos, left_state, UpdateFlags::UPDATE_NONE));
        let behavior = ChestBlock::new(
            &vanilla_blocks::CHEST,
            &sound_events::BLOCK_CHEST_OPEN,
            &sound_events::BLOCK_CHEST_CLOSE,
        );

        for (state, pos) in [(right_state, right_pos), (left_state, left_pos)] {
            let Some(combination) = behavior.combination(state, world.as_ref(), pos, false) else {
                panic!("unblocked double chest should combine");
            };
            assert_eq!(combination.entities.len(), 2);
            assert_eq!(combination.entities[0].get_block_pos(), right_pos);
            assert_eq!(combination.entities[1].get_block_pos(), left_pos);
        }

        assert!(world.set_block(
            left_pos.above(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        assert!(
            behavior
                .combination(right_state, world.as_ref(), right_pos, false)
                .is_none()
        );
        assert_eq!(
            behavior
                .combination(right_state, world.as_ref(), right_pos, true)
                .map(|combination| combination.entities.len()),
            Some(2)
        );
    }
}
