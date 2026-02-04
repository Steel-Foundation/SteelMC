//! A menu can be considered everything that's shown on the screen.
//! It consists of slots, slots consist of a view into a single inventory and position.
//! When you have a chest open for example a chest menu is shown, consisting of the chests slots and the players inventory slots.
//!
//! A menu is always the middle man between the server and the client.
//! This means that when the player doesn't have any menus open it actually has, it always has it's own inventory menu open.
//!
//! A menu holds 3 important structures:
//! - All slots for that menu
//! - All slots as cloned itemstacks
//! - The clients perception of the itemstacks
//!
//! This makes it so every time we run a sync (once per tick) we update the cloned itemstacks.
//! This in turn makes it so we can compare it with the clients perception of the itemstacks.
//! And if there are mismatches we can send the correct itemstacks to the client.
//!
//! The client also sends the itemstacks it thinks it has on interaction, so this makes it so we only update the client if they mismatch.

use std::mem;

use steel_protocol::packets::game::{
    CContainerSetContent, CContainerSetData, CContainerSetSlot, CSetCursorItem, ClickType,
    HashedPatchMap, HashedStack,
};
use steel_registry::{
    REGISTRY, data_components::DataComponentPatch, item_stack::ItemStack, menu_type::MenuTypeRef,
};

use crate::{
    inventory::{
        lock::{ContainerId, ContainerLockGuard, ContainerRef},
        slot::{Slot, SlotType},
    },
    player::{Player, networking::JavaConnection},
};
use std::sync::Arc;

/// Represents the server's perception of what the client knows about a slot.
///
/// This can be either:
/// - A full `ItemStack` (when we've sent the item to the client)
/// - A `HashedStack` (when we've received a hash from the client)
/// - Unknown (initial state, always needs sync)
#[derive(Debug, Clone, Default)]
pub enum RemoteSlot {
    /// We don't know what the client has (initial state).
    #[default]
    Unknown,
    /// We know the exact `ItemStack` the client should have.
    Known(ItemStack),
    /// We received a hash from the client and verified it matches.
    Hashed(HashedStack),
}

impl RemoteSlot {
    /// Creates an unknown remote slot.
    #[must_use]
    pub const fn unknown() -> Self {
        Self::Unknown
    }

    /// Forces the remote slot to a known `ItemStack` state.
    /// Called when we send an item to the client.
    pub fn force(&mut self, item: &ItemStack) {
        *self = Self::Known(item.clone());
    }

    /// Receives a hashed stack from the client.
    /// Called when the client sends us their perception.
    pub fn receive(&mut self, hash: HashedStack) {
        *self = Self::Hashed(hash);
    }

    /// Checks if the remote slot matches the local `ItemStack`.
    #[must_use]
    pub fn matches(&self, local: &ItemStack) -> bool {
        match self {
            Self::Unknown => false,
            Self::Known(remote) => ItemStack::matches(remote, local),
            Self::Hashed(hash) => hashed_stack_matches(hash, local),
        }
    }
}

/// Checks if a hashed stack matches the given `ItemStack`.
fn hashed_stack_matches(hash: &HashedStack, item: &ItemStack) -> bool {
    match hash {
        HashedStack::Empty => {
            if !item.is_empty() {
                log::info!("HashedStack mismatch: client sent Empty, server has {item}");
                return false;
            }
            true
        }
        HashedStack::Item {
            item_id,
            count,
            components,
        } => {
            if item.is_empty() {
                log::info!(
                    "HashedStack mismatch: client sent item_id={item_id} count={count}, server has Empty"
                );
                return false;
            }

            // Check item type and count match
            let local_id = *REGISTRY.items.get_id(item.item) as i32;
            if local_id != *item_id {
                log::info!(
                    "HashedStack mismatch: item_id client={item_id} server={local_id} ({})",
                    item.item.key
                );
                return false;
            }
            if item.count != *count {
                log::info!(
                    "HashedStack mismatch: count client={count} server={} for {}",
                    item.count,
                    item.item.key
                );
                return false;
            }

            // Validate component hashes
            validate_component_hashes(components, item.patch())
        }
    }
}

/// Validates that the hashed component patch matches the local patch.
fn validate_component_hashes(hashed: &HashedPatchMap, patch: &DataComponentPatch) -> bool {
    use rustc_hash::FxHashSet;
    use steel_registry::data_components::ComponentPatchEntry;

    // Check removed components match
    let local_removed: FxHashSet<i32> = patch
        .iter_removed()
        .filter_map(|k| {
            REGISTRY
                .data_components
                .get_id_by_key(k)
                .map(|id| id as i32)
        })
        .collect();
    let hashed_removed: FxHashSet<i32> = hashed.removed_components.iter().copied().collect();

    if local_removed != hashed_removed {
        log::info!(
            "HashedStack mismatch: removed components differ - client={hashed_removed:?} server={local_removed:?}"
        );
        return false;
    }

    // Check added component hashes
    // For each component in our patch, verify the client sent the correct hash
    for (key, entry) in patch.iter() {
        if let ComponentPatchEntry::Set(value) = entry {
            let Some(id) = REGISTRY.data_components.get_id_by_key(key) else {
                continue; // Unknown component, skip
            };
            let id = id as i32;

            let Some(&expected_hash) = hashed.added_components.get(&id) else {
                // Client didn't send hash for this component
                log::info!(
                    "HashedStack mismatch: client missing hash for component {key} (id={id})"
                );
                return false;
            };

            // Compute the hash of the component value using proper HashOps format
            let actual_hash = value.compute_hash();

            if actual_hash != expected_hash {
                log::info!(
                    "HashedStack mismatch: component {key} hash differs - client={expected_hash} server={actual_hash}"
                );
                return false;
            }
        }
    }

    // Check that the client didn't send extra components we don't have
    for &id in hashed.added_components.keys() {
        let Some(key) = REGISTRY.data_components.get_key_by_id(id as usize) else {
            log::info!("HashedStack mismatch: client sent unknown component id={id}");
            return false; // Unknown component ID from client
        };
        if !matches!(patch.get_entry(key), Some(ComponentPatchEntry::Set(_))) {
            log::info!(
                "HashedStack mismatch: client claims component {key} exists but server doesn't have it"
            );
            return false; // Client claims component exists but we don't have it
        }
    }

    true
}

