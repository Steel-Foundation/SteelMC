//! Cartography table menu (map, additional, result).

use std::sync::Arc;

use glam::DVec3;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::data_components::components::MapPostProcessing;
use steel_registry::data_components::vanilla_components::{MAP_ID, MAP_POST_PROCESSING};
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_events;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_items;
use steel_registry::vanilla_menu_types;
use steel_utils::locks::{IntoShared, Shared};
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey};

use crate::behavior::items::MapItem;
use crate::inventory::container::{ResultContainer, SimpleContainer};
use crate::inventory::prelude::*;
use crate::inventory::slots::ResultHandler;
use crate::player::player_inventory::PlayerInventory;
use crate::world::World;

const MAP_SLOT: usize = 0;
const ADDITIONAL_SLOT: usize = 1;
const RESULT_SLOT: usize = 2;
const PLAYER_INV_START: usize = 3;
const PLAYER_INV_END: usize = 30;
const HOTBAR_START: usize = 30;
const HOTBAR_END: usize = 39;

/// Builds the cartography table menu.
#[must_use]
pub fn cartography(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    block_pos: BlockPos,
    world: &Arc<World>,
) -> Menu {
    let input_container = SimpleContainer::new(2).into_shared();
    let result_container = ResultContainer::new().into_shared();
    let handler = CartographyHandler {
        input_container: input_container.clone(),
        result_container: result_container.clone(),
        block_pos,
        world: Arc::clone(world),
    };

    let mut builder = MenuBuilder::new(&vanilla_menu_types::CARTOGRAPHY_TABLE, container_id);
    let mut inputs = builder.split(&input_container);
    let map = builder.section_with(
        &mut inputs,
        1,
        SectionKind::restricted(|_, stack| stack.has(MAP_ID)),
    );
    let additional = builder.section_with(
        &mut inputs,
        1,
        SectionKind::restricted(|_, stack| {
            stack.is(&vanilla_items::PAPER)
                || stack.is(&vanilla_items::MAP)
                || stack.is(&vanilla_items::GLASS_PANE)
        }),
    );
    let result = builder.result_slot(handler);
    let player = builder.player_inventory(&inventory);

    builder.route_with_remainder_policy(
        result,
        player.all(),
        FillDirection::Backward,
        FakeResultRemainderPolicy::Discard,
    );
    builder.route(map, player.all(), FillDirection::Forward);
    builder.route(additional, player.all(), FillDirection::Forward);
    builder.drain(map);
    builder.drain(additional);

    builder.build(CartographyKind {
        input_container,
        result_container,
        block_pos,
        world: Arc::clone(world),
        result,
    })
}

/// Per-menu cartography state.
pub struct CartographyKind {
    pub(crate) input_container: Shared<SimpleContainer>,
    pub(crate) result_container: Shared<ResultContainer>,
    block_pos: BlockPos,
    world: Arc<World>,
    result: Section,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl DowncastType for CartographyKind {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:menu/cartography");
}

impl CartographyKind {
    fn setup_result_slot(&self, guard: &mut ContainerLockGuard) {
        let (map_stack, additional) = {
            let input = guard
                .get_typed::<SimpleContainer>(ContainerId::from_arc(&self.input_container))
                .expect("cartography input is registered");
            (input.get_item(0).clone(), input.get_item(1).clone())
        };
        let result = compute_cartography_result(&map_stack, &additional, &self.world);
        guard
            .get_typed_mut::<ResultContainer>(ContainerId::from_arc(&self.result_container))
            .expect("cartography result is registered")
            .set_item(0, result);
    }
}

fn compute_cartography_result(
    map_stack: &ItemStack,
    additional: &ItemStack,
    world: &World,
) -> ItemStack {
    if map_stack.is_empty() || additional.is_empty() {
        return ItemStack::empty();
    }
    let (locked, scale) = {
        let store = world.map_data.lock();
        let Some(map_data) = MapItem::saved_data(map_stack, &store) else {
            return ItemStack::empty();
        };
        (map_data.locked, map_data.scale)
    };
    if additional.is(&vanilla_items::PAPER) && !locked && scale < 4 {
        let mut result = map_stack.copy_with_count(1);
        result.set(MAP_POST_PROCESSING, MapPostProcessing::Scale);
        result
    } else if additional.is(&vanilla_items::GLASS_PANE) && !locked {
        let mut result = map_stack.copy_with_count(1);
        result.set(MAP_POST_PROCESSING, MapPostProcessing::Lock);
        result
    } else if additional.is(&vanilla_items::MAP) {
        map_stack.copy_with_count(2)
    } else {
        ItemStack::empty()
    }
}

impl MenuKind for CartographyKind {
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        self.world.get_block_state(self.block_pos).get_block() == &vanilla_blocks::CARTOGRAPHY_TABLE
            && player.is_within_block_interaction_range_with_buffer(self.block_pos, 4.0)
    }

