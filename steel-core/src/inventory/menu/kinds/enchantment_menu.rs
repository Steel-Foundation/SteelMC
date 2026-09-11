//! Enchanting table menu.

use std::sync::Arc;

use steel_protocol::packets::game::SoundSource;
use steel_registry::{
    REGISTRY, RegistryEntry, TaggedRegistryExt, blocks::block_state_ext::BlockStateExt,
    item_stack::ItemStack, sound_events, vanilla_blocks, vanilla_custom_stats,
    vanilla_enchantment_tags::EnchantmentTag, vanilla_items, vanilla_menu_types,
};
use steel_utils::random::Random as _;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey, locks::Shared};

use crate::behavior::blocks::EnchantingTableBlock;
use crate::enchantment_helper::{EnchantmentInstance, get_enchantment_cost, select_enchantment};
use crate::inventory::container::DEFAULT_DISTANCE_BUFFER;
use crate::inventory::prelude::*;
use crate::inventory::slots::EnchantItemSlot;
use crate::player::player_inventory::PlayerInventory;
use crate::world::World;

/// Number of selectable offers.
const OFFER_COUNT: usize = 3;
/// Index of the item to enchant within `enchant_slots`.
const ITEM_SLOT: usize = 0;
/// Index of the lapis lazuli within `enchant_slots`.
const LAPIS_SLOT: usize = 1;
/// Vanilla `new SimpleContainer(2)`: the item slot and the lapis slot.
const TABLE_SLOT_COUNT: usize = 2;

/// Builds the enchanting table menu for the table at `pos`.
///
/// `enchantment_seed` is the opening player's vanilla `enchantmentSeed`.
#[must_use]
pub fn enchantment(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    pos: BlockPos,
    world: &Arc<World>,
    enchantment_seed: i32,
) -> Menu {
    let enchant_slots = SimpleContainer::new(TABLE_SLOT_COUNT).into_shared();
    let enchant_slots_ref = ContainerRef::from(Arc::clone(&enchant_slots));

    let mut builder = MenuBuilder::new(&vanilla_menu_types::ENCHANTMENT, container_id);
    let item = builder.section_at(
        enchant_slots_ref.clone(),
        [ITEM_SLOT],
        SectionKind::custom(|container, index| {
            Box::new(EnchantItemSlot::new(container.clone(), index))
        }),
    );
    let lapis = builder.section_at(
        enchant_slots_ref,
        [LAPIS_SLOT],
        SectionKind::restricted(|_, stack| stack.is(&vanilla_items::LAPIS_LAZULI)),
    );
    let player_slots = builder.player_inventory(&inventory);

    // Vanilla data slot order: costs, seed, enchantment clues, level clues.
    // Array literals (rather than `from_fn`) guarantee left-to-right evaluation,
    // so the wire order below rests on a documented contract, not an incidental one.
    let cost_slots = [
        builder.data_slot(0),
        builder.data_slot(0),
        builder.data_slot(0),
    ];
    let seed_slot = builder.data_slot(EnchantmentKind::client_value(enchantment_seed));
    let enchant_clue_slots = [
        builder.data_slot(-1),
        builder.data_slot(-1),
        builder.data_slot(-1),
    ];
    let level_clue_slots = [
        builder.data_slot(-1),
        builder.data_slot(-1),
        builder.data_slot(-1),
    ];

    // Vanilla `removed` hands both table slots back to the player.
    builder.drain([item, lapis]);

    builder.build(EnchantmentKind {
        enchant_slots,
        block_pos: pos,
        world: Arc::clone(world),
        item,
        lapis,
        player_slots,
        enchantment_seed,
        table_changes_seen: 0,
        costs: [0; OFFER_COUNT],
        enchant_clue: [-1; OFFER_COUNT],
        level_clue: [-1; OFFER_COUNT],
        cost_slots,
        seed_slot,
        enchant_clue_slots,
        level_clue_slots,
    })
}

/// Per-menu enchanting table state: the two table slots, the seeded offers, and
/// their client mirrors.
pub struct EnchantmentKind {
    /// The table's own two slots, indexed by [`ITEM_SLOT`] and [`LAPIS_SLOT`].
    enchant_slots: Shared<SimpleContainer>,
    block_pos: BlockPos,
    world: Arc<World>,
    item: Section,
    lapis: Section,
    player_slots: PlayerInventorySections,
    /// Full-precision vanilla `enchantmentSeed`; `seed_slot` carries its low 16 bits.
    enchantment_seed: i32,
    /// `enchant_slots.times_changed()` when the offers were last computed.
    table_changes_seen: u32,
    costs: [i32; OFFER_COUNT],
    /// Registry id of the shown enchantment per offer, or -1.
    enchant_clue: [i32; OFFER_COUNT],
    /// Level of the shown enchantment per offer, or -1.
    level_clue: [i32; OFFER_COUNT],
    cost_slots: [DataSlot; OFFER_COUNT],
    seed_slot: DataSlot,
    enchant_clue_slots: [DataSlot; OFFER_COUNT],
    level_clue_slots: [DataSlot; OFFER_COUNT],
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl DowncastType for EnchantmentKind {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:menu/enchantment");
}

impl EnchantmentKind {
    /// Data slots travel as protocol shorts; vanilla sends the low 16 bits of each int.
    const fn client_value(value: i32) -> i16 {
        let [low, high, _, _] = value.to_le_bytes();
        i16::from_le_bytes([low, high])
    }