/// Slot index for clicking outside the inventory window (drop items).
pub const SLOT_CLICKED_OUTSIDE: i16 = -999;

/// `QuickCraft` (drag) type constants.
pub const QUICKCRAFT_TYPE_CHARITABLE: i32 = 0; // Left-click drag (distribute evenly)
/// Right-click drag mode (place one item in each slot).
pub const QUICKCRAFT_TYPE_GREEDY: i32 = 1; // Right-click drag (place one each)
/// Middle-click drag mode (creative only, place full stacks).
pub const QUICKCRAFT_TYPE_CLONE: i32 = 2; // Middle-click drag (creative only, full stacks)

/// `QuickCraft` header constants (packet phase).
pub const QUICKCRAFT_HEADER_START: i32 = 0;
/// Continue adding slots to the drag operation.
pub const QUICKCRAFT_HEADER_CONTINUE: i32 = 1;
/// Finish the drag operation and distribute items.
pub const QUICKCRAFT_HEADER_END: i32 = 2;

/// Number of slots per row in standard inventory grids.
pub const SLOTS_PER_ROW: usize = 9;

/// Standard slot size in pixels (for UI calculations).
pub const SLOT_SIZE: i32 = 18;

/// Extracts the quickcraft type from a button mask.
/// Type is stored in bits 2-3.
#[must_use]
pub const fn get_quickcraft_type(button: i32) -> i32 {
    (button >> 2) & 3
}

/// Extracts the quickcraft header (phase) from a button mask.
/// Header is stored in bits 0-1.
#[must_use]
pub const fn get_quickcraft_header(button: i32) -> i32 {
    button & 3
}

/// Creates a quickcraft button mask from header and type.
#[must_use]
pub const fn get_quickcraft_mask(header: i32, quickcraft_type: i32) -> i32 {
    (header & 3) | ((quickcraft_type & 3) << 2)
}

/// Checks if a quickcraft type is valid for the given player.
/// Type 2 (clone) requires creative mode (infinite materials).
#[must_use]
pub const fn is_valid_quickcraft_type(quickcraft_type: i32, has_infinite_materials: bool) -> bool {
    match quickcraft_type {
        0 | 1 => true,
        2 => has_infinite_materials,
        _ => false,
    }
}

/// Calculates how many items to place per slot during quickcraft.
#[must_use]
pub fn get_quickcraft_place_count(
    slot_count: usize,
    quickcraft_type: i32,
    item: &ItemStack,
) -> i32 {
    match quickcraft_type {
        0 => (item.count as f32 / slot_count as f32).floor() as i32, // Distribute evenly
        1 => 1,                                                      // One per slot
        2 => item.max_stack_size(),                                  // Full stack (creative)
        _ => item.count,
    }
}

/// Checks if an item can be quick-placed into a slot.
/// If `ignore_size` is true, doesn't check if the combined count would exceed max stack size.
#[must_use]
pub fn can_item_quick_replace(
    slot_item: &ItemStack,
    carried: &ItemStack,
    ignore_size: bool,
) -> bool {
    let slot_is_empty = slot_item.is_empty();
    if slot_is_empty {
        return true;
    }
    if !ItemStack::is_same_item_same_components(carried, slot_item) {
        return false;
    }
    let combined = slot_item.count + if ignore_size { 0 } else { carried.count };
    combined <= carried.max_stack_size()
}

/// Shared behavior and state for all menu types.
pub struct MenuBehavior {
    /// The slots in this menu.
    pub slots: Vec<SlotType>,
    /// Cloned itemstacks from the actual slots (updated each sync).
    pub last_slots: Vec<ItemStack>,
    /// The client's perception of the itemstacks.
    pub remote_slots: Vec<RemoteSlot>,
    /// The item being carried by the cursor.
    pub carried: ItemStack,
    /// The client's perception of the carried item.
    pub remote_carried: RemoteSlot,
    /// The container ID (0 for player inventory).
    pub container_id: u8,
    /// Incremented every time the server and client mismatch.
    pub state_id: u32,
    /// None for player inventory. Some for all other menus.
    pub menu_type: Option<MenuTypeRef>,
    /// When true, remote updates are suppressed (during click handling).
    suppress_remote_updates: bool,
    /// Current quickcraft drag type (-1 if not dragging).
    pub quickcraft_type: i32,
    /// Current quickcraft status/phase (0 = not started, 1 = adding slots, 2 = ending).
    pub quickcraft_status: i32,
    /// Slots involved in the current quickcraft operation.
    pub quickcraft_slots: Vec<usize>,
    /// Data slots (for furnace progress, enchanting levels, etc.).
    data_slots: Vec<i16>,
    /// The client's perception of the data slot values.
    remote_data_slots: Vec<i16>,
}

impl MenuBehavior {
    /// Creates a new menu behavior with the given slots.
    #[must_use]
    pub fn new(slots: Vec<SlotType>, container_id: u8, menu_type: Option<MenuTypeRef>) -> Self {
        let slot_count = slots.len();
        Self {
            slots,
            last_slots: vec![ItemStack::empty(); slot_count],
            remote_slots: vec![RemoteSlot::Unknown; slot_count],
            carried: ItemStack::empty(),
            remote_carried: RemoteSlot::Unknown,
            container_id,
            state_id: 0,
            menu_type,
            suppress_remote_updates: false,
            quickcraft_type: -1,
            quickcraft_status: 0,
            quickcraft_slots: Vec::new(),
            data_slots: Vec::new(),
            remote_data_slots: Vec::new(),
        }
    }

    /// Collects all unique container references from the slots.
    #[must_use]
    pub fn collect_container_refs(&self) -> Vec<ContainerRef> {
        let mut refs = Vec::new();
        for slot in &self.slots {
            for container_ref in slot.all_container_refs() {
                let id = container_ref.container_id();
                if !refs.iter().any(|r: &ContainerRef| r.container_id() == id) {
                    refs.push(container_ref);
                }
            }
        }
        refs
    }

