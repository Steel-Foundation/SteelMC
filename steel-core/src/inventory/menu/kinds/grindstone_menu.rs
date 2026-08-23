//! Grindstone menu (two repair inputs, result, player inventory).

use std::sync::Arc;

use glam::DVec3;
use rand::RngExt;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::data_components::{
    components::ItemEnchantments,
    vanilla_components::{ENCHANTMENTS, MAX_DAMAGE, REPAIR_COST, STORED_ENCHANTMENTS},
};
use steel_registry::item_stack::ItemStack;
use steel_registry::level_events;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_enchantment_tags::EnchantmentTag;
use steel_registry::vanilla_items;
use steel_registry::vanilla_menu_types;
use steel_registry::{REGISTRY, RegistryExt, TaggedRegistryExt};
use steel_utils::locks::{IntoShared, Shared};
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey};

use crate::entity::entities::ExperienceOrbEntity;
use crate::inventory::container::{ResultContainer, SimpleContainer};
use crate::inventory::prelude::*;
use crate::inventory::slots::ResultHandler;
use crate::player::player_inventory::PlayerInventory;
use crate::world::World;

const INPUT_SLOT: usize = 0;
const ADDITIONAL_SLOT: usize = 1;
const RESULT_SLOT: usize = 2;
const PLAYER_INV_START: usize = 3;
const PLAYER_INV_END: usize = 30;
const HOTBAR_START: usize = 30;
const HOTBAR_END: usize = 39;

/// Builds the grindstone menu.
#[must_use]
pub fn grindstone(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    block_pos: BlockPos,
    world: &Arc<World>,
) -> Menu {
    let input_container = SimpleContainer::new(2).into_shared();
    let result_container = ResultContainer::new().into_shared();

    let handler = GrindstoneHandler {
        input_container: input_container.clone(),
        result_container: result_container.clone(),
        block_pos,
        world: Arc::clone(world),
    };

    let mut builder = MenuBuilder::new(&vanilla_menu_types::GRINDSTONE, container_id);
    let input = builder.section_all_with(
        &input_container,
        SectionKind::restricted(|_slot, stack| may_place_in_grindstone(stack)),
    );
    let result = builder.result_slot(handler);
    let player = builder.player_inventory(&inventory);

    builder.route_with_remainder_policy(
        result,
        player.all(),
        FillDirection::Backward,
        FakeResultRemainderPolicy::Discard,
    );
    builder.route(input, player.all(), FillDirection::Forward);
    builder.route(player.hotbar(), input, FillDirection::Forward);
    builder.route(player.main(), input, FillDirection::Forward);
    builder.drain(input);

    builder.build(GrindstoneKind {
        input_container,
        result_container,
        block_pos,
        world: Arc::clone(world),
        result,
    })
}

fn may_place_in_grindstone(stack: &ItemStack) -> bool {
    stack.is_damageable_item() || has_any_enchantments(stack)
}

fn has_any_enchantments(stack: &ItemStack) -> bool {
    stack
        .get_enchantments_for_crafting()
        .is_some_and(|enchantments| !enchantments.is_empty())
}

