//! Normal furnace menu.

use steel_registry::{REGISTRY, item_stack::ItemStack, vanilla_menu_types};
use steel_utils::{DowncastType, DowncastTypeKey, locks::Shared};

use crate::block_entity::entities::{
    FURNACE_FUEL_SLOT, FURNACE_INPUT_SLOT, FURNACE_RESULT_SLOT, FurnaceContainer,
};
use crate::block_entity::vanilla_fuel_values;
use crate::inventory::prelude::*;
use crate::inventory::slots::{FurnaceFuelSlot, FurnaceResultSlot};
use crate::player::player_inventory::PlayerInventory;

/// Builds the normal furnace menu over one furnace block-entity container.
#[must_use]
pub fn furnace(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    container: ContainerRef,
) -> Menu {
    // TODO: Add Vanilla recipe-book placement once Steel has shared recipe-book
    // state, packet handling, and server-side placement support.
    let mut builder = MenuBuilder::new(&vanilla_menu_types::FURNACE, container_id);
    let input = builder.section_at(&container, [FURNACE_INPUT_SLOT], SectionKind::Normal);
    let fuel = builder.section_at(
        &container,
        [FURNACE_FUEL_SLOT],
        SectionKind::custom(|container, index| Box::new(FurnaceFuelSlot::new(container, index))),
    );
    let result = builder.section_at(
        &container,
        [FURNACE_RESULT_SLOT],
        SectionKind::custom(|container, index| Box::new(FurnaceResultSlot::new(container, index))),
    );
    let player = builder.player_inventory(&inventory);
    let data = [
        builder.data_slot(0),
        builder.data_slot(0),
        builder.data_slot(0),
        builder.data_slot(0),
    ];

    builder.build(FurnaceKind {
        container_id: container.container_id(),
        container,
        input,
        fuel,
        result,
        player_main: player.main(),
        player_hotbar: player.hotbar(),
        player_all: player.all(),
        data,
    })
}

/// Per-menu normal-furnace routing and synchronized progress data.
pub struct FurnaceKind {
    container_id: ContainerId,
    container: ContainerRef,
    input: Section,
    fuel: Section,
    result: Section,
    player_main: Section,
    player_hotbar: Section,
    player_all: Section,
    data: [DataSlot; 4],
}

// SAFETY: This Steel-owned key uniquely identifies the normal furnace menu kind.
unsafe impl DowncastType for FurnaceKind {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:menu/furnace");
}

impl FurnaceKind {
    fn sync_data(&self, behavior: &mut MenuBehavior, guard: &ContainerLockGuard) {
        let Some(furnace) = guard.get_typed::<FurnaceContainer>(self.container_id) else {
            return;
        };
        let values = furnace.menu_data();
        for (slot, value) in self.data.into_iter().zip(values) {
            slot.set(behavior, value);
        }
    }

    fn can_smelt(stack: &ItemStack) -> bool {
        REGISTRY.recipes.find_smelting_recipe(stack).is_some()
    }
}

impl MenuKind for FurnaceKind {
    fn on_open(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
        self.sync_data(behavior, guard);
    }