    /// Locks all containers referenced by slots in this menu.
    #[must_use]
    pub fn lock_all_containers(&self) -> ContainerLockGuard {
        let refs = self.collect_container_refs();
        let ref_refs: Vec<&ContainerRef> = refs.iter().collect();
        ContainerLockGuard::lock_all(&ref_refs)
    }

    /// Adds a data slot to the menu with an initial value.
    /// Returns the index of the added data slot.
    pub fn add_data_slot(&mut self, initial_value: i16) -> usize {
        let index = self.data_slots.len();
        self.data_slots.push(initial_value);
        self.remote_data_slots.push(0);
        index
    }

    /// Adds multiple data slots to the menu.
    /// Returns the starting index of the added data slots.
    pub fn add_data_slots(&mut self, count: usize) -> usize {
        let start_index = self.data_slots.len();
        for _ in 0..count {
            self.data_slots.push(0);
            self.remote_data_slots.push(0);
        }
        start_index
    }

    /// Gets the value of a data slot.
    #[must_use]
    pub fn get_data(&self, index: usize) -> Option<i16> {
        self.data_slots.get(index).copied()
    }

    /// Sets the value of a data slot.
    pub fn set_data(&mut self, index: usize, value: i16) {
        if let Some(slot) = self.data_slots.get_mut(index) {
            *slot = value;
        }
    }

    /// Resets the quickcraft state.
    pub fn reset_quick_craft(&mut self) {
        self.quickcraft_status = 0;
        self.quickcraft_slots.clear();
    }

    /// Returns true if a slot can be dragged to during quickcraft.
    /// Menus can override this via the Menu trait.
    #[must_use]
    pub const fn can_drag_to(&self, _slot_index: usize) -> bool {
        true
    }

    /// Moves items from `item_stack` to slots in the range [`start_slot`, `end_slot`).
    /// If `backwards` is true, iterates from end_slot-1 down to `start_slot`.
    /// Returns true if any items were moved.
    ///
    /// This is used by `quick_move_stack` implementations to distribute items.
    /// Based on Java's `AbstractContainerMenu::moveItemStackTo`.
    pub fn move_item_stack_to(
        &self,
        guard: &mut ContainerLockGuard,
        item_stack: &mut ItemStack,
        start_slot: usize,
        end_slot: usize,
        backwards: bool,
    ) -> bool {
        let mut anything_changed = false;

        // First pass: try to stack with existing items
        if item_stack.is_stackable() {
            let mut dest_slot = if backwards { end_slot - 1 } else { start_slot };

            while !item_stack.is_empty() {
                if backwards {
                    if dest_slot < start_slot {
                        break;
                    }
                } else if dest_slot >= end_slot {
                    break;
                }

                let slot = &self.slots[dest_slot];
                let target = slot.get_item(guard).clone();

                if !target.is_empty()
                    && ItemStack::is_same_item_same_components(item_stack, &target)
                {
                    let total_stack = target.count + item_stack.count;
                    let max_stack_size = slot.get_max_stack_size_for_item(guard, &target);

                    if total_stack <= max_stack_size {
                        item_stack.set_count(0);
                        slot.get_item_mut(guard).set_count(total_stack);
                        slot.set_changed(guard);
                        anything_changed = true;
                    } else if target.count < max_stack_size {
                        item_stack.shrink(max_stack_size - target.count);
                        slot.get_item_mut(guard).set_count(max_stack_size);
                        slot.set_changed(guard);
                        anything_changed = true;
                    }
                }

                if backwards {
                    if dest_slot == 0 {
                        break;
                    }
                    dest_slot -= 1;
                } else {
                    dest_slot += 1;
                }
            }
        }

        // Second pass: place in empty slots
        if !item_stack.is_empty() {
            let mut dest_slot = if backwards { end_slot - 1 } else { start_slot };

            while if backwards {
                dest_slot >= start_slot
            } else {
                dest_slot < end_slot
            } {
                let slot = &self.slots[dest_slot];
                let target = slot.get_item(guard).clone();

                if target.is_empty() && slot.may_place(item_stack) {
                    let max_stack_size = slot.get_max_stack_size_for_item(guard, item_stack);
                    let to_place = item_stack.count.min(max_stack_size);
                    let mut placed = item_stack.clone();
                    placed.set_count(to_place);
                    item_stack.shrink(to_place);
                    slot.set_by_player(guard, placed, &ItemStack::empty());
                    slot.set_changed(guard);
                    anything_changed = true;
                    break;
                }

                if backwards {
                    if dest_slot == 0 {
                        break;
                    }
                    dest_slot -= 1;
                } else {
                    dest_slot += 1;
                }
            }
        }

        anything_changed
    }

    /// Returns the current state ID.
    #[must_use]
    pub const fn get_state_id(&self) -> u32 {
        self.state_id
    }

    /// Suppresses remote updates during click handling.
    /// Call this before processing a click.
    pub const fn suppress_remote_updates(&mut self) {
        self.suppress_remote_updates = true;
    }

    /// Resumes remote updates after click handling.
    /// Call this after processing a click.
    pub const fn resume_remote_updates(&mut self) {
        self.suppress_remote_updates = false;
    }

    /// Transfers remote slot state from another menu to this one.
    ///
    /// When a container menu is closed, the inventory menu needs to know what
    /// the client thinks it has in the shared slots (player inventory). Without
    /// this transfer, the inventory menu would think the client has stale data
    /// and would try to resync slots that are actually correct.
    ///
    /// This matches slots by their (`container_id`, `container_slot`) pair, so only
    /// slots that reference the same underlying container position will transfer.
    ///
    /// Based on Java's `AbstractContainerMenu::transferState`.
    pub fn transfer_state(&mut self, other: &MenuBehavior) {
        use rustc_hash::FxHashMap;

        // Build a map of (container_id, container_slot) -> slot_index for the other menu
        let mut other_slots: FxHashMap<(ContainerId, usize), usize> = FxHashMap::default();
        for (slot_index, slot) in other.slots.iter().enumerate() {
            if let Some(key) = slot.container_key() {
                other_slots.insert(key, slot_index);
            }
        }

        // Transfer state for matching slots
        for (slot_index, slot) in self.slots.iter().enumerate() {
            if let Some(key) = slot.container_key()
                && let Some(&other_slot_index) = other_slots.get(&key)
            {
                // Transfer last_slots (the cached item state)
                self.last_slots[slot_index] = other.last_slots[other_slot_index].clone();
                // Transfer remote_slots (client's perception)
                self.remote_slots[slot_index] = other.remote_slots[other_slot_index].clone();
            }
        }
    }

