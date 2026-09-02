//! Standard chest block entity implementation.

use std::sync::{Arc, Weak};

use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::NbtCompound;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, ChestType};
use steel_registry::data_components::DataComponentMap;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{
    BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex,
    translations::CONTAINER_CHEST,
};
use text_components::TextComponent;

use crate::behavior::BLOCK_BEHAVIORS;
use crate::behavior::blocks::ChestBlock;
use crate::block_entity::randomizable_container::RandomizableContainer;
use crate::block_entity::{
    BlockEntity, BlockEntityBase, BlockEntityComponentInput, ContainerOpeners,
    ContainerOpenersCounter,
};
use crate::inventory::lock::{ContainerId, ContainerRef, SharedContainer};
use crate::player::Player;
use crate::world::World;

/// Number of slots in one standard chest half.
pub const CHEST_SLOTS: usize = 27;

/// Inventory and viewer state for one standard chest block.
pub struct ChestBlockEntity {
    base: Arc<BlockEntityBase>,
    container: Arc<SyncMutex<RandomizableContainer>>,
    container_ref: ContainerRef,
    openers: ContainerOpenersCounter,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ChestBlockEntity`.
unsafe impl DowncastType for ChestBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/chest");
}

impl ChestBlockEntity {
    /// Creates one standard chest half.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        let base = Arc::new(BlockEntityBase::new(
            &vanilla_block_entity_types::CHEST,
            level,
            pos,
            state,
        ));
        let container = Arc::new(SyncMutex::new(RandomizableContainer::new(CHEST_SLOTS)));
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
            .display_name(TextComponent::translated(CONTAINER_CHEST.msg()))
    }

    /// Returns whether this chest half has a custom menu title.
    #[must_use]
    pub fn has_custom_name(&self) -> bool {
        self.container.lock().has_custom_name()
    }

    /// Returns whether `player` may open this chest half.
    #[must_use]
    pub fn can_open(&self, player: &Player) -> bool {
        let main_hand = player.get_main_hand_item();
        self.container.lock().can_open(player, &main_hand)
    }

    fn configured_sound(state: BlockStateId, opening: bool) -> Option<SoundEventRef> {
        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        if opening {
            behavior.chest_open_sound()
        } else {
            behavior.chest_close_sound()
        }
    }

    fn play_sound(&self, world: &World, state: BlockStateId, opening: bool) {
        let chest_type = state.get_value(&BlockStateProperties::CHEST_TYPE);
        if chest_type == ChestType::Left {
            return;
        }
        let Some(sound) = Self::configured_sound(state, opening) else {
            return;
        };
        let pos = self.get_block_pos();
        let mut x = f64::from(pos.x()) + 0.5;
        let y = f64::from(pos.y()) + 0.5;
        let mut z = f64::from(pos.z()) + 0.5;
        if chest_type == ChestType::Right {
            let (offset_x, offset_z) = ChestBlock::connected_direction(state).offset_xz();
            x += f64::from(offset_x) * 0.5;
            z += f64::from(offset_z) * 0.5;
        }
        world.play_sound_at(
            sound,
            SoundSource::Blocks,
            glam::DVec3::new(x, y, z),
            0.5,
            rand::random::<f32>() * 0.1 + 0.9,
            None,
        );
    }
}

impl BlockEntity for ChestBlockEntity {
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

    fn trigger_event(&self, param_a: i32, _param_b: i32) -> bool {
        param_a == 1
    }

    fn container_ref(&self) -> Option<ContainerRef> {
        Some(self.container_ref.clone())
    }

    fn container_openers(&self) -> Option<&dyn ContainerOpeners> {
        Some(self)
    }
}

impl ContainerOpeners for ChestBlockEntity {
    fn openers_counter(&self) -> &ContainerOpenersCounter {
        &self.openers
    }

    fn opener_container_id(&self) -> ContainerId {
        self.container_ref.container_id()
    }

    fn on_open(&self, world: &Arc<World>, _pos: BlockPos, block_state: BlockStateId) {
        self.play_sound(world, block_state, true);
    }

    fn on_close(&self, world: &Arc<World>, _pos: BlockPos, block_state: BlockStateId) {
        self.play_sound(world, block_state, false);
    }

    fn opener_count_changed(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        block_state: BlockStateId,
        _previous: i32,
        current: i32,
    ) {
        world.block_event(pos, block_state.get_block(), 1, current);
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use simdnbt::owned::NbtCompound;
    use steel_registry::blocks::properties::{BlockStateProperties, ChestType, Direction};
    use steel_registry::data_component_predicate::DataComponentMatchers;
    use steel_registry::data_components::CustomData;
    use steel_registry::data_components::vanilla_components::{
        BLOCK_ENTITY_DATA, CONTAINER, CUSTOM_DATA, CUSTOM_NAME, ItemContainerContents, LOCK,
    };
    use steel_registry::item_predicate::{IntBounds, ItemPredicate, LockCode};
    use steel_registry::{
        RegistryHolderSet, item_stack::ItemStack, test_support::init_test_registry, vanilla_blocks,
        vanilla_entities, vanilla_items,
    };
    use steel_utils::{ChunkPos, Downcast as _, WorldAabb, types::UpdateFlags};
    use uuid::Uuid;

    use crate::behavior::items::BlockItem;
    use crate::behavior::{BlockPlaceContext, InteractionResult, init_behaviors};
    use crate::block_entity::{BlockEntityComponentsExt as _, init_block_entities};
    use crate::entity::Entity as _;
    use crate::entity::entities::ItemEntity;
    use crate::inventory::container::Container as _;
    use crate::inventory::lock::ContainerLockGuard;
    use crate::inventory::menu::kinds::chest_with_openers;
    use crate::player::ResetReason;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};

    use super::*;

    #[test]
    fn placed_chest_items_apply_their_components_and_pick_block_collects_them() {
        init_test_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("chest_item_components");
        let pos = BlockPos::new(3, 64, 3);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let name = TextComponent::plain("Treasure");
        let mut custom_data = NbtCompound::new();
        custom_data.insert("steel_test", 1_i32);
        let mut stack = ItemStack::new(&vanilla_items::CHEST);
        stack.set(CUSTOM_NAME, name.clone());
        stack.set(
            LOCK,
            LockCode::new(ItemPredicate::new(
                Some(RegistryHolderSet::direct(vec![
                    &vanilla_items::TRIPWIRE_HOOK,
                ])),
                IntBounds::ANY,
                DataComponentMatchers::ANY,
            )),
        );
        stack.set(
            CONTAINER,
            ItemContainerContents::from_items(&[
                ItemStack::empty(),
                ItemStack::with_count(&vanilla_items::DIAMOND, 3),
            ])
            .expect("test container contents should persist"),
        );
        stack.set(
            CUSTOM_DATA,
            CustomData::try_from_compound(custom_data).expect("test custom data should be valid"),
        );

        let context =
            BlockPlaceContext::directional(&world, pos, Direction::Up, &mut stack, Direction::Up);
        assert_eq!(
            BlockItem::new(&vanilla_blocks::CHEST).place(context),
            InteractionResult::Success
        );
        assert!(stack.is_empty());
        let Some(block_entity) = world.get_block_entity(pos) else {
            panic!("chest placement should create its block entity");
        };
        let Some(chest) = block_entity.downcast_ref::<ChestBlockEntity>() else {
            panic!("chest should use its concrete block entity");
        };
        let Some(container_ref) = block_entity.container_ref() else {
            panic!("chest should expose its inventory");
        };

        assert_eq!(chest.display_name(), name);
        {
            let guard = ContainerLockGuard::lock_all(&[&container_ref]);
            let Some(container) = guard.get(container_ref.container_id()) else {
                panic!("chest inventory should be lockable");
            };
            assert!(container.get_item(0).is_empty());
            assert!(container.get_item(1).is(&vanilla_items::DIAMOND));
            assert_eq!(container.get_item(1).count(), 3);
        }
        assert!(
            block_entity.base().components().has(CUSTOM_DATA),
            "components no field consumes stay explicit block-entity components"
        );
        let player =
            TestPlayerBuilder::new(Arc::clone(&world), "Looter", 1)
                .uuid(Uuid::from_u128(1))
                .build();
        player.base().set_position_local(DVec3::new(3.5, 64.0, 3.5));
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        assert!(!chest.can_open(&player), "the item's lock must apply");

        let mut picked = ItemStack::new(&vanilla_items::CHEST);
        block_entity.add_block_data_to_item(&mut picked);
        assert_eq!(picked.get(CUSTOM_NAME), Some(&name));
        assert!(picked.get(LOCK).is_some());
        assert_eq!(
            picked
                .get(CONTAINER)
                .and_then(|contents| contents.items().get(1))
                .and_then(Option::as_ref)
                .map(|diamonds| (diamonds.item(), diamonds.count())),
            Some((&*vanilla_items::DIAMOND, 3))
        );
        assert!(picked.get(CUSTOM_DATA).is_some());
        assert!(
            picked.get(BLOCK_ENTITY_DATA).is_none(),
            "every saved chest field is represented by a component"
        );
    }

    #[test]
    fn breaking_a_named_chest_drops_a_named_chest_item() {
        init_test_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("named_chest_drop");
        let pos = BlockPos::new(3, 64, 3);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        let name = TextComponent::plain("Keepsake");
        let mut stack = ItemStack::new(&vanilla_items::CHEST);
        stack.set(CUSTOM_NAME, name.clone());
        let context =
            BlockPlaceContext::directional(&world, pos, Direction::Up, &mut stack, Direction::Up);
        assert_eq!(
            BlockItem::new(&vanilla_blocks::CHEST).place(context),
            InteractionResult::Success
        );

        assert!(world.destroy_block(pos, true));

        let min = DVec3::new(
            f64::from(pos.x()) - 1.0,
            f64::from(pos.y()) - 1.0,
            f64::from(pos.z()) - 1.0,
        );
        let dropped = world.get_entities_in_aabb_matching(
            &WorldAabb::new(min.x, min.y, min.z, min.x + 3.0, min.y + 3.0, min.z + 3.0),
            |entity| entity.entity_type() == &vanilla_entities::ITEM,
        );
        let chest_items = dropped
            .iter()
            .filter_map(|entity| entity.as_ref().downcast_ref::<ItemEntity>())
            .map(ItemEntity::get_item)
            .filter(|item| item.is(&vanilla_items::CHEST))
            .collect::<Vec<_>>();
        assert_eq!(chest_items.len(), 1);
        assert_eq!(chest_items[0].get(CUSTOM_NAME), Some(&name));
    }

    fn test_chest() -> ChestBlockEntity {
        init_test_registry();
        ChestBlockEntity::new(
            Weak::new(),
            BlockPos::new(1, 2, 3),
            vanilla_blocks::CHEST.default_state(),
        )
    }

    #[test]
    fn pre_remove_preserves_slots_for_existing_menu_references() {
        let chest = test_chest();
        chest
            .container
            .lock()
            .set_item(0, ItemStack::new(&vanilla_items::STONE));

        chest.pre_remove_side_effects(
            BlockPos::new(1, 2, 3),
            vanilla_blocks::CHEST.default_state(),
        );

        let container = chest.container.lock();
        assert_eq!(container.get_container_size(), CHEST_SLOTS);
        assert!(container.items().iter().all(ItemStack::is_empty));
    }

    #[test]
    fn double_chest_menu_owns_both_viewer_counters() {
        init_test_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("double_chest_openers");
        let right_pos = BlockPos::new(3, 64, 3);
        let left_pos = right_pos.west();
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(right_pos));
        let base_state = vanilla_blocks::CHEST
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, Direction::North);
        let right_state = base_state.set_value(&BlockStateProperties::CHEST_TYPE, ChestType::Right);
        let left_state = base_state.set_value(&BlockStateProperties::CHEST_TYPE, ChestType::Left);
        assert!(world.set_block(right_pos, right_state, UpdateFlags::UPDATE_NONE));
        assert!(world.set_block(left_pos, left_state, UpdateFlags::UPDATE_NONE));
        let Some(right_entity) = world.get_block_entity(right_pos) else {
            panic!("right chest half should create its block entity");
        };
        let Some(left_entity) = world.get_block_entity(left_pos) else {
            panic!("left chest half should create its block entity");
        };
        let Some(right) = right_entity.downcast_ref::<ChestBlockEntity>() else {
            panic!("right chest half should use the concrete chest entity");
        };
        let Some(left) = left_entity.downcast_ref::<ChestBlockEntity>() else {
            panic!("left chest half should use the concrete chest entity");
        };
        let Some(right_container) = right_entity.container_ref() else {
            panic!("right chest half should expose its inventory");
        };
        let Some(left_container) = left_entity.container_ref() else {
            panic!("left chest half should expose its inventory");
        };
        let player =
            TestPlayerBuilder::new(Arc::clone(&world), "ChestViewer", 1)
                .uuid(Uuid::from_u128(1))
                .build();
        player.base().set_position_local(DVec3::new(3.5, 64.0, 3.5));
        let inventory = Arc::clone(&player.inventory);
        let right_opener = Arc::clone(&right_entity);
        let left_opener = Arc::clone(&left_entity);
        player.open_menu("Large Chest", move |context| {
            chest_with_openers(
                inventory,
                context.container_id,
                vec![
                    (right_container, CHEST_SLOTS),
                    (left_container, CHEST_SLOTS),
                ],
                6,
                vec![right_opener, left_opener],
            )
        });

        assert_eq!(right.openers.opener_count(), 1);
        assert_eq!(left.openers.opener_count(), 1);
        assert!(world.has_scheduled_block_tick(right_pos, &vanilla_blocks::CHEST));
        assert!(world.has_scheduled_block_tick(left_pos, &vanilla_blocks::CHEST));

        player.close_container();

        assert_eq!(right.openers.opener_count(), 0);
        assert_eq!(left.openers.opener_count(), 0);
    }
}
