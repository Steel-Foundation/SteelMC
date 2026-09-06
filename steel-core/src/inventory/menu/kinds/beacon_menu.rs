//! Beacon menu.
//!
//! A single payment slot plus the player inventory. Three data slots mirror the
//! beacon's pyramid level and configured effects, matching vanilla's
//! `DATA_LEVELS`, `DATA_PRIMARY`, and `DATA_SECONDARY`.

use std::slice;
use std::sync::Arc;

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::{
    REGISTRY, RegistryEntry, TaggedRegistryExt, item_stack::ItemStack, mob_effect::MobEffectRef,
    sound_events, vanilla_blocks, vanilla_item_tags, vanilla_menu_types,
};
use steel_utils::{BlockPos, locks::SyncMutex};

use crate::block_entity::BlockEntityBase;
use crate::block_entity::entities::{BeaconBlockEntity, BeaconState};
use crate::inventory::container::Container;
use crate::inventory::prelude::*;
use crate::player::player_inventory::PlayerInventory;
use crate::world::World;

/// Payment slot container; capped at 1 item like vanilla's `PaymentSlot`.
struct PaymentContainer {
    item: ItemStack,
}

// SAFETY: This Steel-owned key uniquely identifies this concrete container type.
unsafe impl steel_utils::DowncastType for PaymentContainer {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:container/beacon_payment");
}

impl PaymentContainer {
    fn new() -> Self {
        Self {
            item: ItemStack::empty(),
        }
    }
}

impl Container for PaymentContainer {
    fn items(&self) -> &[ItemStack] {
        slice::from_ref(&self.item)
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        slice::from_mut(&mut self.item)
    }

    fn get_max_stack_size(&self) -> i32 {
        1
    }

    fn set_changed(&mut self) {}
}

/// Builds the beacon menu.
#[must_use]
pub fn beacon(
    inventory: Shared<PlayerInventory>,
    container_id: u8,
    pos: BlockPos,
    world: &Arc<World>,
    state: Arc<SyncMutex<BeaconState>>,
    block_entity: Arc<BlockEntityBase>,
) -> Menu {
    let payment_container = PaymentContainer::new().into_shared();
    let payment_ref = ContainerRef::from(payment_container.clone());

    let mut builder = MenuBuilder::new(&vanilla_menu_types::BEACON, container_id);
    // Vanilla gates the payment slot with `ItemTags.BEACON_PAYMENT_ITEMS`.
    let payment = builder.section_with(
        &payment_ref,
        1,
        SectionKind::restricted(|_, stack| {
            REGISTRY.items.is_in_tag(
                stack.item(),
                &vanilla_item_tags::ItemTag::BEACON_PAYMENT_ITEMS,
            )
        }),
    );
    let player = builder.player_inventory(&inventory);
    let levels = builder.data_slot(0);
    let primary = builder.data_slot(0);
    let secondary = builder.data_slot(0);

    builder.route(payment, player.all(), FillDirection::Backward);
    // Deliberately not drained; `removed` below drops the payment as vanilla does.

    builder.build(BeaconKind {
        payment_ref,
        payment_container,
        state,
        block_entity,
        levels,
        primary,
        secondary,
        payment,
        player_main: player.main(),
        player_hotbar: player.hotbar(),
        world: Arc::clone(world),
        pos,
    })
}

/// Encodes an effect as vanilla's `BeaconMenu.encodeEffect`: `0` for none, or
/// the effect's registry id plus one.
fn encode_effect(effect: Option<MobEffectRef>) -> i16 {
    effect.map_or(0, |effect| effect.id() as i16 + 1)
}

/// Per-menu beacon state.
pub struct BeaconKind {
    payment_ref: ContainerRef,
    payment_container: Shared<PaymentContainer>,
    state: Arc<SyncMutex<BeaconState>>,
    /// Steel's stand-in for the `ContainerLevelAccess` vanilla's `BeaconMenu` holds, used only
    /// to mark the beacon changed after a selection.
    block_entity: Arc<BlockEntityBase>,
    levels: DataSlot,
    primary: DataSlot,
    secondary: DataSlot,
    payment: Section,
    player_main: Section,
    player_hotbar: Section,
    world: Arc<World>,
    pos: BlockPos,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl steel_utils::DowncastType for BeaconKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/beacon");
}

impl BeaconKind {
    fn sync_data_slots(&self, behavior: &mut MenuBehavior) {
        let state = self.state.lock();
        self.levels.set(behavior, state.levels as i16);
        self.primary
            .set(behavior, encode_effect(state.primary_power));
        self.secondary
            .set(behavior, encode_effect(state.secondary_power));
    }
}

