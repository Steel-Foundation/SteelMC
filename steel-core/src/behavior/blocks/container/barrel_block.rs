//! Barrel block behavior implementation.
//!
//! Opens a 27-slot container menu when right-clicked.

use std::sync::{Arc, Weak};

use crate::behavior::InventoryAccess;
use crate::behavior::block::{BlockBehavior, BlockEntityCreation};
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::block_entity::base_container::BaseContainer;
use crate::block_entity::entities::BarrelBlockEntity;
use crate::block_entity::{BLOCK_ENTITIES, SharedBlockEntity};
use crate::entity::Entity as _;
use crate::inventory::container::{
    ContainerAccessResult, ContainerReadiness, calculate_redstone_signal_from_container,
};
use crate::inventory::lock::{ContainerId, ContainerLockGuard, ContainerRef};
use crate::inventory::menu::kinds::chest_with_openers;
use crate::inventory::menu::{MenuCreation, MenuProvider};
use crate::player::Player;
use crate::server::jobs::{JobPoll, ServerJob, ServerJobContext};
use crate::world::{LevelReader, World};
use glam::DVec3;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::{vanilla_block_entity_types, vanilla_custom_stats};
use steel_utils::Downcast as _;
use steel_utils::{BlockPos, BlockStateId};

/// Behavior for barrel blocks.
///
/// Barrels are container block entities with 27 slots (3x9 grid).
/// They use the same menu as chests but cannot form double containers.
#[derive(Clone, Copy)]
#[block_behavior]
pub struct BarrelBlock {
    block: BlockRef,
}

struct DeferredBarrelOpenJob {
    world: Arc<World>,
    pos: BlockPos,
    block: BlockRef,
    container_id: ContainerId,
    player: Weak<Player>,
    token: u64,
}

/// Mirrors `BarrelBlockEntity` acting as its own Vanilla `MenuProvider`.
struct BarrelMenuProvider {
    block: BarrelBlock,
    world: Arc<World>,
    pos: BlockPos,
    block_entity: SharedBlockEntity,
}

impl MenuProvider for BarrelMenuProvider {
    fn create_menu(self: Box<Self>, player: &Player) -> MenuCreation {
        let Self {
            block,
            world,
            pos,
            block_entity,
        } = *self;
        let Some(barrel) = block_entity.downcast_ref::<BarrelBlockEntity>() else {
            return MenuCreation::Unavailable;
        };
        if !barrel.can_open(player) {
            // RandomizableContainerBlockEntity.createMenu only notifies non-spectators.
            if !player.is_spectator() {
                BaseContainer::send_chest_locked_notifications(
                    &world,
                    DVec3::new(
                        f64::from(pos.x()) + 0.5,
                        f64::from(pos.y()) + 0.5,
                        f64::from(pos.z()) + 0.5,
                    ),
                    player,
                    barrel.display_name(),
                );
            }
            return MenuCreation::Unavailable;
        }
        let Some(container_ref) = block_entity.container_ref() else {
            return MenuCreation::Unavailable;
        };
        match container_ref.prepare_access(Some(player)) {
            ContainerAccessResult::Ready => {
                BarrelBlock::open(player, block_entity, container_ref);
                MenuCreation::Opened
            }
            ContainerAccessResult::Pending => {
                block.defer_open(&world, pos, player, container_ref.container_id());
                MenuCreation::Deferred
            }
            ContainerAccessResult::Failed => MenuCreation::Unavailable,
        }
    }
}

impl BarrelBlock {
    /// Creates a new barrel block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn open(player: &Player, block_entity: SharedBlockEntity, container_ref: ContainerRef) {
        let Some(barrel) = block_entity.downcast_ref::<BarrelBlockEntity>() else {
            return;
        };
        let title = barrel.display_name();
        let inventory = player.inventory.clone();
        player.open_menu(title, move |context| {
            chest_with_openers(
                inventory,
                context.container_id,
                vec![(container_ref, 27)],
                3,
                vec![block_entity],
            )
        });
    }

    fn defer_open(
        self,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        container_id: ContainerId,
    ) {
        let Some(player) = player.shared_in_world(world) else {
            return;
        };
        let token = player.begin_deferred_container_open();
        let job = DeferredBarrelOpenJob {
            world: Arc::clone(world),
            pos,
            block: self.block,
            container_id,
            player: Arc::downgrade(&player),
            token,
        };
        if !world.spawn_server_job(job) {
            player.finish_deferred_container_open(token);
        }
    }
}

