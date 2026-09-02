//! Barrel block entity implementation.
//!
//! Barrels are container block entities with 27 slots (3x9 grid),
//! functioning similarly to chests but without double-chest behavior.

use std::sync::{Arc, Weak};

use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::data_components::DataComponentMap;
use steel_registry::{sound_events, vanilla_block_entity_types};
use steel_utils::types::UpdateFlags;
use steel_utils::{
    BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex,
    translations::CONTAINER_BARREL,
};
use text_components::TextComponent;

use crate::block_entity::randomizable_container::RandomizableContainer;
use crate::block_entity::{
    BlockEntity, BlockEntityBase, BlockEntityComponentInput, ContainerOpeners,
    ContainerOpenersCounter,
};
use crate::inventory::lock::{ContainerId, ContainerRef, SharedContainer};
use crate::player::Player;
use crate::world::World;

/// Number of slots in a barrel (3 rows of 9).
pub const BARREL_SLOTS: usize = 27;

/// Barrel block entity.
///
/// A simple container with 27 slots, using the same menu as chests.
pub struct BarrelBlockEntity {
    base: Arc<BlockEntityBase>,
    container: Arc<SyncMutex<RandomizableContainer>>,
    container_ref: ContainerRef,
    openers: ContainerOpenersCounter,
}

// SAFETY: This key is owned by Steel and uniquely identifies `BarrelBlockEntity`.
unsafe impl DowncastType for BarrelBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/barrel");
}

impl BarrelBlockEntity {
    /// Creates a new barrel block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        let base = Arc::new(BlockEntityBase::new(
            &vanilla_block_entity_types::BARREL,
            level,
            pos,
            state,
        ));
        let container = Arc::new(SyncMutex::new(RandomizableContainer::new(BARREL_SLOTS)));
        let shared_container: SharedContainer = container.clone();
        Self {
            container_ref: ContainerRef::owned_by_block_entity(shared_container, Arc::clone(&base)),
            base,
            container,
            openers: ContainerOpenersCounter::default(),
        }
    }

    /// Returns the menu title, preferring a persisted custom name.
    #[must_use]
    pub fn display_name(&self) -> TextComponent {
        self.container
            .lock()
            .display_name(TextComponent::translated(CONTAINER_BARREL.msg()))
    }

    /// Returns whether `player` may open this barrel.
    #[must_use]
    pub fn can_open(&self, player: &Player) -> bool {
        let main_hand = player.get_main_hand_item();
        self.container.lock().can_open(player, &main_hand)
    }

    fn play_sound(&self, world: &World, state: BlockStateId, opening: bool) {
        let direction = state.get_value(&BlockStateProperties::FACING);
        let (offset_x, offset_y, offset_z) = direction.offset();
        let pos = self.get_block_pos();
        world.play_sound_at(
            if opening {
                &sound_events::BLOCK_BARREL_OPEN
            } else {
                &sound_events::BLOCK_BARREL_CLOSE
            },
            SoundSource::Blocks,
            glam::DVec3::new(
                f64::from(pos.x()) + 0.5 + f64::from(offset_x) * 0.5,
                f64::from(pos.y()) + 0.5 + f64::from(offset_y) * 0.5,
                f64::from(pos.z()) + 0.5 + f64::from(offset_z) * 0.5,
            ),
            0.5,
            rand::random::<f32>() * 0.1 + 0.9,
            None,
        );
    }

    fn update_open_state(&self, world: &Arc<World>, state: BlockStateId, open: bool) {
        let _ = world.set_block(
            self.get_block_pos(),
            state.set_value(&BlockStateProperties::OPEN, open),
            UpdateFlags::UPDATE_ALL,
        );
    }
}

impl BlockEntity for BarrelBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
        self.container_ref.prepare_access(None);
        let items = {
            let mut container = self.container.lock();
            container.remove_and_take_ready_items()
        };
        let Some(world) = self.get_level() else {
            return;
        };
        for item in items {
            world.drop_item_stack(pos, item);
        }
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        // Convert to NbtCompound view for accessing methods
        let nbt_view: NbtCompoundView<'_, '_> = nbt.into();
        self.container.lock().load(&nbt_view);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.container.lock().save(nbt);
    }

    fn apply_implicit_components(&self, components: &BlockEntityComponentInput<'_>) {
        self.container.lock().apply_implicit_components(components);
    }

    fn collect_implicit_components(&self, components: &mut DataComponentMap) {
        self.container
            .lock()
            .collect_implicit_components(components);
    }

    fn remove_components_from_tag(&self, nbt: &mut NbtCompound) {
        RandomizableContainer::remove_components_from_tag(nbt);
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        // Barrels don't need to send inventory to clients on chunk load
        // (unlike signs which display text)
        None
    }

    fn container_ref(&self) -> Option<ContainerRef> {
        Some(self.container_ref.clone())
    }

    fn container_openers(&self) -> Option<&dyn ContainerOpeners> {
        Some(self)
    }
}

impl ContainerOpeners for BarrelBlockEntity {
    fn openers_counter(&self) -> &ContainerOpenersCounter {
        &self.openers
    }

    fn opener_container_id(&self) -> ContainerId {
        self.container_ref.container_id()
    }