    /// Java `Random(seed)` over an int seed, matching vanilla's `setSeed(int)` widening.
    fn seeded_random(seed: i32) -> LegacyRandom {
        LegacyRandom::from_seed(i64::from(seed).cast_unsigned())
    }

    fn sync_data(&self, behavior: &mut MenuBehavior) {
        for offer in 0..OFFER_COUNT {
            self.cost_slots[offer].set(behavior, Self::client_value(self.costs[offer]));
            self.enchant_clue_slots[offer]
                .set(behavior, Self::client_value(self.enchant_clue[offer]));
            self.level_clue_slots[offer].set(behavior, Self::client_value(self.level_clue[offer]));
        }
        self.seed_slot
            .set(behavior, Self::client_value(self.enchantment_seed));
    }

    const fn clear_offers(&mut self) {
        self.costs = [0; OFFER_COUNT];
        self.enchant_clue = [-1; OFFER_COUNT];
        self.level_clue = [-1; OFFER_COUNT];
    }

    fn count_bookshelves(&self) -> i32 {
        EnchantingTableBlock::BOOKSHELF_OFFSETS
            .iter()
            .filter(|offset| {
                EnchantingTableBlock::is_valid_book_shelf(&self.world, self.block_pos, **offset)
            })
            .count() as i32
    }

    /// Vanilla `EnchantmentMenu.getEnchantmentList`: reseeds `random` with
    /// `seed + slot` and selects the offer; books drop one result when several were rolled.
    fn enchantment_list(
        &self,
        random: &mut LegacyRandom,
        stack: &ItemStack,
        slot: usize,
        enchantment_cost: i32,
    ) -> Vec<EnchantmentInstance> {
        random.set_seed(i64::from(self.enchantment_seed.wrapping_add(slot as i32)));
        let mut list = select_enchantment(
            random,
            stack,
            enchantment_cost,
            REGISTRY
                .enchantments
                .iter_tag(&EnchantmentTag::IN_ENCHANTING_TABLE),
        );
        if stack.is(&vanilla_items::BOOK) && list.len() > 1 {
            let removed = random.next_i32_bounded(list.len() as i32) as usize;
            list.remove(removed);
        }
        list
    }

    /// `enchant_slots.times_changed()` as seen through `guard`.
    fn table_changes(&self, guard: &ContainerLockGuard) -> Option<u32> {
        guard
            .get_typed::<SimpleContainer>(ContainerId::from_arc(&self.enchant_slots))
            .map(SimpleContainer::times_changed)
    }

    /// Vanilla `EnchantmentMenu.slotsChanged`: rerolls the three offers for the table item.
    fn update_offers(&mut self, behavior: &mut MenuBehavior, guard: &mut ContainerLockGuard) {
        if let Some(changes) = self.table_changes(guard) {
            self.table_changes_seen = changes;
        }
        let stack = behavior.slots()[self.item.start()].get_item(guard).clone();
        if stack.is_empty() || !stack.is_enchantable() {
            self.clear_offers();
            self.sync_data(behavior);
            return;
        }

        // Counting bookshelves reads world blocks, not containers, so it does not
        // need the menu containers locked; run it unlocked to avoid holding the
        // container guard across an unrelated world-lock acquisition.
        let bookcases = guard.run_unlocked(|| self.count_bookshelves());
        let mut random = Self::seeded_random(self.enchantment_seed);
        for offer in 0..OFFER_COUNT {
            self.costs[offer] = get_enchantment_cost(&mut random, offer, bookcases, &stack);
            self.enchant_clue[offer] = -1;
            self.level_clue[offer] = -1;
            if self.costs[offer] < offer as i32 + 1 {
                self.costs[offer] = 0;
            }
        }
        for offer in 0..OFFER_COUNT {
            if self.costs[offer] <= 0 {
                continue;
            }
            let list = self.enchantment_list(&mut random, &stack, offer, self.costs[offer]);
            if list.is_empty() {
                continue;
            }
            let clue = list[random.next_i32_bounded(list.len() as i32) as usize];
            self.enchant_clue[offer] = clue.enchantment.id() as i32;
            self.level_clue[offer] = clue.level as i32;
        }
        self.sync_data(behavior);
    }
}

impl MenuKind for EnchantmentKind {
    /// Vanilla `stillValid`: the table must still stand there and be in reach.
    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        self.world.get_block_state(self.block_pos).get_block() == &vanilla_blocks::ENCHANTING_TABLE
            && player.is_within_block_interaction_range_with_buffer(
                self.block_pos,
                f64::from(DEFAULT_DISTANCE_BUFFER),
            )
    }