impl ServerJob for DeferredBarrelOpenJob {
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
        if self.world.get_block_state(self.pos).get_block() != self.block {
            player.finish_deferred_container_open(self.token);
            return JobPoll::Finished;
        }
        let Some(block_entity) = self.world.get_block_entity(self.pos) else {
            player.finish_deferred_container_open(self.token);
            return JobPoll::Finished;
        };
        let Some(barrel) = block_entity.downcast_ref::<BarrelBlockEntity>() else {
            player.finish_deferred_container_open(self.token);
            return JobPoll::Finished;
        };
        let Some(container) = block_entity.container_ref() else {
            player.finish_deferred_container_open(self.token);
            return JobPoll::Finished;
        };
        if container.container_id() != self.container_id
            || !container.still_valid(&player)
            || !barrel.can_open(&player)
        {
            player.finish_deferred_container_open(self.token);
            return JobPoll::Finished;
        }
        match container.preparation_readiness() {
            ContainerReadiness::Pending => JobPoll::Pending,
            ContainerReadiness::NeedsPreparation => {
                player.finish_deferred_container_open(self.token);
                JobPoll::Finished
            }
            ContainerReadiness::Ready => {
                if player.finish_deferred_container_open(self.token) {
                    BarrelBlock::open(&player, block_entity, container);
                }
                JobPoll::Finished
            }
        }
    }

    fn cancel(&mut self) {
        if let Some(player) = self.player.upgrade() {
            player.finish_deferred_container_open(self.token);
        }
    }
}

impl BlockBehavior for BarrelBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // Barrel faces opposite to the player's look direction (all 6 directions).
        let facing = context.get_nearest_looking_direction().opposite();

        Some(
            self.block
                .default_state()
                .set_value(&BlockStateProperties::FACING, facing),
        )
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
            player.award_custom_stat(&vanilla_custom_stats::OPEN_BARREL);
            // TODO: Anger nearby piglins (PiglinAi.angerNearbyPiglins)
        }
        InteractionResult::Success
    }

    fn get_menu_provider(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
    ) -> Option<Box<dyn MenuProvider>> {
        let block_entity = world.get_block_entity(pos)?;
        block_entity.downcast_ref::<BarrelBlockEntity>()?;
        Some(Box::new(BarrelMenuProvider {
            block: *self,
            world: Arc::clone(world),
            pos,
            block_entity,
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

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::BARREL,
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

    fn get_analog_output_signal(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return 0;
        };
        if block_entity.downcast_ref::<BarrelBlockEntity>().is_none() {
            return 0;
        }
        let Some(container_ref) = ContainerRef::from_block_entity(block_entity) else {
            return 0;
        };
        if container_ref.prepare_access(None) != ContainerAccessResult::Ready {
            return 0;
        }
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
    use std::{
        io::Cursor,
        sync::{Arc, Weak},
    };

    use glam::DVec3;
    use simdnbt::{borrow::read_compound as read_borrowed_compound, owned::NbtCompound};
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};
    use steel_utils::{
        ChunkPos,
        types::{InteractionHand, UpdateFlags},
    };
    use uuid::Uuid;

    use crate::{
        behavior::{InventoryAccess, init_behaviors},
        block_entity::init_block_entities,
        player::ResetReason,
        server::{Server, jobs::ServerJobQueue},
        test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk},
    };

    use super::*;

    #[test]
    fn explorer_map_completion_automatically_opens_the_revalidated_barrel() {
        init_test_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("deferred_barrel_open");
        let jobs = Arc::new(ServerJobQueue::new());
        world.bind_server_jobs(Arc::downgrade(&jobs));
        let pos = BlockPos::new(3, 64, 3);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let state = vanilla_blocks::BARREL.default_state();
        assert!(world.set_block(pos, state, UpdateFlags::UPDATE_NONE));
        let Some(block_entity) = world.get_block_entity(pos) else {
            panic!("barrel placement should create its block entity");
        };
        let mut loot_nbt = NbtCompound::new();
        loot_nbt.insert("LootTable", "minecraft:chests/shipwreck_map");
        loot_nbt.insert("LootTableSeed", 42_i64);
        let mut bytes = Vec::new();
        loot_nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test loot NBT should reborrow");
        block_entity.load_additional(&borrowed);

        let player = TestPlayerBuilder::new(Arc::clone(&world), "Explorer", 1)
            .uuid(Uuid::from_u128(1))
            .build();
        player.base().set_position_local(DVec3::new(3.5, 64.0, 3.5));
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let hit = BlockHitResult {
            location: DVec3::new(3.5, 64.5, 3.5),
            direction: Direction::Up,
            block_pos: pos,
            miss: false,
            inside: false,
            world_border_hit: false,
        };
        let mut inventory =
            InventoryAccess::new(player.inventory.clone(), InteractionHand::MainHand);
        let behavior = BarrelBlock::new(&vanilla_blocks::BARREL);

        assert_eq!(
            behavior.use_without_item(state, &world, pos, &player, &hit, &mut inventory),
            InteractionResult::Success
        );
        assert!(!player.has_container_open());
        assert_eq!(jobs.len(), 2);

        let tick_stats = jobs.tick(Weak::<Server>::new(), 0, true);
        assert_eq!(tick_stats.finished, 2);
        assert!(player.has_container_open());
    }
}