    fn on_open(&self, world: &Arc<World>, _pos: BlockPos, block_state: BlockStateId) {
        self.play_sound(world, block_state, true);
        self.update_open_state(world, block_state, true);
    }

    fn on_close(&self, world: &Arc<World>, _pos: BlockPos, block_state: BlockStateId) {
        self.play_sound(world, block_state, false);
        self.update_open_state(world, block_state, false);
    }

    fn opener_count_changed(
        &self,
        _world: &Arc<World>,
        _pos: BlockPos,
        _block_state: BlockStateId,
        _previous: i32,
        _current: i32,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_registry::{
        data_components::vanilla_components::MAX_STACK_SIZE, item_stack::ItemStack,
        test_support::init_test_registry, vanilla_attributes, vanilla_blocks, vanilla_items,
    };
    use steel_utils::{ChunkPos, Downcast as _, types::UpdateFlags};
    use uuid::Uuid;

    use crate::behavior::init_behaviors;
    use crate::block_entity::init_block_entities;
    use crate::entity::{Entity as _, LivingEntity as _};
    use crate::inventory::container::Container as _;
    use crate::inventory::menu::kinds::chest_with_openers;
    use crate::player::ResetReason;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};

    use super::*;

    fn test_barrel() -> BarrelBlockEntity {
        init_test_registry();
        BarrelBlockEntity::new(
            Weak::new(),
            BlockPos::new(1, 2, 3),
            vanilla_blocks::BARREL.default_state(),
        )
    }

    #[test]
    fn set_item_honors_vanilla_ninety_nine_container_maximum() {
        let barrel = test_barrel();
        let mut stack = ItemStack::with_count(&vanilla_items::STONE, 100);
        stack.set(MAX_STACK_SIZE, 99);
        barrel.container.lock().set_item(0, stack);

        assert_eq!(barrel.container.lock().get_item(0).count(), 99);
    }

    #[test]
    fn pre_remove_preserves_slots_for_existing_menu_references() {
        let barrel = test_barrel();
        barrel
            .container
            .lock()
            .set_item(0, ItemStack::new(&vanilla_items::STONE));

        barrel.pre_remove_side_effects(
            BlockPos::new(1, 2, 3),
            vanilla_blocks::BARREL.default_state(),
        );

        let container = barrel.container.lock();
        assert_eq!(container.get_container_size(), BARREL_SLOTS);
        assert!(container.items().iter().all(ItemStack::is_empty));
    }

    #[test]
    fn menu_lifecycle_updates_open_state_and_viewer_count() {
        init_test_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("barrel_open_state");
        let pos = BlockPos::new(3, 64, 3);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos,
            vanilla_blocks::BARREL.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let Some(block_entity) = world.get_block_entity(pos) else {
            panic!("barrel placement should create its block entity");
        };
        let Some(barrel) = block_entity.downcast_ref::<BarrelBlockEntity>() else {
            panic!("barrel should use its concrete block entity");
        };
        let Some(container) = block_entity.container_ref() else {
            panic!("barrel should expose its inventory");
        };
        let player = TestPlayerBuilder::new(Arc::clone(&world), "BarrelViewer", 1)
            .uuid(Uuid::from_u128(1))
            .build();
        player.base().set_position_local(DVec3::new(3.5, 64.0, 3.5));
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        let inventory = Arc::clone(&player.inventory);
        let opener = Arc::clone(&block_entity);
        player.open_menu("Barrel", move |context| {
            chest_with_openers(
                inventory,
                context.container_id,
                vec![(container, BARREL_SLOTS)],
                3,
                vec![opener],
            )
        });

        assert!(
            world
                .get_block_state(pos)
                .get_value(&BlockStateProperties::OPEN)
        );
        assert_eq!(barrel.openers.opener_count(), 1);
        assert!(world.has_scheduled_block_tick(pos, &vanilla_blocks::BARREL));

        barrel.recheck_open();
        assert_eq!(barrel.openers.opener_count(), 1);

        player
            .attributes()
            .lock()
            .set_base_value(vanilla_attributes::BLOCK_INTERACTION_RANGE, 12.0);
        barrel.recheck_open();
        assert!(
            player
                .base()
                .try_set_position(DVec3::new(15.0, 64.0, 3.5))
                .is_ok()
        );
        barrel.recheck_open();
        assert_eq!(barrel.openers.opener_count(), 1);

        player
            .attributes()
            .lock()
            .set_base_value(vanilla_attributes::BLOCK_INTERACTION_RANGE, 1.0);
        barrel.recheck_open();
        assert_eq!(barrel.openers.opener_count(), 1);
        barrel.recheck_open();
        assert_eq!(barrel.openers.opener_count(), 0);
        assert!(
            !world
                .get_block_state(pos)
                .get_value(&BlockStateProperties::OPEN)
        );

        assert!(
            player
                .base()
                .try_set_position(DVec3::new(3.5, 64.0, 3.5))
                .is_ok()
        );
        barrel.recheck_open();
        assert_eq!(barrel.openers.opener_count(), 1);
        assert!(
            world
                .get_block_state(pos)
                .get_value(&BlockStateProperties::OPEN)
        );

        player.close_container();

        assert!(
            !world
                .get_block_state(pos)
                .get_value(&BlockStateProperties::OPEN)
        );
        assert_eq!(barrel.openers.opener_count(), 0);
    }
}