/// Per-menu grindstone state.
pub struct GrindstoneKind {
    pub(crate) input_container: Shared<SimpleContainer>,
    pub(crate) result_container: Shared<ResultContainer>,
    block_pos: BlockPos,
    world: Arc<World>,
    result: Section,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl DowncastType for GrindstoneKind {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:menu/grindstone");
}

impl GrindstoneKind {
    fn create_result(&self, guard: &mut ContainerLockGuard) {
        let input = guard
            .get_typed::<SimpleContainer>(ContainerId::from_arc(&self.input_container))
            .expect("grindstone input is registered");
        let first = input.get_item(0).clone();
        let second = input.get_item(1).clone();
        let result = compute_result(&first, &second);
        guard
            .get_typed_mut::<ResultContainer>(ContainerId::from_arc(&self.result_container))
            .expect("grindstone result is registered")
            .set_item(0, result);
    }
}

fn compute_result(input: &ItemStack, additional: &ItemStack) -> ItemStack {
    if input.is_empty() && additional.is_empty() {
        return ItemStack::empty();
    }
    if input.count() > 1 || additional.count() > 1 {
        return ItemStack::empty();
    }

    if input.is_empty() || additional.is_empty() {
        let item = if input.is_empty() { additional } else { input };
        if has_any_enchantments(item) {
            remove_non_curses_from(item.copy_with_count(item.count()))
        } else {
            ItemStack::empty()
        }
    } else {
        merge_items(input, additional)
    }
}

fn merge_items(input: &ItemStack, additional: &ItemStack) -> ItemStack {
    if !input.is(additional.item) {
        return ItemStack::empty();
    }

    let durability = input.get_max_damage().max(additional.get_max_damage());
    let remaining = (input.get_max_damage() - input.get_damage_value())
        + (additional.get_max_damage() - additional.get_damage_value())
        + durability * 5 / 100;

    let mut count = 1;
    if !input.is_damageable_item() {
        if input.max_stack_size() < 2 || !ItemStack::matches(input, additional) {
            return ItemStack::empty();
        }
        count = 2;
    }

    let mut new_item = input.copy_with_count(count);
    if new_item.is_damageable_item() {
        new_item.set(MAX_DAMAGE, durability);
        new_item.set_damage_value((durability - remaining).max(0));
    }

    merge_enchants_from(&mut new_item, additional);
    remove_non_curses_from(new_item)
}

fn merge_enchants_from(target: &mut ItemStack, source: &ItemStack) {
    let Some(source_enchantments) = source.get_enchantments_for_crafting().cloned() else {
        return;
    };
    let mut target_enchantments = target
        .get_enchantments_for_crafting()
        .cloned()
        .unwrap_or_default();

    for (ident, level) in source_enchantments {
        let Some(enchantment) = REGISTRY.enchantments.by_key(&ident) else {
            continue;
        };
        let is_curse = REGISTRY
            .enchantments
            .is_in_tag(enchantment, &EnchantmentTag::CURSE);
        if !is_curse || target_enchantments.get_level(&ident) == 0 {
            target_enchantments.upgrade(ident, level);
        }
    }

    set_crafting_enchantments(target, target_enchantments);
}

fn remove_non_curses_from(mut item: ItemStack) -> ItemStack {
    let mut kept = ItemEnchantments::empty();
    if let Some(enchantments) = item.get_enchantments_for_crafting() {
        for (ident, level) in enchantments.iter() {
            let Some(enchantment) = REGISTRY.enchantments.by_key(ident) else {
                continue;
            };
            if REGISTRY
                .enchantments
                .is_in_tag(enchantment, &EnchantmentTag::CURSE)
            {
                kept.set(ident.clone(), *level);
            }
        }
    }

    set_crafting_enchantments(&mut item, kept.clone());
    if item.is(&vanilla_items::ENCHANTED_BOOK) && kept.is_empty() {
        item.set_item(&vanilla_items::BOOK.key);
        item.remove(STORED_ENCHANTMENTS);
    }

    let mut repair_cost = 0;
    for _ in 0..kept.len() {
        repair_cost = calculate_increased_repair_cost(repair_cost);
    }
    item.set(REPAIR_COST, repair_cost);
    item
}

fn set_crafting_enchantments(item: &mut ItemStack, enchantments: ItemEnchantments) {
    if item.is(&vanilla_items::ENCHANTED_BOOK) {
        if enchantments.is_empty() {
            item.remove(STORED_ENCHANTMENTS);
        } else {
            item.set(STORED_ENCHANTMENTS, enchantments);
        }
    } else if enchantments.is_empty() {
        item.remove(ENCHANTMENTS);
    } else {
        item.set(ENCHANTMENTS, enchantments);
    }
}

const fn calculate_increased_repair_cost(old_repair_cost: i32) -> i32 {
    old_repair_cost.saturating_mul(2).saturating_add(1)
}

fn experience_from_item(item: &ItemStack) -> i32 {
    let Some(enchantments) = item.get_enchantments_for_crafting() else {
        return 0;
    };
    let mut amount = 0;
    for (ident, level) in enchantments.iter() {
        let Some(enchantment) = REGISTRY.enchantments.by_key(ident) else {
            continue;
        };
        if REGISTRY
            .enchantments
            .is_in_tag(enchantment, &EnchantmentTag::CURSE)
        {
            continue;
        }
        let level = i32::try_from(*level).unwrap_or(i32::MAX);
        amount +=
            enchantment.min_cost.base + enchantment.min_cost.per_level_above_first * (level - 1);
    }
    amount
}

impl MenuKind for GrindstoneKind {
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        self.world.get_block_state(self.block_pos).get_block() == &vanilla_blocks::GRINDSTONE
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
        self.create_result(guard);
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
        let has_both_inputs = {
            let input = guard
                .get_typed::<SimpleContainer>(ContainerId::from_arc(&self.input_container))
                .expect("grindstone input is registered");
            !input.get_item(0).is_empty() && !input.get_item(1).is_empty()
        };