    /// Returns true if a slot index is valid for this menu.
    /// -999 is used for clicking outside the inventory.
    /// -1 is also accepted (matches Java behavior, though not used by vanilla clients).
    #[must_use]
    pub const fn is_valid_slot_index(&self, slot: i16) -> bool {
        slot == -1 || slot == -999 || (slot >= 0 && (slot as usize) < self.slots.len())
    }

    /// Returns the number of slots in this menu.
    #[must_use]
    pub const fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// Gets a reference to a slot by index.
    #[must_use]
    pub fn get_slot(&self, index: usize) -> Option<&SlotType> {
        self.slots.get(index)
    }

    /// Gets the carried item (cursor).
    #[must_use]
    pub const fn get_carried(&self) -> &ItemStack {
        &self.carried
    }

    /// Sets the carried item (cursor).
    pub fn set_carried(&mut self, item: ItemStack) {
        self.carried = item;
    }

    /// Increments and returns the new state ID.
    pub const fn increment_state_id(&mut self) -> u32 {
        self.state_id = self.state_id.wrapping_add(1) & 0x7FFF; // Keep it within 15 bits
        self.state_id
    }

    /// Updates `last_slots` from actual slot contents.
    /// Call this once per tick before comparing with `remote_slots`.
    pub fn update_last_slots(&mut self, guard: &ContainerLockGuard) {
        for (i, slot) in self.slots.iter().enumerate() {
            self.last_slots[i] = slot.get_item(guard).clone();
        }
    }

    /// Checks if a slot has changed compared to remote perception.
    /// Returns true if slot needs to be synced to client.
    #[must_use]
    pub fn slot_needs_sync(&self, index: usize) -> bool {
        if index >= self.last_slots.len() || index >= self.remote_slots.len() {
            return false;
        }
        !self.remote_slots[index].matches(&self.last_slots[index])
    }

    /// Marks a slot as synced (updates remote perception).
    pub fn mark_slot_synced(&mut self, index: usize) {
        if index < self.last_slots.len() && index < self.remote_slots.len() {
            self.remote_slots[index].force(&self.last_slots[index]);
        }
    }

    /// Checks if carried item needs sync.
    #[must_use]
    pub fn carried_needs_sync(&self) -> bool {
        !self.remote_carried.matches(&self.carried)
    }

    /// Marks carried as synced.
    pub fn mark_carried_synced(&mut self) {
        self.remote_carried.force(&self.carried);
    }

    /// Sends all slots and carried item to the client (full sync).
    /// This is called when:
    /// - A menu is first opened
    /// - The client requests a full refresh
    /// - After certain operations that may have desynced the client
    pub fn send_all_data_to_remote(&mut self, connection: &Arc<JavaConnection>) {
        let guard = self.lock_all_containers();

        // First, update last_slots from actual slot contents
        self.update_last_slots(&guard);

        // Send full container content
        let packet = CContainerSetContent {
            container_id: i32::from(self.container_id),
            state_id: self.state_id as i32,
            items: self.last_slots.clone(),
            carried_item: self.carried.clone(),
        };

        connection.send_packet(packet);

        // Mark all slots and carried as synced
        for i in 0..self.last_slots.len() {
            self.remote_slots[i].force(&self.last_slots[i]);
        }
        self.remote_carried.force(&self.carried);

        // Send all data slots
        for i in 0..self.data_slots.len() {
            self.remote_data_slots[i] = self.data_slots[i];
            let packet = CContainerSetData {
                container_id: i32::from(self.container_id),
                id: i as i16,
                value: self.data_slots[i],
            };
            connection.send_packet(packet);
        }
    }

    /// Broadcasts changes to the client (incremental sync).
    /// This is called every tick to sync only changed slots.
    ///
    /// Based on Java's `AbstractContainerMenu::broadcastChanges`.
    /// Note: This does NOT increment `state_id` - that only happens when
    /// processing client clicks (via `increment_state_id`).
    pub fn broadcast_changes(&mut self, connection: &Arc<JavaConnection>) {
        let guard = self.lock_all_containers();

        // Update last_slots from actual slot contents
        self.update_last_slots(&guard);

        // Check each slot for changes
        for i in 0..self.last_slots.len() {
            if self.slot_needs_sync(i) {
                self.synchronize_slot_to_remote(i, connection);
            }
        }

        // Check carried item
        if self.carried_needs_sync() {
            self.synchronize_carried_to_remote(connection);
        }

        // Check data slots for changes
        for i in 0..self.data_slots.len() {
            self.synchronize_data_slot_to_remote(i, connection);
        }
    }

    /// Sends a data slot update to the client if it has changed.
    /// Based on Java's `AbstractContainerMenu::synchronizeDataSlotToRemote`.
    fn synchronize_data_slot_to_remote(&mut self, index: usize, connection: &Arc<JavaConnection>) {
        if self.suppress_remote_updates || index >= self.data_slots.len() {
            return;
        }

        let current = self.data_slots[index];
        let remote = self.remote_data_slots[index];

        if current != remote {
            self.remote_data_slots[index] = current;
            let packet = CContainerSetData {
                container_id: i32::from(self.container_id),
                id: index as i16,
                value: current,
            };
            connection.send_packet(packet);
        }
    }

    /// Sends a single slot update to the client.
    /// Based on Java's `AbstractContainerMenu::synchronizeSlotToRemote`.
    fn synchronize_slot_to_remote(&mut self, slot: usize, connection: &Arc<JavaConnection>) {
        if self.suppress_remote_updates || slot >= self.last_slots.len() {
            return;
        }

        let item = &self.last_slots[slot];

        let packet = CContainerSetSlot {
            container_id: i32::from(self.container_id),
            state_id: self.state_id as i32,
            slot: slot as i16,
            item_stack: item.clone(),
        };

        connection.send_packet(packet);
        self.mark_slot_synced(slot);
    }

