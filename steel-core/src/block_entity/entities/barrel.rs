//! Barrel block entity implementation.
//!
//! Barrels are container block entities with 27 slots (3x9 grid),
//! functioning similarly to chests but without double-chest behavior.

use std::{
    mem,
    sync::{Arc, Weak},
};

use simdnbt::ToNbtTag;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{
    BlockStateProperties, BoolProperty, Direction, EnumProperty,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::{sound_events, vanilla_block_entity_types};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex};

use crate::block_entity::openers_counter::{ContainerOpenersCounter, ContainerOpenersHost};
use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::entity::Entity as _;
use crate::inventory::container::{Container, ContainerLoot, unpack_container_loot};
use crate::inventory::lock::{ContainerId, ContainerRef, SharedContainer};
use crate::player::Player;
use crate::world::World;

/// Number of slots in a barrel (3 rows of 9).
pub const BARREL_SLOTS: usize = 27;

/// Vanilla `BarrelBlockEntity.playSound` volume.
const LID_SOUND_VOLUME: f32 = 0.5;

const OPEN: &BoolProperty = &BlockStateProperties::OPEN;
const FACING: &EnumProperty<Direction> = &BlockStateProperties::FACING;

/// Barrel block entity.
///
/// A simple container with 27 slots, using the same menu as chests.
pub struct BarrelBlockEntity {
    base: Arc<BlockEntityBase>,
    container: Arc<SyncMutex<BarrelContainer>>,
    container_ref: ContainerRef,
    openers_counter: ContainerOpenersCounter,
}

struct BarrelContainer {
    items: Vec<ItemStack>,
    /// Vanilla `RandomizableContainerBlockEntity`'s pending loot table.
    loot: ContainerLoot,
}

// SAFETY: This key is owned by Steel and uniquely identifies `BarrelBlockEntity`.
unsafe impl DowncastType for BarrelBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/barrel");
}

// SAFETY: This key is owned by Steel and uniquely identifies the independently
// lockable inventory data used by a barrel block entity.
unsafe impl DowncastType for BarrelContainer {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/barrel");
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
        let container = Arc::new(SyncMutex::new(BarrelContainer {
            items: vec![ItemStack::empty(); BARREL_SLOTS],
            loot: ContainerLoot::default(),
        }));
        let shared_container: SharedContainer = container.clone();
        Self {
            container_ref: ContainerRef::owned_by_block_entity(shared_container, Arc::clone(&base)),
            base,
            container,
            openers_counter: ContainerOpenersCounter::new(),
        }
    }

    /// Vanilla `RandomizableContainer.unpackLootTable`.
    ///
    /// Rolls a worldgen-assigned loot table into the barrel the first time its
    /// contents become observable.
    pub fn unpack_loot_table(&self, luck: f32) {
        let pos = self.get_block_pos();
        let mut container = self.container.lock();
        let Some(pending) = container.loot.take() else {
            return;
        };
        unpack_container_loot(pending, &mut *container, pos, luck);
        drop(container);
        self.set_changed();
    }

    /// Vanilla `BarrelBlockEntity.recheckOpen`, driven by the block's scheduled tick.
    pub fn recheck_open(&self, world: &Arc<World>) {
        if self.base.is_removed() {
            return;
        }
        self.openers_counter.recheck_openers(
            self,
            world,
            self.get_block_pos(),
            self.get_block_state(),
        );
    }

    /// Vanilla `BarrelBlockEntity.updateBlockState`.
    fn update_open_state(&self, world: &Arc<World>, state: BlockStateId, is_open: bool) {
        world.set_block(
            self.get_block_pos(),
            state.set_value(OPEN, is_open),
            UpdateFlags::UPDATE_ALL,
        );
    }

    /// Vanilla `BarrelBlockEntity.playSound`, offset half a block toward the lid.
    fn play_lid_sound(
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        sound: SoundEventRef,
    ) {
        let (dx, dy, dz) = state.get_value(FACING).offset();
        let position = glam::DVec3::new(
            f64::from(pos.x()) + 0.5 + f64::from(dx) / 2.0,
            f64::from(pos.y()) + 0.5 + f64::from(dy) / 2.0,
            f64::from(pos.z()) + 0.5 + f64::from(dz) / 2.0,
        );
        let pitch = rand::random::<f32>().mul_add(0.1, 0.9);
        world.play_sound_at(
            sound,
            SoundSource::Blocks,
            position,
            LID_SOUND_VOLUME,
            pitch,
            None,
        );
    }
}