    fn can_take_item_for_pick_all(&self, _carried: &ItemStack, slot_index: usize) -> bool {
        !self.result.contains(slot_index)
    }

    fn removed(&mut self, _behavior: &mut MenuBehavior, _player: &Player) {
        self.result_container.lock().set_item(0, ItemStack::empty());
    }

    fn slots_changed(
        &mut self,
        _behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
        self.setup_result_slot(guard);
    }

    fn quick_move(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        slot_index: usize,
        player: &Player,
    ) -> Option<ItemStack> {
        let clicked = behavior.slots()[slot_index].get_item(guard).clone();
        if clicked.is_empty() {
            return Some(ItemStack::empty());
        }
        let mut remaining = clicked.clone();
        if slot_index == RESULT_SLOT {
            MapItem::apply_post_processing(&mut remaining, &self.world);
        }
        let moved = if slot_index == RESULT_SLOT {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                PLAYER_INV_START,
                HOTBAR_END,
                FillDirection::Backward,
            )
        } else if slot_index != MAP_SLOT && slot_index != ADDITIONAL_SLOT {
            if clicked.has(MAP_ID) {
                behavior.move_item_stack_to(
                    guard,
                    slot_index,
                    &mut remaining,
                    MAP_SLOT,
                    ADDITIONAL_SLOT,
                    FillDirection::Forward,
                )
            } else if clicked.is(&vanilla_items::PAPER)
                || clicked.is(&vanilla_items::MAP)
                || clicked.is(&vanilla_items::GLASS_PANE)
            {
                behavior.move_item_stack_to(
                    guard,
                    slot_index,
                    &mut remaining,
                    ADDITIONAL_SLOT,
                    RESULT_SLOT,
                    FillDirection::Forward,
                )
            } else if (PLAYER_INV_START..PLAYER_INV_END).contains(&slot_index) {
                behavior.move_item_stack_to(
                    guard,
                    slot_index,
                    &mut remaining,
                    HOTBAR_START,
                    HOTBAR_END,
                    FillDirection::Forward,
                )
            } else if (HOTBAR_START..HOTBAR_END).contains(&slot_index) {
                behavior.move_item_stack_to(
                    guard,
                    slot_index,
                    &mut remaining,
                    PLAYER_INV_START,
                    PLAYER_INV_END,
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
                PLAYER_INV_START,
                HOTBAR_END,
                FillDirection::Forward,
            )
        };
        if !moved {
            return Some(ItemStack::empty());
        }
        behavior.update_quick_move_source(guard, slot_index, &remaining, &clicked);
        if remaining.count == clicked.count {
            return Some(ItemStack::empty());
        }
        if let Some(remainder) = behavior.slots()[slot_index].on_take(guard, &mut remaining, player)
        {
            player.add_item_or_drop_with_guard(guard, remainder);
        }
        Some(clicked)
    }
}

struct CartographyHandler {
    input_container: Shared<SimpleContainer>,
    result_container: Shared<ResultContainer>,
    block_pos: BlockPos,
    world: Arc<World>,
}

impl ResultHandler for CartographyHandler {
    fn result_container(&self) -> ContainerRef {
        ContainerRef::from(self.result_container.clone())
    }

    fn dependencies(&self) -> Vec<ContainerRef> {
        vec![ContainerRef::from(self.input_container.clone())]
    }

    fn update_result(&self, _guard: &mut ContainerLockGuard) {}

    fn on_result_taken(
        &self,
        guard: &mut ContainerLockGuard,
        _player: &Player,
    ) -> Option<ItemStack> {
        let input = guard
            .get_typed_mut::<SimpleContainer>(ContainerId::from_arc(&self.input_container))
            .expect("cartography input is registered");
        let _ = input.remove_item(0, 1);
        let _ = input.remove_item(1, 1);

        self.world.play_sound_at(
            &sound_events::UI_CARTOGRAPHY_TABLE_TAKE_RESULT,
            SoundSource::Blocks,
            DVec3::new(
                f64::from(self.block_pos.x()) + 0.5,
                f64::from(self.block_pos.y()) + 0.5,
                f64::from(self.block_pos.z()) + 0.5,
            ),
            1.0,
            1.0,
            None,
        );
        None
    }