    /// Sends the carried item (cursor) to the client.
    /// Based on Java's `AbstractContainerMenu::synchronizeCarriedToRemote`.
    fn synchronize_carried_to_remote(&mut self, connection: &Arc<JavaConnection>) {
        if self.suppress_remote_updates {
            return;
        }

        let packet = CSetCursorItem {
            item_stack: self.carried.clone(),
        };

        connection.send_packet(packet);
        self.mark_carried_synced();
    }

    /// Sets a remote slot to a known `ItemStack`.
    /// Called when we know exactly what the client has (e.g., creative mode set).
    /// Based on Java's `AbstractContainerMenu::setRemoteSlot`.
    pub fn set_remote_slot_known(&mut self, slot: usize, item: &ItemStack) {
        if slot < self.remote_slots.len() {
            self.remote_slots[slot].force(item);
        }
    }

    /// Handles a remote slot update from the client.
    /// This is called when the client sends us their perception of a slot.
    /// Based on Java's `AbstractContainerMenu::setRemoteSlotUnsafe`.
    pub fn set_remote_slot(&mut self, slot: usize, hash: HashedStack) {
        if slot < self.remote_slots.len() {
            self.remote_slots[slot].receive(hash);
        } else {
            log::debug!(
                "Incorrect slot index: {} available slots: {}",
                slot,
                self.remote_slots.len()
            );
        }
    }

    /// Handles a remote carried update from the client.
    /// Based on Java's `AbstractContainerMenu::setRemoteCarried`.
    pub fn set_remote_carried(&mut self, hash: HashedStack) {
        self.remote_carried.receive(hash);
    }

    /// Broadcasts full state to client.
    /// This triggers slot listeners for all slots and then sends all data to remote.
    /// Based on Java's `AbstractContainerMenu::broadcastFullState`.
    ///
    /// Note: This does NOT increment `state_id` - it just forces a full resync.
    pub fn broadcast_full_state(&mut self, connection: &Arc<JavaConnection>) {
        self.send_all_data_to_remote(connection);
    }

    /// Handles quickcraft (drag) operations.
    /// Based on Java's `AbstractContainerMenu::doClick` for `ClickType.QUICK_CRAFT`.
    pub fn do_quick_craft(
        &mut self,
        slot_num: i16,
        button: i8,
        has_infinite_materials: bool,
        player: &Player,
    ) {
        let expected_status = self.quickcraft_status;
        let new_status = get_quickcraft_header(i32::from(button));

        // Validate state transitions: must go 0->1->2 or stay at 1
        if (expected_status != 1 || new_status != 2) && expected_status != new_status {
            self.reset_quick_craft();
            return;
        }

        // Must have items to drag
        if self.carried.is_empty() {
            self.reset_quick_craft();
            return;
        }

        if new_status == QUICKCRAFT_HEADER_START {
            // Starting a new drag
            self.quickcraft_type = get_quickcraft_type(i32::from(button));
            if is_valid_quickcraft_type(self.quickcraft_type, has_infinite_materials) {
                self.quickcraft_status = 1;
                self.quickcraft_slots.clear();
            } else {
                self.reset_quick_craft();
            }
        } else if new_status == QUICKCRAFT_HEADER_CONTINUE {
            // Adding a slot to the drag
            if slot_num < 0 || slot_num as usize >= self.slots.len() {
                return;
            }
            let slot_index = slot_num as usize;
            let slot = &self.slots[slot_index];

            let guard = self.lock_all_containers();
            let slot_item = slot.get_item(&guard).clone();

            if can_item_quick_replace(&slot_item, &self.carried, true)
                && slot.may_place(&self.carried)
                && (self.quickcraft_type == QUICKCRAFT_TYPE_CLONE
                    || self.carried.count > self.quickcraft_slots.len() as i32)
                && self.can_drag_to(slot_index)
                && !self.quickcraft_slots.contains(&slot_index)
            {
                self.quickcraft_slots.push(slot_index);
            }
        } else if new_status == QUICKCRAFT_HEADER_END {
            // Finishing the drag - distribute items
            if !self.quickcraft_slots.is_empty() {
                if self.quickcraft_slots.len() == 1 {
                    // Only one slot - treat as a regular pickup click
                    let slot = self.quickcraft_slots[0];
                    self.reset_quick_craft();
                    self.do_pickup(slot as i16, self.quickcraft_type as i8, player);
                    return;
                }

                let source = self.carried.clone();
                if source.is_empty() {
                    self.reset_quick_craft();
                    return;
                }

                let mut remaining = self.carried.count;
                let quickcraft_slots = self.quickcraft_slots.clone();

                let mut guard = self.lock_all_containers();

                for &slot_index in &quickcraft_slots {
                    let slot = &self.slots[slot_index];
                    let slot_item = slot.get_item(&guard).clone();

                    if can_item_quick_replace(&slot_item, &self.carried, true)
                        && slot.may_place(&self.carried)
                        && (self.quickcraft_type == QUICKCRAFT_TYPE_CLONE
                            || self.carried.count >= quickcraft_slots.len() as i32)
                        && self.can_drag_to(slot_index)
                    {
                        let current_count = if slot_item.is_empty() {
                            0
                        } else {
                            slot_item.count
                        };
                        let max_size = source
                            .max_stack_size()
                            .min(slot.get_max_stack_size_for_item(&guard, &source));
                        let place_count = get_quickcraft_place_count(
                            quickcraft_slots.len(),
                            self.quickcraft_type,
                            &source,
                        );
                        let new_count = (place_count + current_count).min(max_size);
                        remaining -= new_count - current_count;

                        let mut new_item = source.clone();
                        new_item.set_count(new_count);
                        slot.set_item(&mut guard, new_item);
                    }
                }

                let mut new_carried = source;
                new_carried.set_count(remaining);
                self.carried = new_carried;
            }

            self.reset_quick_craft();
        } else {
            self.reset_quick_craft();
        }
    }