impl ContainerOpenersHost for BarrelBlockEntity {
    fn on_open(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        Self::play_lid_sound(world, pos, state, &sound_events::BLOCK_BARREL_OPEN);
        self.update_open_state(world, state, true);
    }

    fn on_close(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        Self::play_lid_sound(world, pos, state, &sound_events::BLOCK_BARREL_CLOSE);
        self.update_open_state(world, state, false);
    }

    fn opener_count_changed(
        &self,
        _world: &Arc<World>,
        _pos: BlockPos,
        _state: BlockStateId,
        _previous: i32,
        _current: i32,
    ) {
        // Vanilla's barrel has no per-change side effect.
    }

    fn opener_container_id(&self) -> ContainerId {
        self.container_ref.container_id()
    }
}

impl BlockEntity for BarrelBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
        let items = {
            let mut container = self.container.lock();
            mem::replace(&mut container.items, vec![ItemStack::empty(); BARREL_SLOTS])
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
        let mut container = self.container.lock();
        container.items.fill(ItemStack::empty());
        container.loot.load(&nbt_view);

        // Load items from NBT using borrowed NBT for proper ItemStack parsing
        if let Some(items_list) = nbt_view.list("Items")
            && let Some(compounds) = items_list.compounds()
        {
            for compound in compounds {
                // Each item has a "Slot" byte and item data
                if let Some(slot) = compound.byte("Slot") {
                    let slot = slot as usize;
                    if slot < BARREL_SLOTS {
                        // Parse item directly from the borrowed compound
                        if let Some(item) = ItemStack::from_borrowed_compound(&compound) {
                            container.items[slot] = item;
                        }
                    }
                }
            }
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        // Save items to NBT (only non-empty slots)
        let container = self.container.lock();
        if container.loot.save(nbt) {
            // Vanilla skips the item list while a loot table is still pending.
            return;
        }
        let mut items: Vec<NbtCompound> = Vec::new();
        for (slot, item) in container.items.iter().enumerate() {
            if !item.is_empty() {
                // Use ItemStack's ToNbtTag implementation for proper component serialization
                if let NbtTag::Compound(mut item_nbt) = item.clone().to_nbt_tag() {
                    item_nbt.insert("Slot", slot as i8);
                    items.push(item_nbt);
                }
            }
        }
        nbt.insert("Items", NbtList::Compound(items));
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        // Barrels don't need to send inventory to clients on chunk load
        // (unlike signs which display text)
        None
    }

    fn container_ref(&self) -> Option<ContainerRef> {
        Some(self.container_ref.clone())
    }

    fn start_open(&self, player: &Player) {
        if self.base.is_removed() || player.is_spectator() {
            return;
        }
        let Some(world) = self.get_level() else {
            return;
        };
        self.openers_counter.increment_openers(
            self,
            player,
            &world,
            self.get_block_pos(),
            self.get_block_state(),
        );
    }

    fn stop_open(&self, player: &Player) {
        if self.base.is_removed() || player.is_spectator() {
            return;
        }
        let Some(world) = self.get_level() else {
            return;
        };
        self.openers_counter.decrement_openers(
            self,
            player,
            &world,
            self.get_block_pos(),
            self.get_block_state(),
        );
    }
}

impl Container for BarrelContainer {
    fn items(&self) -> &[ItemStack] {
        &self.items
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    fn get_container_size(&self) -> usize {
        BARREL_SLOTS
    }

    fn set_item(&mut self, slot: usize, mut stack: ItemStack) {
        if slot < BARREL_SLOTS {
            let max_stack_size = self.get_max_stack_size_for_item(&stack);
            if !stack.is_empty() && stack.count() > max_stack_size {
                stack.set_count(max_stack_size);
            }
            self.items[slot] = stack;
        }
    }

    fn get_max_stack_size(&self) -> i32 {
        64
    }

    fn set_changed(&mut self) {}
}

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_items};

    use super::*;

    fn test_barrel() -> BarrelBlockEntity {
        init_vanilla_registry();
        BarrelBlockEntity::new(
            Weak::new(),
            BlockPos::new(1, 2, 3),
            vanilla_blocks::BARREL.default_state(),
        )
    }

    #[test]
    fn set_item_limits_stack_to_vanilla_container_maximum() {
        let barrel = test_barrel();
        barrel
            .container
            .lock()
            .set_item(0, ItemStack::with_count(&vanilla_items::STONE, 100));

        assert_eq!(barrel.container.lock().get_item(0).count(), 64);
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
        assert_eq!(container.items.len(), BARREL_SLOTS);
        assert!(container.items.iter().all(ItemStack::is_empty));
    }
}