    /// Vanilla `slotsChanged`: rerolls the offers only when `enchantSlots`
    /// changed. Vanilla reaches this through that container's `setChanged`
    /// override, so player-inventory clicks leave the offers alone.
    fn slots_changed(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
        if self.table_changes(guard) == Some(self.table_changes_seen) {
            return;
        }
        self.update_offers(behavior, guard);
    }

    /// Vanilla `clickMenuButton`: applies offer `button_id` when the player can pay.
    fn click_menu_button(
        &mut self,
        behavior: &mut MenuBehavior,
        button_id: i32,
        player: &Player,
    ) -> bool {
        let Some(offer) = usize::try_from(button_id)
            .ok()
            .filter(|offer| *offer < OFFER_COUNT)
        else {
            log::debug!(
                "{} pressed invalid button id: {button_id}",
                player.gameprofile.name
            );
            return false;
        };
        let enchantment_cost = offer as i32 + 1;
        let has_infinite_materials = player.has_infinite_materials();

        // Only the table container takes part; the player inventory stays unlocked.
        let mut guard = ContainerLockGuard::lock_all(&[ContainerRef::from(&self.enchant_slots)]);
        let container_id = ContainerId::from_arc(&self.enchant_slots);
        let Some(container) = guard.get(container_id) else {
            return false;
        };
        let (stack, mut currency) = (
            container.get_item(ITEM_SLOT).clone(),
            container.get_item(LAPIS_SLOT).clone(),
        );

        if (currency.is_empty() || currency.count() < enchantment_cost) && !has_infinite_materials {
            return false;
        }
        let level = player.experience.lock().level();
        if self.costs[offer] <= 0
            || stack.is_empty()
            || ((level < enchantment_cost || level < self.costs[offer]) && !has_infinite_materials)
        {
            return false;
        }

        // The seed here is discarded immediately: `enchantment_list` reseeds `random`
        // with `seed + offer` before using it, mirroring vanilla's reuse of one
        // long-lived `random` field rather than seeding fresh for the roll itself.
        let mut random = Self::seeded_random(self.enchantment_seed);
        let new_enchantments = self.enchantment_list(&mut random, &stack, offer, self.costs[offer]);
        if new_enchantments.is_empty() {
            return true;
        }

        {
            let mut experience = player.experience.lock();
            experience.on_enchantment_performed(enchantment_cost, rand::random());
            self.enchantment_seed = experience.enchantment_seed();
        }

        // Vanilla `transmuteCopy`: a book keeps its count and components as an enchanted book.
        // `with_count_and_patch` approximates it via `sanitize_against`, which drops patch
        // entries equal to the new prototype's default, where vanilla's `forget` drops any
        // entry the new item defines a default for regardless of value. The two differ only
        // for patches unreachable through gameplay; identical for anything reachable in play.
        let mut enchanted = if stack.is(&vanilla_items::BOOK) {
            ItemStack::with_count_and_patch(
                &vanilla_items::ENCHANTED_BOOK,
                stack.count(),
                stack.components_patch().clone(),
            )
        } else {
            stack
        };
        for instance in &new_enchantments {
            enchanted.upgrade_enchantment(instance.enchantment.key.clone(), instance.level);
        }
        currency.consume(enchantment_cost, has_infinite_materials);

        let Some(container) = guard.get_mut(container_id) else {
            return false;
        };
        container.set_item(ITEM_SLOT, enchanted);
        container.set_item(LAPIS_SLOT, currency);
        container.set_changed();

        player.award_custom_stat(&vanilla_custom_stats::ENCHANT_ITEM);
        // TODO: Trigger CriteriaTriggers.ENCHANTED_ITEM once Steel has shared advancement foundations.

        self.update_offers(behavior, &mut guard);
        drop(guard);

        self.world.play_sound(
            &sound_events::BLOCK_ENCHANTMENT_TABLE_USE,
            SoundSource::Blocks,
            self.block_pos,
            1.0,
            rand::random::<f32>() * 0.1 + 0.9,
            None,
        );
        true
    }

    /// Vanilla `quickMoveStack`: table slots empty into the inventory, lapis goes
    /// to its slot, and anything else moves a single item into the empty item slot.
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
        let clicked = behavior.slots()[slot_index].get_item(guard).clone();
        if clicked.is_empty() {
            return Some(ItemStack::empty());
        }
        let mut remaining = clicked.clone();

        let moved = if self.item.contains(slot_index) || self.lapis.contains(slot_index) {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                self.player_slots.all().start(),
                self.player_slots.all().end(),
                FillDirection::Backward,
            )
        } else if clicked.is(&vanilla_items::LAPIS_LAZULI) {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                self.lapis.start(),
                self.lapis.end(),
                FillDirection::Backward,
            )
        } else {
            let item_slot = &behavior.slots()[self.item.start()];
            if item_slot.has_item(guard) || !item_slot.may_place(&clicked) {
                return Some(ItemStack::empty());
            }
            let single = remaining.split(1);
            item_slot.set_by_player(guard, single, &ItemStack::empty());
            true
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
mod tests;
