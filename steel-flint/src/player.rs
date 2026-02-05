//! Test player implementation for `SteelMC`.
//!
//! This implementation wraps a real `steel_core::player::Player` with a
//! `FlintConnection` to enable testing of player interactions (like `use_item_on`)
//! without real network connections.

use std::sync;
use std::sync::Arc;

use flint_core::test_spec::{BlockFace, PlayerSlot};
use flint_core::{BlockPos, FlintPlayer, Item};
use steel_core::behavior::BlockHitResult;
use steel_core::inventory::container::Container;
use steel_core::player::game_mode;
use steel_core::player::player_inventory::PlayerInventory;
use steel_core::player::{ClientInformation, GameProfile, Player};
use steel_core::server::Server;
use steel_core::world::World;
use steel_registry::REGISTRY;
use steel_registry::item_stack::ItemStack;
use steel_utils::math::Vector3;
use steel_utils::types::InteractionHand;
use uuid::Uuid;

use crate::convert::{flint_face_to_direction, flint_pos_to_steel};
use crate::test_connection;
use crate::test_connection::FlintConnection;

/// Test player implementation that wraps a real `Player`.
///
/// This provides inventory management and enables calling real game logic
/// like `use_item_on` through the underlying player.
pub struct SteelTestPlayer {
    /// The real player instance.
    player: Arc<Player>,
    /// The test connection (kept for event inspection).
    #[allow(dead_code)]
    connection: Arc<FlintConnection>,
}

impl SteelTestPlayer {
    /// Creates a new test player in the given world.
    pub fn new(world: Arc<World>) -> Self {
        // Create a test connection
        let connection = Arc::new(FlintConnection::new());

        // Create a dummy game profile
        let gameprofile = GameProfile {
            id: Uuid::new_v4(),
            name: "TestPlayer".to_string(),
            properties: vec![],
            profile_actions: None,
        };

        // Create the player with our test connection
        let player = Arc::new_cyclic(|player_weak| {
            let p = Player::new(
                gameprofile,
                connection.clone(),
                world,
                sync::Weak::<Server>::new(),
                -1, // Negative entity ID for test players
                player_weak,
                ClientInformation::default(),
            );
            // Mark as loaded so interactions work
            p.client_loaded.store(true, sync::atomic::Ordering::Relaxed);
            p
        });

        Self { player, connection }
    }

    /// Gets the connection's recorded events (for test assertions).
    #[allow(dead_code)]
    #[must_use]
    pub fn get_events(&self) -> Vec<test_connection::PlayerEvent> {
        self.connection.get_events()
    }

    /// Clears the connection's recorded events.
    #[allow(dead_code)]
    pub fn clear_events(&self) {
        self.connection.clear_events();
    }

    /// Returns a reference to the underlying player.
    #[allow(dead_code)]
    #[must_use]
    pub const fn player(&self) -> &Arc<Player> {
        &self.player
    }
}

/// Converts a Flint [`PlayerSlot`] to a Steel inventory slot index.
///
/// Flint uses semantic slot names (e.g., `Hotbar1`, `OffHand`, `Helmet`),
/// while Steel uses numeric indices. This function maps between the two:
/// - Hotbar slots 1-9 → indices 0-8
/// - `OffHand` → `PlayerInventory::SLOT_OFFHAND`
/// - Armor slots → indices 36-39 (boots to helmet)
const fn player_slot_to_index(slot: PlayerSlot) -> usize {
    match slot {
        PlayerSlot::Hotbar1 => 0,
        PlayerSlot::Hotbar2 => 1,
        PlayerSlot::Hotbar3 => 2,
        PlayerSlot::Hotbar4 => 3,
        PlayerSlot::Hotbar5 => 4,
        PlayerSlot::Hotbar6 => 5,
        PlayerSlot::Hotbar7 => 6,
        PlayerSlot::Hotbar8 => 7,
        PlayerSlot::Hotbar9 => 8,
        PlayerSlot::OffHand => PlayerInventory::SLOT_OFFHAND,
        PlayerSlot::Boots => 36,
        PlayerSlot::Leggings => 37,
        PlayerSlot::Chestplate => 38,
        PlayerSlot::Helmet => 39,
    }
}