    fn on_tick(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
        self.sync_data(behavior, guard);
    }

    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        self.container.still_valid(player)
    }

    /// Mirrors `AbstractFurnaceMenu.quickMoveStack` route ordering.
    fn quick_move(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        slot_index: usize,
        player: &Player,
    ) -> Option<ItemStack> {
        if slot_index >= behavior.slots().len() {
            return Some(ItemStack::empty());
        }
        let stack = behavior.slots()[slot_index].get_item(guard).clone();
        if stack.is_empty() {
            return Some(ItemStack::empty());
        }

        let clicked = stack.clone();
        let mut remaining = stack;
        let moved = if self.result.contains(slot_index) {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                self.player_all.start(),
                self.player_all.end(),
                FillDirection::Backward,
            )
        } else if !self.input.contains(slot_index) && !self.fuel.contains(slot_index) {
            if Self::can_smelt(&remaining) {
                behavior.move_item_stack_to(
                    guard,
                    slot_index,
                    &mut remaining,
                    self.input.start(),
                    self.input.end(),
                    FillDirection::Forward,
                )
            } else if vanilla_fuel_values().is_fuel(&remaining) {
                behavior.move_item_stack_to(
                    guard,
                    slot_index,
                    &mut remaining,
                    self.fuel.start(),
                    self.fuel.end(),
                    FillDirection::Forward,
                )
            } else if self.player_main.contains(slot_index) {
                behavior.move_item_stack_to(
                    guard,
                    slot_index,
                    &mut remaining,
                    self.player_hotbar.start(),
                    self.player_hotbar.end(),
                    FillDirection::Forward,
                )
            } else if self.player_hotbar.contains(slot_index) {
                behavior.move_item_stack_to(
                    guard,
                    slot_index,
                    &mut remaining,
                    self.player_main.start(),
                    self.player_main.end(),
                    FillDirection::Forward,
                )
            } else {
                false
            }
        } else {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                self.player_all.start(),
                self.player_all.end(),
                FillDirection::Forward,
            )
        };

        if !moved {
            return Some(ItemStack::empty());
        }
        behavior.update_quick_move_source(guard, slot_index, &remaining, &clicked);
        if remaining.count() == clicked.count() {
            return Some(ItemStack::empty());
        }
        if let Some(remainder) = behavior.slots()[slot_index].on_take(guard, &remaining, player) {
            player.add_item_or_drop_with_guard(guard, remainder);
        }
        Some(clicked)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::Cursor,
        slice,
        sync::{Arc, Weak},
    };

    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use simdnbt::owned::NbtCompound;
    use steel_registry::{
        test_support::init_test_registry, vanilla_blocks, vanilla_entities, vanilla_items,
    };
    use steel_utils::{BlockPos, ChunkPos, Downcast as _, WorldAabb};
    use uuid::Uuid;

    use super::*;
    use crate::block_entity::BlockEntity as _;
    use crate::block_entity::entities::FurnaceBlockEntity;
    use crate::entity::entities::ExperienceOrbEntity;
    use crate::inventory::click::{Click, MouseButton};
    use crate::inventory::container::Container as _;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};

    fn test_menu(key: &'static str) -> (Arc<Player>, ContainerRef, Menu) {
        init_test_registry();
        let world = fresh_test_world(key);
        let player = TestPlayerBuilder::new(world, "Smelter", 1)
            .uuid(Uuid::from_u128(1))
            .build();
        let block_entity = FurnaceBlockEntity::new(
            Weak::new(),
            BlockPos::ZERO,
            vanilla_blocks::FURNACE.default_state(),
        );
        let Some(container) = block_entity.container_ref() else {
            panic!("furnace should expose its container");
        };
        let menu = furnace(Arc::clone(&player.inventory), 1, container.clone());
        (player, container, menu)
    }

    #[test]
    fn quick_move_prefers_smelting_input_then_fuel() {
        let (player, container, mut menu) = test_menu("furnace_quick_move");
        let container_id = container.container_id();
        {
            let mut inventory = player.inventory.lock();
            inventory.set_item(9, ItemStack::new(&vanilla_items::RAW_IRON));
            inventory.set_item(10, ItemStack::new(&vanilla_items::COAL));
        }

        menu.clicked(Click::QuickMove { slot: 3 }, &player);
        menu.clicked(Click::QuickMove { slot: 4 }, &player);

        let guard = ContainerLockGuard::lock_all(&[container]);
        let Some(furnace) = guard.get_typed::<FurnaceContainer>(container_id) else {
            panic!("menu should retain the concrete furnace container");
        };
        assert!(
            furnace
                .get_item(FURNACE_INPUT_SLOT)
                .is(&vanilla_items::RAW_IRON)
        );
        assert!(furnace.get_item(FURNACE_FUEL_SLOT).is(&vanilla_items::COAL));
    }

    #[test]
    fn full_smelting_input_does_not_fallback_to_the_hotbar() {
        let (player, container, mut menu) = test_menu("furnace_full_input_quick_move");
        let container_id = container.container_id();
        {
            let mut guard = ContainerLockGuard::lock_all(slice::from_ref(&container));
            let Some(furnace) = guard.get_typed_mut::<FurnaceContainer>(container_id) else {
                panic!("test container should be a furnace");
            };
            furnace.set_item(
                FURNACE_INPUT_SLOT,
                ItemStack::with_count(&vanilla_items::RAW_IRON, 64),
            );
        }
        player
            .inventory
            .lock()
            .set_item(9, ItemStack::new(&vanilla_items::RAW_IRON));

        menu.clicked(Click::QuickMove { slot: 3 }, &player);

        let inventory = player.inventory.lock();
        assert!(inventory.get_item(9).is(&vanilla_items::RAW_IRON));
        assert!((0..9).all(|slot| inventory.get_item(slot).is_empty()));
    }

    #[test]
    fn empty_buckets_are_limited_to_one_in_the_fuel_slot() {
        let (_player, container, menu) = test_menu("furnace_bucket_limit");
        let mut guard = ContainerLockGuard::lock_all(&[container]);
        let remainder = menu.behavior().slots()[1].safe_insert(
            &mut guard,
            ItemStack::with_count(&vanilla_items::BUCKET, 16),
            16,
        );

        assert_eq!(menu.behavior().slots()[1].get_item(&guard).count(), 1);
        assert_eq!(remainder.count(), 15);
    }

    #[test]
    fn opening_copies_all_four_furnace_data_values() {
        init_test_registry();
        let world = fresh_test_world("furnace_menu_data");
        let player = TestPlayerBuilder::new(world, "DataSmelter", 1)
            .uuid(Uuid::from_u128(2))
            .build();
        let block_entity = FurnaceBlockEntity::new(
            Weak::new(),
            BlockPos::ZERO,
            vanilla_blocks::FURNACE.default_state(),
        );
        let mut source = NbtCompound::new();
        source.insert("lit_time_remaining", 321_i16);
        source.insert("lit_total_time", 1_600_i16);
        source.insert("cooking_time_spent", 123_i16);
        source.insert("cooking_total_time", 200_i16);
        let mut bytes = Vec::new();
        source.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test furnace data should reborrow");
        block_entity.load_additional(&borrowed);
        let Some(container) = block_entity.container_ref() else {
            panic!("furnace should expose its container");
        };
        let mut menu = furnace(Arc::clone(&player.inventory), 1, container);

        menu.on_open(&player);

        let Some(kind) = menu.kind().downcast_ref::<FurnaceKind>() else {
            panic!("furnace builder should create a furnace menu");
        };
        assert_eq!(
            kind.data.map(|data| data.get(menu.behavior())),
            [321, 1_600, 123, 200]
        );
    }

    #[test]
    fn taking_result_pays_and_clears_accumulated_recipe_experience() {
        init_test_registry();
        let world = fresh_test_world("furnace_result_experience");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let player = TestPlayerBuilder::new(Arc::clone(&world), "ExperiencedSmelter", 1)
            .uuid(Uuid::from_u128(3))
            .build();
        let block_entity = FurnaceBlockEntity::new(
            Arc::downgrade(&world),
            BlockPos::ZERO,
            vanilla_blocks::FURNACE.default_state(),
        );
        let mut source = NbtCompound::new();
        let mut recipes = NbtCompound::new();
        recipes.insert("minecraft:gold_ingot_from_smelting_raw_gold", 7_i32);
        source.insert("RecipesUsed", recipes);
        let mut bytes = Vec::new();
        source.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test furnace recipes should reborrow");
        block_entity.load_additional(&borrowed);
        let Some(container) = block_entity.container_ref() else {
            panic!("furnace should expose its container");
        };
        let container_id = container.container_id();
        {
            let mut guard = ContainerLockGuard::lock_all(slice::from_ref(&container));
            assert!(guard.set_item(
                container_id,
                FURNACE_RESULT_SLOT,
                ItemStack::new(&vanilla_items::GOLD_INGOT),
            ));
        }
        let mut menu = furnace(Arc::clone(&player.inventory), 1, container);

        menu.clicked(
            Click::Pickup {
                slot: 2,
                button: MouseButton::Left,
            },
            &player,
        );

        assert!(menu.behavior().carried().is(&vanilla_items::GOLD_INGOT));
        let orbs = world.get_entities_in_aabb_matching(
            &WorldAabb::new(-4.0, -4.0, -4.0, 4.0, 4.0, 4.0),
            |entity| entity.entity_type() == &vanilla_entities::EXPERIENCE_ORB,
        );
        let mut experience = 0;
        for orb in orbs {
            let Some(orb) = orb.as_ref().downcast_ref::<ExperienceOrbEntity>() else {
                panic!("experience entity should retain its concrete type");
            };
            experience += orb.value();
        }
        assert_eq!(experience, 7);

        let mut saved = NbtCompound::new();
        block_entity.save_additional(&mut saved);
        assert!(
            saved
                .compound("RecipesUsed")
                .is_some_and(NbtCompound::is_empty)
        );
    }
}
