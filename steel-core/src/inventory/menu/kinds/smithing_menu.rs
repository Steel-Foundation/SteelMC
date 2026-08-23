//! Smithing table menu (template, base, addition, result).

use std::sync::Arc;

use steel_registry::REGISTRY;
use steel_registry::item_stack::ItemStack;
use steel_registry::level_events;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_menu_types;
use steel_utils::locks::{IntoShared, Shared};
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey};

use crate::inventory::container::{ResultContainer, SimpleContainer};
use crate::inventory::prelude::*;
use crate::inventory::slots::ResultHandler;
use crate::player::player_inventory::PlayerInventory;
use crate::world::World;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;

const TEMPLATE_SLOT: usize = 0;
const RESULT_SLOT: usize = 3;
const PLAYER_INV_START: usize = 4;
const PLAYER_INV_END: usize = 31;
const HOTBAR_START: usize = 31;
const HOTBAR_END: usize = 40;

/// Builds the smithing table menu.
#[must_use]
pub fn smithing(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    block_pos: BlockPos,
    world: &Arc<World>,
) -> Menu {
    let input_container = SimpleContainer::new(3).into_shared();
    let result_container = ResultContainer::new().into_shared();
    let handler = SmithingHandler {
        input_container: input_container.clone(),
        result_container: result_container.clone(),
        block_pos,
        world: Arc::clone(world),
    };

    let mut builder = MenuBuilder::new(&vanilla_menu_types::SMITHING, container_id);
    let mut inputs = builder.split(&input_container);
    let template = builder.section_with(
        &mut inputs,
        1,
        SectionKind::restricted(|_, stack| REGISTRY.recipes.smithing_accepts_template(stack)),
    );
    let base = builder.section_with(
        &mut inputs,
        1,
        SectionKind::restricted(|_, stack| REGISTRY.recipes.smithing_accepts_base(stack)),
    );
    let addition = builder.section_with(
        &mut inputs,
        1,
        SectionKind::restricted(|_, stack| REGISTRY.recipes.smithing_accepts_addition(stack)),
    );
    let result = builder.result_slot(handler);
    let player = builder.player_inventory(&inventory);
    let has_recipe_error = builder.data_slot(0);

    builder.route_with_remainder_policy(
        result,
        player.all(),
        FillDirection::Backward,
        FakeResultRemainderPolicy::Discard,
    );
    builder.route(template, player.all(), FillDirection::Forward);
    builder.route(base, player.all(), FillDirection::Forward);
    builder.route(addition, player.all(), FillDirection::Forward);
    builder.drain(template);
    builder.drain(base);
    builder.drain(addition);

    builder.build(SmithingKind {
        input_container,
        result_container,
        block_pos,
        world: Arc::clone(world),
        result,
        has_recipe_error,
    })
}

/// Per-menu smithing state.
pub struct SmithingKind {
    pub(crate) input_container: Shared<SimpleContainer>,
    pub(crate) result_container: Shared<ResultContainer>,
    block_pos: BlockPos,
    world: Arc<World>,
    result: Section,
    has_recipe_error: DataSlot,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl DowncastType for SmithingKind {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:menu/smithing");
}

impl SmithingKind {
    fn create_result(&self, behavior: &mut MenuBehavior, guard: &mut ContainerLockGuard) {
        let (template, base, addition) = {
            let input = guard
                .get_typed::<SimpleContainer>(ContainerId::from_arc(&self.input_container))
                .expect("smithing input is registered");
            (
                input.get_item(0).clone(),
                input.get_item(1).clone(),
                input.get_item(2).clone(),
            )
        };
        let result = REGISTRY
            .recipes
            .find_smithing_recipe(&template, &base, &addition)
            .map_or_else(ItemStack::empty, |recipe| {
                recipe.assemble(&template, &base, &addition)
            });
        guard
            .get_typed_mut::<ResultContainer>(ContainerId::from_arc(&self.result_container))
            .expect("smithing result is registered")
            .set_item(0, result.clone());

        let has_error =
            !template.is_empty() && !base.is_empty() && !addition.is_empty() && result.is_empty();
        self.has_recipe_error.set(behavior, i16::from(has_error));
    }
}

impl MenuKind for SmithingKind {
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        self.world.get_block_state(self.block_pos).get_block() == &vanilla_blocks::SMITHING_TABLE
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
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
        self.create_result(behavior, guard);
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
        let moved = if slot_index == RESULT_SLOT {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                PLAYER_INV_START,
                HOTBAR_END,
                FillDirection::Backward,
            )
        } else if slot_index < RESULT_SLOT {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                PLAYER_INV_START,
                HOTBAR_END,
                FillDirection::Forward,
            )
        } else if can_move_into_inputs(guard, &self.input_container, &clicked) {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                TEMPLATE_SLOT,
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
        };

        if !moved {
            return Some(ItemStack::empty());
        }
        behavior.update_quick_move_source(guard, slot_index, &remaining, &clicked);
        if remaining.count == clicked.count {
            return Some(ItemStack::empty());
        }
        if let Some(remainder) = behavior.slots()[slot_index].on_take(guard, &mut remaining, player) {
            player.add_item_or_drop_with_guard(guard, remainder);
        }
        Some(clicked)
    }
}