    fn is_result_valid(&self, guard: &ContainerLockGuard, _player: &Player) -> bool {
        !guard
            .get_typed::<ResultContainer>(ContainerId::from_arc(&self.result_container))
            .expect("cartography result is registered")
            .get_item(0)
            .is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{CartographyKind, cartography};
    use crate::{
        behavior::{init_behaviors, items::MapItem},
        entity::Entity as _,
        inventory::{
            click::{Click, MouseButton},
            container::Container as _,
        },
        test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk},
    };
    use glam::DVec3;
    use steel_registry::{
        data_components::components::MapPostProcessing,
        data_components::vanilla_components::{MAP_ID, MAP_POST_PROCESSING},
        init_vanilla_registry,
        item_stack::ItemStack,
        vanilla_blocks, vanilla_items,
    };
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos, Downcast as _};

    fn setup_cartography() -> (
        Arc<crate::world::World>,
        Arc<crate::player::Player>,
        crate::inventory::menu::Menu,
        ItemStack,
    ) {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("cartography");
        let pos = BlockPos::new(0, 64, 0);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos,
            vanilla_blocks::CARTOGRAPHY_TABLE.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        let player = TestPlayerBuilder::new(Arc::clone(&world), "Mapper", 1).build();
        player.base().set_position_local(DVec3::new(0.5, 64.0, 0.5));
        let menu = cartography(Arc::clone(&player.inventory), 1, pos, &world);
        let map = MapItem::create(&world, 0, 0, 0, true, false);
        (world, player, menu, map)
    }

    #[test]
    fn paper_preview_marks_scale_without_allocating_a_map_id() {
        let (world, player, mut menu, map) = setup_cartography();
        let original_id = *map.get(MAP_ID).expect("created map has an id");

        *menu.behavior_mut().carried_mut() = map;
        menu.clicked(
            Click::Pickup {
                slot: 0,
                button: MouseButton::Left,
            },
            &player,
        );
        *menu.behavior_mut().carried_mut() = ItemStack::new(&vanilla_items::PAPER);
        menu.clicked(
            Click::Pickup {
                slot: 1,
                button: MouseButton::Left,
            },
            &player,
        );

        let kind = menu.kind().downcast_ref::<CartographyKind>().unwrap();
        let result = kind.result_container.lock().get_item(0).clone();
        assert_eq!(result.get(MAP_ID), Some(&original_id));
        assert_eq!(
            result.get(MAP_POST_PROCESSING),
            Some(&MapPostProcessing::Scale)
        );

        let extra = MapItem::create(&world, 0, 0, 0, true, false);
        assert_eq!(
            extra.get(MAP_ID).map(|id| id.id()),
            Some(original_id.id() + 1)
        );
    }

    #[test]
    fn taking_a_scaled_map_allocates_a_new_id() {
        let (world, player, mut menu, map) = setup_cartography();
        let original_id = *map.get(MAP_ID).expect("created map has an id");

        *menu.behavior_mut().carried_mut() = map;
        menu.clicked(
            Click::Pickup {
                slot: 0,
                button: MouseButton::Left,
            },
            &player,
        );
        *menu.behavior_mut().carried_mut() = ItemStack::new(&vanilla_items::PAPER);
        menu.clicked(
            Click::Pickup {
                slot: 1,
                button: MouseButton::Left,
            },
            &player,
        );
        menu.clicked(
            Click::Pickup {
                slot: 2,
                button: MouseButton::Left,
            },
            &player,
        );

        let taken = menu.behavior().carried().clone();
        let taken_id = *taken.get(MAP_ID).expect("scaled map has an id");
        assert_ne!(taken_id, original_id);
        assert!(taken.get(MAP_POST_PROCESSING).is_none());
        let store = world.map_data.lock();
        let data = store.get(taken_id).expect("scaled map data exists");
        assert_eq!(data.scale, 1);
        assert!(!data.locked);
    }

    #[test]
    fn cloning_a_map_keeps_the_same_id() {
        let (_world, player, mut menu, map) = setup_cartography();
        let original_id = *map.get(MAP_ID).expect("created map has an id");

        *menu.behavior_mut().carried_mut() = map;
        menu.clicked(
            Click::Pickup {
                slot: 0,
                button: MouseButton::Left,
            },
            &player,
        );
        *menu.behavior_mut().carried_mut() = ItemStack::new(&vanilla_items::MAP);
        menu.clicked(
            Click::Pickup {
                slot: 1,
                button: MouseButton::Left,
            },
            &player,
        );

        let kind = menu.kind().downcast_ref::<CartographyKind>().unwrap();
        let result = kind.result_container.lock().get_item(0).clone();
        assert_eq!(result.count(), 2);
        assert_eq!(result.get(MAP_ID), Some(&original_id));
        assert!(result.get(MAP_POST_PROCESSING).is_none());
    }
}
