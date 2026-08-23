//! Chest block entity implementation.
//!
//! Backs chests and trapped chests: 27 slots, viewer tracking through
//! [`ContainerOpenersCounter`], and the lid block event clients animate.

use std::{
    mem,
    sync::{Arc, Weak},
};

use simdnbt::ToNbtTag;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_protocol::packets::game::SoundSource;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, ChestType};
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, locks::SyncMutex};

use crate::behavior::BLOCK_BEHAVIORS;
use crate::behavior::blocks::connected_chest_direction;
use crate::block_entity::openers_counter::{ContainerOpenersCounter, ContainerOpenersHost};
use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::entity::Entity as _;
use crate::inventory::container::{Container, ContainerLoot, unpack_container_loot};
use crate::inventory::lock::{ContainerId, ContainerRef, SharedContainer};
use crate::player::Player;
use crate::world::World;

/// Number of slots in a chest (3 rows of 9).
pub const CHEST_SLOTS: usize = 27;

/// Vanilla `ChestBlockEntity.EVENT_SET_OPEN_COUNT`.
const EVENT_SET_OPEN_COUNT: i32 = 1;

/// Vanilla `ChestBlockEntity.playSound` volume.
const LID_SOUND_VOLUME: f32 = 0.5;

/// Which of Vanilla's two chest block entity classes this instance stands in for.
///
/// Vanilla splits `ChestBlockEntity` and `TrappedChestBlockEntity`, which differ
/// only in `signalOpenCount`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ChestVariant {
    Chest,
    Trapped,
}

/// Chest block entity, shared by chests and trapped chests.
pub struct ChestBlockEntity {
    base: Arc<BlockEntityBase>,
    container: Arc<SyncMutex<ChestContainer>>,
    container_ref: ContainerRef,
    openers_counter: ContainerOpenersCounter,
    variant: ChestVariant,
}