/// Converts a Flint [`Item`] to a Steel [`ItemStack`].
///
/// Handles the `minecraft:` namespace prefix (strips it if present) and
/// looks up the item in the registry. Returns an empty stack if the item
/// is not found.
fn flint_item_to_stack(item: &Item) -> ItemStack {
    // Parse the item ID - may have "minecraft:" prefix
    let item_id = if item.id.starts_with("minecraft:") {
        &item.id[10..]
    } else {
        &item.id
    };

    let identifier = steel_utils::Identifier::vanilla(item_id.to_string());

    // Look up the item in the registry
    if let Some(item_ref) = REGISTRY.items.by_key(&identifier) {
        ItemStack::with_count(item_ref, i32::from(item.count))
    } else {
        tracing::warn!("Unknown item: {} - returning empty stack", item.id);
        ItemStack::empty()
    }
}

/// Converts a Steel [`ItemStack`] to a Flint [`Item`].
///
/// Returns `None` for empty stacks. Adds the `minecraft:` namespace prefix
/// to the item ID for consistency with Flint's expected format.
fn stack_to_flint_item(stack: &ItemStack) -> Option<Item> {
    if stack.is_empty() {
        return None;
    }

    let id = format!("minecraft:{}", stack.item.key.path);
    Some(Item {
        id,
        count: stack.count.try_into().unwrap_or(1),
    })
}

impl FlintPlayer for SteelTestPlayer {
    fn set_slot(&mut self, slot: PlayerSlot, item: Option<&Item>) {
        let index = player_slot_to_index(slot);
        let stack = item.map_or_else(ItemStack::empty, flint_item_to_stack);

        let mut inv = self.player.inventory.lock();
        inv.set_item(index, stack);
    }

    fn get_slot(&self, slot: PlayerSlot) -> Option<Item> {
        let index = player_slot_to_index(slot);

        let inv = self.player.inventory.lock();
        let stack = inv.get_item(index);
        stack_to_flint_item(stack)
    }

    fn select_hotbar(&mut self, slot: u8) {
        if (1..=9).contains(&slot) {
            // Flint uses 1-9, Steel uses 0-8
            self.player.inventory.lock().set_selected_slot(slot - 1);
        }
    }

    fn selected_hotbar(&self) -> u8 {
        // Steel uses 0-8, Flint uses 1-9
        self.player.inventory.lock().get_selected_slot() + 1
    }

    fn use_item_on(&mut self, pos: BlockPos, face: &BlockFace) {
        let steel_pos = flint_pos_to_steel(pos);
        let direction = flint_face_to_direction(*face);

        // Create a block hit result
        let hit_result = BlockHitResult {
            location: Vector3::new(
                f64::from(steel_pos.x()) + 0.5,
                f64::from(steel_pos.y()) + 0.5,
                f64::from(steel_pos.z()) + 0.5,
            ),
            direction,
            block_pos: steel_pos,
            inside: false,
            world_border_hit: false,
            miss: false,
        };

        // Call the real game_mode::use_item_on
        let result = game_mode::use_item_on(
            &self.player,
            &self.player.world,
            InteractionHand::MainHand,
            &hit_result,
        );

        tracing::debug!("use_item_on({pos:?}, {face:?}) -> {result:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_test_registries;
    use crate::world::SteelTestWorld;
    use flint_core::FlintWorld;

    #[test]
    fn test_inventory() {
        init_test_registries();
        let mut world = SteelTestWorld::new();
        let mut player = world.create_player();

        let item = Item::new("minecraft:stone");
        player.set_slot(PlayerSlot::Hotbar1, Some(&item));

        let retrieved = player
            .get_slot(PlayerSlot::Hotbar1)
            .expect("Slot not found");
        assert_eq!(retrieved.id, "minecraft:stone");
    }

    #[test]
    fn test_hotbar_selection() {
        init_test_registries();
        let mut world = SteelTestWorld::new();
        let mut player = world.create_player();

        // Default is slot 1
        assert_eq!(player.selected_hotbar(), 1);

        player.select_hotbar(5);
        assert_eq!(player.selected_hotbar(), 5);

        // Out of range values should be ignored
        player.select_hotbar(0);
        assert_eq!(player.selected_hotbar(), 5);

        player.select_hotbar(10);
        assert_eq!(player.selected_hotbar(), 5);
    }
}