    /// Handles pickup click (left/right click to pick up or place items).
    /// Based on Java's `AbstractContainerMenu::doClick` for ClickType.PICKUP.
    #[allow(clippy::too_many_lines)]
    pub fn do_pickup(&mut self, slot_num: i16, button: i8, player: &Player) {
        // Slot -999 means clicked outside the inventory (drop items)
        if slot_num == -999 {
            if !self.carried.is_empty() {
                if button == 0 {
                    // Left click outside - drop all carried items
                    let to_drop = mem::take(&mut self.carried);
                    player.drop_item(to_drop, false, true);
                } else {
                    // Right click outside - drop one carried item
                    let mut to_drop = self.carried.clone();
                    to_drop.set_count(1);
                    self.carried.shrink(1);
                    player.drop_item(to_drop, false, true);
                }
            }
            return;
        }

        let slot_index = slot_num as usize;
        if slot_index >= self.slots.len() {
            return;
        }

        let mut guard = self.lock_all_containers();

        let slot = &self.slots[slot_index];

        // Get the current item in the slot
        let slot_item = slot.get_item(&guard).clone();
        let carried = mem::take(&mut self.carried);

        if slot_item.is_empty() {
            // Slot is empty - place carried items (if allowed)
            if !carried.is_empty() && slot.may_place(&carried) {
                let max_for_slot = slot.get_max_stack_size_for_item(&guard, &carried);
                let requested = if button == 0 { carried.count } else { 1 };
                let amount = requested.min(max_for_slot);

                let mut to_place = carried.clone();
                to_place.set_count(amount);

                let remaining = carried.count - amount;
                if remaining > 0 {
                    let mut new_carried = carried;
                    new_carried.set_count(remaining);
                    self.carried = new_carried;
                }

                slot.set_by_player(&mut guard, to_place, &ItemStack::empty());
            } else {
                // Can't place - keep carrying
                self.carried = carried;
            }
        } else if carried.is_empty() {
            // Carried is empty - pick up from slot (if allowed)
            // Use try_remove which enforces allow_modification rules
            // (result slots must be picked up in full, not partially)
            let amount = if button == 0 {
                slot_item.count
            } else {
                (slot_item.count + 1) / 2
            };

            // max_amount is i32::MAX for primary action (take all requested)
            // For result slots, try_remove will reject partial takes
            if let Some(taken) = slot.try_remove(&mut guard, amount, i32::MAX) {
                if let Some(remainder) = slot.on_take(&mut guard, &taken, player) {
                    // There's a remainder from crafting - add to player inventory or drop
                    player.add_item_or_drop_with_guard(&mut guard, remainder);
                }
                self.carried = taken;
            }
        } else if ItemStack::is_same_item_same_components(&slot_item, &carried) {
            // Same item type - try to stack (if slot allows this item type)
            if slot.may_place(&carried) {
                if button == 0 {
                    // Left click - add as many as possible to slot
                    let max = slot.get_max_stack_size_for_item(&guard, &carried);
                    let space = max - slot_item.count;
                    let to_add = space.min(carried.count);

                    if to_add > 0 {
                        slot.get_item_mut(&mut guard)
                            .set_count(slot_item.count + to_add);
                        let remaining = carried.count - to_add;
                        if remaining > 0 {
                            let mut new_carried = carried;
                            new_carried.set_count(remaining);
                            self.carried = new_carried;
                        }
                    } else {
                        self.carried = carried;
                    }
                } else {
                    // Right click - add one to slot
                    let max = slot.get_max_stack_size_for_item(&guard, &carried);
                    if slot_item.count < max {
                        slot.get_item_mut(&mut guard).set_count(slot_item.count + 1);
                        let remaining = carried.count - 1;
                        if remaining > 0 {
                            let mut new_carried = carried;
                            new_carried.set_count(remaining);
                            self.carried = new_carried;
                        }
                    } else {
                        self.carried = carried;
                    }
                }
            } else {
                // Can't place this item type in this slot
                // In Java, if items are same type but may_place fails, try to take from slot
                if slot.may_pickup() {
                    // Try to add slot items to carried stack
                    let space = carried.max_stack_size() - carried.count;
                    if space > 0 {
                        let take_amount = slot_item.count.min(space);
                        let taken = slot.remove(&mut guard, take_amount);
                        // Handle any remainder (regular slots don't produce remainders, but be safe)
                        if let Some(remainder) = slot.on_take(&mut guard, &taken, player) {
                            player.add_item_or_drop_with_guard(&mut guard, remainder);
                        }
                        let mut new_carried = carried;
                        new_carried.grow(taken.count);
                        self.carried = new_carried;
                    } else {
                        self.carried = carried;
                    }
                } else {
                    self.carried = carried;
                }
            }
        } else {
            // Different items - swap (if both operations are allowed)
            if slot.may_pickup() && slot.may_place(&carried) {
                if carried.count <= slot.get_max_stack_size_for_item(&guard, &carried) {
                    slot.set_by_player(&mut guard, carried, &slot_item);
                    self.carried = slot_item;
                } else {
                    self.carried = carried;
                }
            } else {
                self.carried = carried;
            }
        }

        slot.set_changed(&mut guard);
    }

    /// Handles clone (middle-click in creative).
    pub fn do_clone(&mut self, slot_num: i16, has_infinite_materials: bool) {
        if !has_infinite_materials || !self.carried.is_empty() || slot_num < 0 {
            return;
        }

        let slot_index = slot_num as usize;
        if slot_index >= self.slots.len() {
            return;
        }

        let guard = self.lock_all_containers();
        let slot = &self.slots[slot_index];
        let slot_item = slot.get_item(&guard).clone();

        if !slot_item.is_empty() {
            let mut cloned = slot_item.clone();
            cloned.set_count(cloned.max_stack_size());
            self.carried = cloned;
        }
    }