fn can_move_into_inputs(
    guard: &ContainerLockGuard,
    input_container: &Shared<SimpleContainer>,
    stack: &ItemStack,
) -> bool {
    let input = guard
        .get_typed::<SimpleContainer>(ContainerId::from_arc(input_container))
        .expect("smithing input is registered");
    (REGISTRY.recipes.smithing_accepts_template(stack) && input.get_item(0).is_empty())
        || (REGISTRY.recipes.smithing_accepts_base(stack) && input.get_item(1).is_empty())
        || (REGISTRY.recipes.smithing_accepts_addition(stack) && input.get_item(2).is_empty())
}

struct SmithingHandler {
    input_container: Shared<SimpleContainer>,
    result_container: Shared<ResultContainer>,
    block_pos: BlockPos,
    world: Arc<World>,
}

impl ResultHandler for SmithingHandler {
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
            .expect("smithing input is registered");
        for slot in 0..3 {
            let stack = input.get_item_mut(slot);
            if !stack.is_empty() {
                stack.shrink(1);
            }
        }
        self.world.level_event(
            level_events::SOUND_SMITHING_TABLE_USED,
            self.block_pos,
            0,
            None,
        );
        None
    }

    fn is_result_valid(&self, guard: &ContainerLockGuard, _player: &Player) -> bool {
        !guard
            .get_typed::<ResultContainer>(ContainerId::from_arc(&self.result_container))
            .expect("smithing result is registered")
            .get_item(0)
            .is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{SmithingKind, smithing};
    use crate::{
        behavior::init_behaviors,
        entity::Entity as _,
        inventory::{
            click::{Click, MouseButton},
            container::Container as _,
        },
        test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk},
    };
    use glam::DVec3;
    use steel_registry::{
        init_vanilla_registry, item_stack::ItemStack, vanilla_blocks, vanilla_items,
    };
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos, Downcast as _};

    #[test]
    fn netherite_upgrade_converts_diamond_chestplate() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("smithing_netherite");
        let pos = BlockPos::new(0, 64, 0);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos,
            vanilla_blocks::SMITHING_TABLE.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        let player = TestPlayerBuilder::new(Arc::clone(&world), "Smith", 1).build();
        player.base().set_position_local(DVec3::new(0.5, 64.0, 0.5));
        let mut menu = smithing(Arc::clone(&player.inventory), 1, pos, &world);

        *menu.behavior_mut().carried_mut() =
            ItemStack::new(&vanilla_items::NETHERITE_UPGRADE_SMITHING_TEMPLATE);
        menu.clicked(
            Click::Pickup {
                slot: 0,
                button: MouseButton::Left,
            },
            &player,
        );
        *menu.behavior_mut().carried_mut() = ItemStack::new(&vanilla_items::DIAMOND_CHESTPLATE);
        menu.clicked(
            Click::Pickup {
                slot: 1,
                button: MouseButton::Left,
            },
            &player,
        );
        *menu.behavior_mut().carried_mut() = ItemStack::new(&vanilla_items::NETHERITE_INGOT);
        menu.clicked(
            Click::Pickup {
                slot: 2,
                button: MouseButton::Left,
            },
            &player,
        );

        let kind = menu.kind().downcast_ref::<SmithingKind>().unwrap();
        let result = kind.result_container.lock().get_item(0).clone();
        assert!(result.is(&vanilla_items::NETHERITE_CHESTPLATE));
    }
}