struct ChestContainer {
    items: Vec<ItemStack>,
    /// Vanilla `RandomizableContainerBlockEntity`'s pending loot table.
    loot: ContainerLoot,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ChestBlockEntity`.
unsafe impl DowncastType for ChestBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/chest");
}

// SAFETY: This key is owned by Steel and uniquely identifies the independently
// lockable inventory data used by a chest block entity.
unsafe impl DowncastType for ChestContainer {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:container/chest");
}

impl ChestBlockEntity {
    fn of_variant(
        variant: ChestVariant,
        block_entity_type: BlockEntityTypeRef,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Self {
        let base = Arc::new(BlockEntityBase::new(block_entity_type, level, pos, state));
        let container = Arc::new(SyncMutex::new(ChestContainer {
            items: vec![ItemStack::empty(); CHEST_SLOTS],
            loot: ContainerLoot::default(),
        }));
        let shared_container: SharedContainer = container.clone();
        Self {
            container_ref: ContainerRef::owned_by_block_entity(shared_container, Arc::clone(&base)),
            base,
            container,
            openers_counter: ContainerOpenersCounter::new(),
            variant,
        }
    }

    /// Creates a chest block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self::of_variant(
            ChestVariant::Chest,
            &vanilla_block_entity_types::CHEST,
            level,
            pos,
            state,
        )
    }

    /// Creates a trapped chest block entity.
    #[must_use]
    pub fn new_trapped(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self::of_variant(
            ChestVariant::Trapped,
            &vanilla_block_entity_types::TRAPPED_CHEST,
            level,
            pos,
            state,
        )
    }

    /// Vanilla `RandomizableContainer.unpackLootTable`.
    ///
    /// Rolls a worldgen-assigned loot table into the chest the first time its
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

    /// Vanilla `ChestBlockEntity.getOpenCount`, read by trapped chests.
    #[must_use]
    pub fn opener_count(&self) -> i32 {
        self.openers_counter.opener_count()
    }

    /// Vanilla `ChestBlockEntity.recheckOpen`, driven by the block's scheduled tick.
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

    /// Vanilla `ChestBlockEntity.playSound`.
    ///
    /// A double chest plays one sound between both halves: the left half stays
    /// silent and the right half offsets toward its partner.
    fn play_lid_sound(
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        sound: SoundEventRef,
    ) {
        let chest_type = state.get_value(&BlockStateProperties::CHEST_TYPE);
        if chest_type == ChestType::Left {
            return;
        }

        let mut x = f64::from(pos.x()) + 0.5;
        let y = f64::from(pos.y()) + 0.5;
        let mut z = f64::from(pos.z()) + 0.5;
        if chest_type == ChestType::Right {
            let (step_x, step_z) = connected_chest_direction(state).offset_xz();
            x += f64::from(step_x) * 0.5;
            z += f64::from(step_z) * 0.5;
        }

        let pitch = rand::random::<f32>().mul_add(0.1, 0.9);
        world.play_sound_at(
            sound,
            SoundSource::Blocks,
            glam::DVec3::new(x, y, z),
            LID_SOUND_VOLUME,
            pitch,
            None,
        );
    }

    fn lid_sounds(state: BlockStateId) -> Option<(SoundEventRef, SoundEventRef)> {
        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        let chest = behavior.as_chest()?;
        Some((chest.open_sound(), chest.close_sound()))
    }
}

impl ContainerOpenersHost for ChestBlockEntity {
    fn on_open(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        if let Some((open_sound, _)) = Self::lid_sounds(state) {
            Self::play_lid_sound(world, pos, state, open_sound);
        }
    }

    fn on_close(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        if let Some((_, close_sound)) = Self::lid_sounds(state) {
            Self::play_lid_sound(world, pos, state, close_sound);
        }
    }

    fn opener_count_changed(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        previous: i32,
        current: i32,
    ) {
        let block = state.get_block();
        world.block_event(pos, block, EVENT_SET_OPEN_COUNT, current);

        // Vanilla `TrappedChestBlockEntity.signalOpenCount` republishes the
        // redstone signal the chest emits into its own and the block below it.
        if self.variant == ChestVariant::Trapped && previous != current {
            world.update_neighbors_at(pos, block);
            world.update_neighbors_at(pos.below(), block);
        }
    }

    fn opener_container_id(&self) -> ContainerId {
        self.container_ref.container_id()
    }
}

impl BlockEntity for ChestBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
        let items = {
            let mut container = self.container.lock();
            mem::replace(&mut container.items, vec![ItemStack::empty(); CHEST_SLOTS])
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
        let mut container = self.container.lock();
        container.items.fill(ItemStack::empty());

        container.loot.load(&nbt_view);

        if let Some(items_list) = nbt_view.list("Items")
            && let Some(compounds) = items_list.compounds()
        {
            for compound in compounds {
                let Some(slot) = compound.byte("Slot") else {
                    continue;
                };
                let slot = slot as usize;
                if slot < CHEST_SLOTS
                    && let Some(item) = ItemStack::from_borrowed_compound(&compound)
                {
                    container.items[slot] = item;
                }
            }
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let container = self.container.lock();
        if container.loot.save(nbt) {
            // Vanilla skips the item list while a loot table is still pending.
            return;
        }

        let mut items: Vec<NbtCompound> = Vec::new();
        for (slot, item) in container.items.iter().enumerate() {
            if !item.is_empty()
                && let NbtTag::Compound(mut item_nbt) = item.clone().to_nbt_tag()
            {
                item_nbt.insert("Slot", slot as i8);
                items.push(item_nbt);
            }
        }
        nbt.insert("Items", NbtList::Compound(items));
    }

    fn trigger_event(&self, param_a: i32, _param_b: i32) -> bool {
        // The lid animation itself is client-side; the server only confirms the
        // event so the block-event packet reaches nearby clients.
        param_a == EVENT_SET_OPEN_COUNT
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

impl Container for ChestContainer {
    fn items(&self) -> &[ItemStack] {
        &self.items
    }

    fn items_mut(&mut self) -> &mut [ItemStack] {
        &mut self.items
    }

    fn get_container_size(&self) -> usize {
        CHEST_SLOTS
    }

    fn set_item(&mut self, slot: usize, mut stack: ItemStack) {
        if slot < CHEST_SLOTS {
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