    /// Handles throw (drop key Q or Ctrl+Q).
    /// button 0 = Q (drop 1), button 1 = Ctrl+Q (drop all, repeating while same item)
    ///
    /// Based on Java's `AbstractContainerMenu::doClick` for ClickType.THROW.
    pub fn do_throw(&mut self, slot_num: i16, button: i8, player: &Player) {
        if slot_num < 0 {
            return;
        }

        let slot_index = slot_num as usize;
        if slot_index >= self.slots.len() {
            return;
        }

        let mut guard = self.lock_all_containers();
        let slot = &self.slots[slot_index];

        // Check if pickup is allowed (Java's safeTake checks this internally)
        if !slot.may_pickup() {
            return;
        }

        // Java checks player.canDropItems() before each drop
        if !player.can_drop_items() {
            return;
        }

        let amount = if button == 0 {
            1
        } else {
            slot.get_item(&guard).count
        };

        let dropped = slot.remove(&mut guard, amount);
        if !dropped.is_empty() {
            player.drop_item(dropped.clone(), false, true);
        }
        slot.set_changed(&mut guard);

        // Ctrl+Q: Keep dropping while the slot has the same item type
        if button == 1 {
            loop {
                // Check may_pickup again for each iteration (Java does this via safeTake)
                if !slot.may_pickup() {
                    break;
                }
                // Java checks player.canDropItems() before each drop
                if !player.can_drop_items() {
                    break;
                }
                let current_item = slot.get_item(&guard).clone();
                if current_item.is_empty() || !ItemStack::is_same_item(&current_item, &dropped) {
                    break;
                }
                let more_dropped = slot.remove(&mut guard, current_item.count);
                if more_dropped.is_empty() {
                    break;
                }
                player.drop_item(more_dropped, false, true);
                slot.set_changed(&mut guard);
            }
        }
    }
}

/// Trait for menu implementations.
pub trait Menu {
    /// Returns a reference to the menu behavior.
    fn behavior(&self) -> &MenuBehavior;

    /// Returns a mutable reference to the menu behavior.
    fn behavior_mut(&mut self) -> &mut MenuBehavior;

    /// Handles shift-click (quick move) for a slot.
    ///
    /// Returns the item that was originally in the slot (before any move occurred),
    /// or empty if nothing was moved.
    ///
    /// Based on Java's `AbstractContainerMenu::quickMoveStack`.
    fn quick_move_stack(
        &mut self,
        guard: &mut ContainerLockGuard,
        slot_index: usize,
        player: &Player,
    ) -> ItemStack;

    /// Returns true if this menu is still valid for the player.
    fn still_valid(&self) -> bool {
        true
    }

    /// Returns true if the item can be taken from the slot during pickup all.
    /// Override to prevent pickup from certain slots (like crafting result).
    fn can_take_item_for_pick_all(&self, _carried: &ItemStack, _slot_index: usize) -> bool {
        true
    }

    /// Called when the menu is closed/removed.
    /// Override to handle cleanup like returning crafting grid items to the player.
    /// The default implementation clears the carried item by dropping it.
    fn removed(&mut self, player: &Player) {
        // Default: drop the carried item
        let carried = mem::take(&mut self.behavior_mut().carried);
        if !carried.is_empty() {
            player.drop_item(carried, false, true);
        }
    }

    /// Handles a click action in this menu.
    /// Based on Java's `AbstractContainerMenu::clicked` and doClick.
    ///
    /// `has_infinite_materials` should be true if the player is in creative mode.
    ///
    /// TODO: Add `tryItemClickBehaviourOverride` for bundle item support.
    fn clicked(
        &mut self,
        slot_num: i16,
        button: i8,
        click_type: ClickType,
        has_infinite_materials: bool,
        player: &Player,
    ) {
        if click_type == ClickType::QuickCraft {
            self.behavior_mut()
                .do_quick_craft(slot_num, button, has_infinite_materials, player);
        } else {
            // Any non-quickcraft click resets quickcraft state if in progress
            if self.behavior().quickcraft_status != 0 {
                self.behavior_mut().reset_quick_craft();
            }
            match click_type {
                ClickType::Pickup => {
                    self.behavior_mut().do_pickup(slot_num, button, player);
                }
                ClickType::QuickMove => {
                    self.do_quick_move(slot_num, player);
                }
                ClickType::Swap => {
                    self.do_swap(slot_num, button, player);
                }
                ClickType::Clone => {
                    self.behavior_mut()
                        .do_clone(slot_num, has_infinite_materials);
                }
                ClickType::Throw => {
                    self.behavior_mut().do_throw(slot_num, button, player);
                }
                ClickType::PickupAll => {
                    self.do_pickup_all(slot_num, button, player);
                }
                ClickType::QuickCraft => unreachable!(),
            }
        }
    }

    /// Handles quick move (shift-click).
    /// Based on Java's `AbstractContainerMenu::doClick` for `ClickType.QUICK_MOVE`.
    fn do_quick_move(&mut self, slot_num: i16, player: &Player) {
        if slot_num < 0 {
            return;
        }

        let slot_index = slot_num as usize;
        let slot_count = self.behavior().slots.len();
        if slot_index >= slot_count {
            return;
        }

        // Check if slot allows pickup
        let may_pickup = self.behavior().slots[slot_index].may_pickup();
        if !may_pickup {
            return;
        }

        let mut guard = self.behavior().lock_all_containers();

        // Get the initial item for comparison
        let initial_item = self.behavior().slots[slot_index].get_item(&guard).clone();
        if initial_item.is_empty() {
            return;
        }

        // Call quick_move_stack in a loop while the item type remains the same
        let mut result = self.quick_move_stack(&mut guard, slot_index, player);

        while !result.is_empty() {
            let current_item = self.behavior().slots[slot_index].get_item(&guard).clone();
            if !ItemStack::is_same_item(&current_item, &result) {
                break;
            }
            result = self.quick_move_stack(&mut guard, slot_index, player);
        }
    }