        let moved = if slot_index == RESULT_SLOT {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                PLAYER_INV_START,
                HOTBAR_END,
                FillDirection::Backward,
            )
        } else if slot_index != INPUT_SLOT && slot_index != ADDITIONAL_SLOT {
            if has_both_inputs {
                if (PLAYER_INV_START..PLAYER_INV_END).contains(&slot_index) {
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
                    INPUT_SLOT,
                    RESULT_SLOT,
                    FillDirection::Forward,
                )
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
        if let Some(remainder) = behavior.slots()[slot_index].on_take(guard, &mut remaining, player) {
            player.add_item_or_drop_with_guard(guard, remainder);
        }
        Some(clicked)
    }
}

struct GrindstoneHandler {
    input_container: Shared<SimpleContainer>,
    result_container: Shared<ResultContainer>,
    block_pos: BlockPos,
    world: Arc<World>,
}

impl ResultHandler for GrindstoneHandler {
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
        let xp = {
            let input = guard
                .get_typed::<SimpleContainer>(ContainerId::from_arc(&self.input_container))
                .expect("grindstone input is registered");
            experience_from_item(input.get_item(0)) + experience_from_item(input.get_item(1))
        };
        {
            let input = guard
                .get_typed_mut::<SimpleContainer>(ContainerId::from_arc(&self.input_container))
                .expect("grindstone input is registered");
            input.set_item(0, ItemStack::empty());
            input.set_item(1, ItemStack::empty());
        }

        if xp > 0 {
            let half = (f64::from(xp) / 2.0).ceil() as i32;
            let amount = half + rand::rng().random_range(0..half.max(1));
            let pos = DVec3::new(
                f64::from(self.block_pos.x()) + 0.5,
                f64::from(self.block_pos.y()) + 0.5,
                f64::from(self.block_pos.z()) + 0.5,
            );
            let world = Arc::clone(&self.world);
            guard.run_unlocked(|| ExperienceOrbEntity::award(&world, pos, amount));
        }

        self.world
            .level_event(level_events::SOUND_GRINDSTONE_USED, self.block_pos, 0, None);
        None
    }

    fn is_result_valid(&self, guard: &ContainerLockGuard, _player: &Player) -> bool {
        !guard
            .get_typed::<ResultContainer>(ContainerId::from_arc(&self.result_container))
            .expect("grindstone result is registered")
            .get_item(0)
            .is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{GrindstoneKind, grindstone};
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
    use steel_utils::{BlockPos, ChunkPos, Downcast as _, Identifier};

    #[test]
    fn disenchanting_a_sword_clears_non_curse_enchantments() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("grindstone_disenchant");
        let pos = BlockPos::new(0, 64, 0);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos,
            vanilla_blocks::GRINDSTONE.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        let player = TestPlayerBuilder::new(Arc::clone(&world), "Grinder", 1).build();
        player.base().set_position_local(DVec3::new(0.5, 64.0, 0.5));
        let mut menu = grindstone(Arc::clone(&player.inventory), 1, pos, &world);

        let mut sword = ItemStack::new(&vanilla_items::DIAMOND_SWORD);
        sword.set_enchantments(&[(Identifier::vanilla_static("sharpness"), 5)], false);
        *menu.behavior_mut().carried_mut() = sword;
        menu.clicked(
            Click::Pickup {
                slot: 0,
                button: MouseButton::Left,
            },
            &player,
        );

        let kind = menu.kind().downcast_ref::<GrindstoneKind>().unwrap();
        let result = kind.result_container.lock().get_item(0).clone();
        assert!(result.is(&vanilla_items::DIAMOND_SWORD));
        assert!(
            result
                .get_enchantments_for_crafting()
                .is_none_or(|enchantments| enchantments.is_empty())
        );
    }
}