impl MenuKind for BeaconKind {
    /// Mirrors vanilla `BeaconMenu.removed`, which drops the unspent payment rather than
    /// returning it. This is why the payment section is not drained: `return_drained_items`
    /// would put it back whenever the player is still connected and alive.
    fn removed(&mut self, _behavior: &mut MenuBehavior, player: &Player) {
        let payment = self.payment_container.lock().remove_item_no_update(0);
        if payment.is_empty() {
            return;
        }
        let _ = player.drop_item(payment, false, false);
    }

    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        self.world.get_block_state(self.pos).get_block() == &vanilla_blocks::BEACON
            && player.is_within_block_interaction_range_with_buffer(self.pos, 4.0)
    }

    fn on_open(
        &mut self,
        behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
        self.sync_data_slots(behavior);
    }

    fn on_tick(
        &mut self,
        behavior: &mut MenuBehavior,
        _guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
        self.sync_data_slots(behavior);
    }

    fn on_update_effects(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        primary: Option<MobEffectRef>,
        secondary: Option<MobEffectRef>,
    ) -> bool {
        let Some(payment) = guard.get(self.payment_ref.container_id()) else {
            return false;
        };
        if payment.get_item(0).is_empty() {
            return false;
        }

        let has_beam = {
            let mut state = self.state.lock();
            let levels = state.levels;
            if !BeaconState::validate_effects(primary, secondary, levels) {
                return false;
            }
            state.primary_power = primary;
            state.secondary_power = secondary;
            !state.beam_sections.is_empty()
        };

        // Vanilla removes the payment through `Slot.remove(1)`.
        guard.set_item(self.payment_ref.container_id(), 0, ItemStack::empty());
        self.sync_data_slots(behavior);

        // Vanilla `this.access.execute(Level::blockEntityChanged)`: marks the chunk unsaved so
        // the selection survives a restart. Vanilla sends no block-entity packet here either.
        self.block_entity.set_changed();

        // Vanilla plays this from `dataAccess.set(DATA_PRIMARY, ..)`, guarded on the beam.
        if has_beam {
            BeaconBlockEntity::play_sound(
                &self.world,
                self.pos,
                &sound_events::BLOCK_BEACON_POWER_SELECT,
            );
        }
        true
    }

    fn quick_move(
        &mut self,
        behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        slot_index: usize,
        player: &Player,
    ) -> Option<ItemStack> {
        if self.payment.contains(slot_index) {
            // Falls back to the route table (payment → player.all()).
            return None;
        }

        let clicked = behavior.slots()[slot_index].get_item(guard).clone();
        if clicked.is_empty() {
            return Some(ItemStack::empty());
        }
        if !behavior.slots()[slot_index].may_pickup(guard, player) {
            return Some(ItemStack::empty());
        }

        let mut remaining = clicked.clone();

        // Vanilla: `!paymentSlot.hasItem() && paymentSlot.mayPlace(stack) && count == 1`. Without
        // `hasItem` a single payment item is un-shift-clickable while the slot is occupied.
        let pay_start = self.payment.start();
        let payment_slot = &behavior.slots()[pay_start];
        let is_single_payment = !payment_slot.has_item(guard)
            && payment_slot.may_place(&clicked)
            && clicked.count() == 1;

        let moved = if is_single_payment {
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                pay_start,
                pay_start + 1,
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
            // Vanilla's final fallback targets the whole player inventory.
            behavior.move_item_stack_to(
                guard,
                slot_index,
                &mut remaining,
                self.player_main.start(),
                self.player_hotbar.end(),
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
        let slot = &behavior.slots()[slot_index];
        if let Some(leftover) = slot.on_take(guard, &remaining, player) {
            player.add_item_or_drop_with_guard(guard, leftover);
        }
        Some(clicked)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use steel_registry::{
        init_vanilla_registry, item_stack::ItemStack, vanilla_blocks, vanilla_items,
        vanilla_mob_effects,
    };
    use steel_utils::locks::SyncMutex;
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos, Downcast as _};

    use crate::behavior::init_behaviors;
    use crate::block_entity::entities::{BeaconBlockEntity, BeaconState};
    use crate::block_entity::init_block_entities;
    use crate::inventory::click::{Click, MouseButton};
    use crate::inventory::container::Container as _;
    use crate::inventory::lock::ContainerId;
    use crate::inventory::prelude::Menu;
    use crate::player::Player;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};
    use crate::world::World;

    use super::{BeaconKind, beacon};

    /// Places a beacon and opens its menu, returning the menu and the entity's shared state.
    fn open_test_beacon(
        world: &Arc<World>,
        player: &Player,
        pos: BlockPos,
    ) -> (Menu, Arc<SyncMutex<BeaconState>>) {
        assert!(world.set_block(
            pos,
            vanilla_blocks::BEACON.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let block_entity = world.get_block_entity(pos).expect("beacon block entity");
        let beacon_entity = block_entity
            .downcast_ref::<BeaconBlockEntity>()
            .expect("beacon block entity type");

        let menu = beacon(
            player.inventory.clone(),
            1,
            pos,
            world,
            beacon_entity.state(),
            beacon_entity.base_handle(),
        );
        (menu, beacon_entity.state())
    }

    /// Returns the payment container id of a beacon menu.
    fn payment_id_of(menu: &Menu) -> ContainerId {
        menu.kind()
            .downcast_ref::<BeaconKind>()
            .expect("beacon kind")
            .payment_ref
            .container_id()
    }

    /// Puts `stack` into the payment slot through a normal click.
    fn click_into_payment(menu: &mut Menu, player: &Player, stack: ItemStack) {
        *menu.behavior_mut().carried_mut() = stack;
        menu.clicked(
            Click::Pickup {
                slot: 0,
                button: MouseButton::Left,
            },
            player,
        );
    }

    #[test]
    fn beacon_menu_rejects_other_items_and_consumes_iron_on_effect_selection() {
        init_vanilla_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("beacon_menu_iron_payment");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let player = TestPlayerBuilder::new(Arc::clone(&world), "BeaconTester", 1).build();

        let (mut menu, state) = open_test_beacon(&world, &player, BlockPos::new(8, 64, 8));
        let payment_id = payment_id_of(&menu);

        // Non-payment items stay on the cursor.
        click_into_payment(&mut menu, &player, ItemStack::new(&vanilla_items::DIRT));
        assert!(menu.behavior().carried().is(&vanilla_items::DIRT));
        assert!({
            let guard = menu.behavior().lock_all_containers();
            guard
                .get(payment_id)
                .expect("payment container locked")
                .get_item(0)
                .is_empty()
        });

        // Iron is accepted, capped at one item like vanilla's payment slot.
        click_into_payment(
            &mut menu,
            &player,
            ItemStack::with_count(&vanilla_items::IRON_INGOT, 2),
        );
        assert_eq!(menu.behavior().carried().count(), 1);
        assert!({
            let guard = menu.behavior().lock_all_containers();
            guard
                .get(payment_id)
                .expect("payment container locked")
                .get_item(0)
                .is(&vanilla_items::IRON_INGOT)
        });

        // Simulate a level-1 pyramid so Speed is a valid primary.
        state.lock().levels = 1;

        // Selecting an effect consumes the payment and stores the selection.
        menu.update_effects(Some(vanilla_mob_effects::SPEED), None, &player.connection);
        assert!({
            let guard = menu.behavior().lock_all_containers();
            guard
                .get(payment_id)
                .expect("payment container locked")
                .get_item(0)
                .is_empty()
        });
        let state = state.lock();
        assert_eq!(
            state.primary_power.map(|effect| effect.key.clone()),
            Some(vanilla_mob_effects::SPEED.key.clone())
        );
        assert!(state.secondary_power.is_none());
    }

    /// Vanilla's `quickMoveStack` only routes an item into the payment slot when that slot is
    /// empty. Without the `!hasItem()` guard the move into the full 1-capacity slot fails and the
    /// item is stranded instead of falling through to the inventory/hotbar branch.
    #[test]
    fn shift_click_moves_payment_item_when_payment_slot_is_occupied() {
        init_vanilla_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("beacon_menu_occupied_payment");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let player = TestPlayerBuilder::new(Arc::clone(&world), "BeaconShift", 1).build();

        let (mut menu, _) = open_test_beacon(&world, &player, BlockPos::new(8, 64, 8));
        let payment_id = payment_id_of(&menu);

        // Occupy the payment slot.
        click_into_payment(
            &mut menu,
            &player,
            ItemStack::new(&vanilla_items::IRON_INGOT),
        );

        // Put a single ingot in the first main-inventory slot and shift-click it.
        let main_slot = menu
            .kind()
            .downcast_ref::<BeaconKind>()
            .expect("beacon kind")
            .player_main
            .start();
        {
            let mut guard = menu.behavior().lock_all_containers();
            menu.behavior().slots()[main_slot]
                .set_item(&mut guard, ItemStack::new(&vanilla_items::IRON_INGOT));
        }

        menu.clicked(Click::QuickMove { slot: main_slot }, &player);

        // The ingot moved rather than being stranded, and the payment slot is untouched.
        assert!(
            {
                let guard = menu.behavior().lock_all_containers();
                menu.behavior().slots()[main_slot]
                    .get_item(&guard)
                    .is_empty()
            },
            "shift-clicked ingot should have left the main inventory slot"
        );
        assert!({
            let guard = menu.behavior().lock_all_containers();
            guard
                .get(payment_id)
                .expect("payment container locked")
                .get_item(0)
                .is(&vanilla_items::IRON_INGOT)
        });
    }

    /// Vanilla `BeaconMenu.removed` drops the unspent payment on the floor instead of returning
    /// it to the inventory, even for a connected, living player.
    #[test]
    fn closing_the_menu_drops_the_unspent_payment() {
        init_vanilla_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("beacon_menu_drops_payment");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let player = TestPlayerBuilder::new(Arc::clone(&world), "BeaconClose", 1).build();

        let (mut menu, _) = open_test_beacon(&world, &player, BlockPos::new(8, 64, 8));
        click_into_payment(
            &mut menu,
            &player,
            ItemStack::new(&vanilla_items::IRON_INGOT),
        );

        menu.removed(&player);

        assert!(
            !player
                .inventory
                .lock()
                .contains_stack(&ItemStack::new(&vanilla_items::IRON_INGOT)),
            "vanilla drops the payment rather than returning it to the inventory"
        );
    }
}