    /// Handles swap (number keys to swap with hotbar).
    /// button is the hotbar slot (0-8) or 40 for offhand.
    ///
    /// Based on Java's `AbstractContainerMenu::doClick` for `ClickType.SWAP`.
    fn do_swap(&mut self, slot_num: i16, button: i8, player: &Player) {
        // Validate button is a valid hotbar slot (0-8) or offhand (40)
        if !((0..9).contains(&button) || button == 40) {
            return;
        }

        if slot_num < 0 {
            return;
        }

        let slot_index = slot_num as usize;
        if slot_index >= self.behavior().slots.len() {
            return;
        }

        let mut guard = self.behavior().lock_all_containers();

        // Get the player inventory container ID from the player's inventory arc
        let player_inv_id = ContainerId::from_arc(&player.inventory);

        let behavior = self.behavior();
        let target_slot = &behavior.slots[slot_index];
        let inventory_slot = button as usize;

        // Get items from target slot (menu) and source (player inventory via guard)
        let target_item = target_slot.get_item(&guard).clone();
        let source_item = guard
            .get(player_inv_id)
            .map_or_else(ItemStack::empty, |inv| inv.get_item(inventory_slot).clone());

        if source_item.is_empty() && target_item.is_empty() {
            return;
        }

        if source_item.is_empty() {
            // Move from target to inventory
            if target_slot.may_pickup() {
                if let Some(inv) = guard.get_mut(player_inv_id) {
                    inv.set_item(inventory_slot, target_item.clone());
                }
                target_slot.set_by_player(&mut guard, ItemStack::empty(), &target_item);
                if let Some(remainder) = target_slot.on_take(&mut guard, &target_item, player) {
                    player.add_item_or_drop_with_guard(&mut guard, remainder);
                }
            }
        } else if target_item.is_empty() {
            // Move from inventory to target
            if target_slot.may_place(&source_item) {
                let max_size = target_slot.get_max_stack_size_for_item(&guard, &source_item);
                if source_item.count > max_size {
                    // Split the stack
                    let mut to_place = source_item.clone();
                    to_place.set_count(max_size);
                    target_slot.set_by_player(&mut guard, to_place, &ItemStack::empty());
                    if let Some(inv) = guard.get_mut(player_inv_id) {
                        inv.get_item_mut(inventory_slot).shrink(max_size);
                    }
                } else {
                    // Move entire stack
                    if let Some(inv) = guard.get_mut(player_inv_id) {
                        inv.set_item(inventory_slot, ItemStack::empty());
                    }
                    target_slot.set_by_player(&mut guard, source_item, &ItemStack::empty());
                }
            }
        } else {
            // Swap items between target and inventory
            if target_slot.may_pickup() && target_slot.may_place(&source_item) {
                let max_size = target_slot.get_max_stack_size_for_item(&guard, &source_item);
                if source_item.count > max_size {
                    // Source is too big - place partial and add target to inventory
                    let mut to_place = source_item.clone();
                    to_place.set_count(max_size);
                    target_slot.set_by_player(&mut guard, to_place, &target_item);
                    if let Some(remainder) = target_slot.on_take(&mut guard, &target_item, player) {
                        player.add_item_or_drop_with_guard(&mut guard, remainder);
                    }
                    // Try to add target item to inventory, drop if can't fit
                    if let Some(inv) = guard.get_mut(player_inv_id) {
                        inv.get_item_mut(inventory_slot).shrink(max_size);
                    }
                    player.add_item_or_drop_with_guard(&mut guard, target_item);
                } else {
                    // Simple swap
                    if let Some(inv) = guard.get_mut(player_inv_id) {
                        inv.set_item(inventory_slot, target_item.clone());
                    }
                    target_slot.set_by_player(&mut guard, source_item, &target_item);
                    if let Some(remainder) = target_slot.on_take(&mut guard, &target_item, player) {
                        player.add_item_or_drop_with_guard(&mut guard, remainder);
                    }
                }
            }
        }
    }

    /// Handles pickup all (double-click).
    /// Collects matching items from all slots into the carried stack.
    /// Based on Java's `AbstractContainerMenu::doClick` for `ClickType.PICKUP_ALL`.
    fn do_pickup_all(&mut self, slot_num: i16, button: i8, player: &Player) {
        if slot_num < 0 {
            return;
        }

        let slot_index = slot_num as usize;
        if slot_index >= self.behavior().slots.len() {
            return;
        }

        let mut guard = self.behavior().lock_all_containers();

        let behavior = self.behavior();
        let slot = &behavior.slots[slot_index];
        let slot_has_item = !slot.get_item(&guard).is_empty();
        let slot_may_pickup = slot.may_pickup();

        // Can only pickup all if carried is not empty and (slot is empty or can't be picked up)
        // Java: if (!carried.isEmpty() && (!slotxx.hasItem() || !slotxx.mayPickup(player)))
        if behavior.carried.is_empty() || (slot_has_item && slot_may_pickup) {
            return;
        }

        let max_stack = behavior.carried.max_stack_size();
        let carried_item = behavior.carried.clone();
        let slot_count = behavior.slots.len();

        // Determine iteration direction based on button
        // Java uses button == 0 for forward, button == 1 for reverse
        let (start, step): (i32, i32) = if button == 0 {
            (0, 1)
        } else {
            (slot_count as i32 - 1, -1)
        };

        // Two passes: first collect non-full stacks, then full stacks
        for pass in 0..2 {
            let mut i = start;
            while i >= 0 && i < slot_count as i32 && self.behavior().carried.count < max_stack {
                let target_slot = &self.behavior().slots[i as usize];
                let target_item = target_slot.get_item(&guard).clone();

                // Java checks: target.hasItem() && canItemQuickReplace(target, carried, true)
                //              && target.mayPickup(player) && this.canTakeItemForPickAll(carried, target)
                if !target_item.is_empty()
                    && can_item_quick_replace(&target_item, &carried_item, true)
                    && target_slot.may_pickup()
                    && self.can_take_item_for_pick_all(&carried_item, i as usize)
                {
                    // First pass: skip full stacks; Second pass: include full stacks
                    if pass != 0 || target_item.count != target_item.max_stack_size() {
                        let can_take = max_stack - self.behavior().carried.count;
                        let to_take = target_item.count.min(can_take);
                        let removed = target_slot.safe_take(&mut guard, to_take, i32::MAX, player);
                        self.behavior_mut().carried.grow(removed.count);
                    }
                }

                i += step;
            }
        }
    }
}
