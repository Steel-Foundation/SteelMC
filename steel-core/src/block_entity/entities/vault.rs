//! Vault block entity implementation.
//!
//! Port of vanilla `VaultBlockEntity` (server side) + `VaultState` state
//! machine. Vaults hold a display item, accept trial keys from players, and
//! eject their reward loot one stack at a time. Every player can unlock a
//! vault once per cooldown cycle (per-vault `rewarded_players` tracking).
//!
//! Steel differences: the ambient particle/sound presentation and the ominous
//! variant's separate display table are simplified; server state transitions,
//! key gating, and loot ejection follow vanilla.

use std::sync::{Arc, Weak};
use uuid::Uuid;

use glam::DVec3;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::ToNbtTag;
use steel_registry::RegistryExt;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, VaultState};
use steel_registry::item_stack::ItemStack;
use steel_registry::loot_table::LootContext;
use steel_registry::vanilla_items;
use steel_registry::sound_events;
use steel_registry::{REGISTRY, vanilla_block_entity_types};
use steel_utils::locks::SyncMutex;

use steel_protocol::packets::game::SoundSource;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, Identifier};

use crate::block_entity::{BlockEntity, BlockEntityBase};
use crate::entity::{Entity as _, LivingEntity as _};
use crate::player::Player;
use crate::world::{LevelAccessor as _, World};

const CONFIG_TAG: &str = "config";
const SERVER_DATA_TAG: &str = "server_data";
const SHARED_DATA_TAG: &str = "shared_data";
const KEY_ITEM_TAG: &str = "key_item";
const LOOT_TABLE_TAG: &str = "loot_table";
const ACTIVATION_RANGE_TAG: &str = "activation_range";
const DEACTIVATION_RANGE_TAG: &str = "deactivation_range";
const REWARDED_PLAYERS_TAG: &str = "rewarded_players";
const STATE_UPDATING_RESUMES_AT_TAG: &str = "state_updating_resumes_at";
const ITEMS_TO_EJECT_TAG: &str = "items_to_eject";
const TOTAL_EJECTIONS_NEEDED_TAG: &str = "total_ejections_needed";
const DISPLAY_ITEM_TAG: &str = "display_item";

/// Vanilla `VaultServerData.MAX_REWARD_PLAYERS`.
const MAX_REWARD_PLAYERS: usize = 128;
/// Vanilla `VaultServer.UNLOCKING_DELAY_TICKS`.
const UNLOCKING_DELAY_TICKS: i64 = 14;
/// Vanilla `VaultState.DELAY_BETWEEN_EJECTIONS_TICKS`.
const EJECTION_DELAY_TICKS: i64 = 20;
/// Vanilla `VaultConfig.DEFAULT` ranges.
const DEFAULT_ACTIVATION_RANGE: f64 = 4.0;
const DEFAULT_DEACTIVATION_RANGE: f64 = 4.5;

/// Parsed vanilla `VaultConfig`.
#[derive(Debug, Clone)]
struct VaultConfig {
    key_item: ItemStack,
    loot_table: Identifier,
    activation_range: f64,
    deactivation_range: f64,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            key_item: ItemStack::with_count(&vanilla_items::TRIAL_KEY, 1),
            loot_table: Identifier::vanilla_static("chests/trial_chambers/reward"),
            activation_range: DEFAULT_ACTIVATION_RANGE,
            deactivation_range: DEFAULT_DEACTIVATION_RANGE,
        }
    }
}

/// Mutable server-side vault state (vanilla `VaultServerData`).
struct VaultServerData {
    rewarded_players: Vec<Uuid>,
    state_updating_resumes_at: i64,
    items_to_eject: Vec<ItemStack>,
    total_ejections_needed: i32,
    last_insert_fail_timestamp: i64,
}

impl VaultServerData {
    fn new() -> Self {
        Self {
            rewarded_players: Vec::new(),
            state_updating_resumes_at: 0,
            items_to_eject: Vec::new(),
            total_ejections_needed: 0,
            last_insert_fail_timestamp: 0,
        }
    }

    fn has_rewarded_player(&self, player_uuid: &Uuid) -> bool {
        self.rewarded_players.contains(player_uuid)
    }

    /// Vanilla `addToRewardedPlayers` with oldest-first eviction.
    fn add_rewarded_player(&mut self, player_uuid: Uuid) {
        self.rewarded_players.push(player_uuid);
        if self.rewarded_players.len() > MAX_REWARD_PLAYERS {
            self.rewarded_players.remove(0);
        }
    }

    fn set_items_to_eject(&mut self, items: Vec<ItemStack>) {
        self.total_ejections_needed = items.len() as i32;
        self.items_to_eject = items;
    }

    fn pop_next_item_to_eject(&mut self) -> ItemStack {
        self.items_to_eject.pop().unwrap_or(ItemStack::empty())
    }

    fn ejection_progress(&self) -> f32 {
        if self.total_ejections_needed == 1 {
            1.0
        } else {
            1.0 - (self.items_to_eject.len() as f32 - 1.0)
                / (self.total_ejections_needed.max(1) as f32 - 1.0)
        }
    }
}

/// Vault block entity.
pub struct VaultBlockEntity {
    base: BlockEntityBase,
    config: SyncMutex<VaultConfig>,
    server_data: SyncMutex<VaultServerData>,
    shared_data: SyncMutex<VaultSharedData>,
}

/// Client-visible state (vanilla `VaultSharedData`).
struct VaultSharedData {
    display_item: ItemStack,
    connected_players: usize,
}

// SAFETY: Steel owns this concrete block entity key.
unsafe impl DowncastType for VaultBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/vault");
}

impl VaultBlockEntity {
    /// Creates a new vault block entity.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            base: BlockEntityBase::new(&vanilla_block_entity_types::VAULT, level, pos, state),
            config: SyncMutex::new(VaultConfig::default()),
            server_data: SyncMutex::new(VaultServerData::new()),
            shared_data: SyncMutex::new(VaultSharedData {
                display_item: ItemStack::empty(),
                connected_players: 0,
            }),
        }
    }

    /// Vanilla `VaultServer.tryInsertKey`.
    pub fn try_insert_key(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        held_stack: &mut ItemStack,
    ) -> bool {
        let config = self.config.lock().clone();
        let state: VaultState = self
            .get_block_state()
            .get_value(&BlockStateProperties::VAULT_STATE);
        let game_time = world.game_time();

        // Vanilla `canEjectReward`: a key item must be configured and the vault
        // must not be inactive.
        if config.key_item.is_empty() || state == VaultState::Inactive {
            return false;
        }

        let key_count = config.key_item.count();
        let is_valid_key = ItemStack::is_same_item_same_components(held_stack, &config.key_item)
            && held_stack.count() >= key_count;
        if !is_valid_key {
            self.play_insert_fail_sound(world, game_time, pos, &sound_events::BLOCK_VAULT_INSERT_ITEM_FAIL);
            return false;
        }

        {
            let server_data = self.server_data.lock();
            if server_data.has_rewarded_player(&player.uuid()) {
                drop(server_data);
                self.play_insert_fail_sound(
                    world,
                    game_time,
                    pos,
                    &sound_events::BLOCK_VAULT_REJECT_REWARDED_PLAYER,
                );
                return false;
            }
        }

        let items_to_eject = Self::resolve_items_to_eject(world, pos, player, held_stack, config.loot_table);
        if items_to_eject.is_empty() {
            return false;
        }

        held_stack.shrink(key_count);
        {
            let mut server_data = self.server_data.lock();
            server_data.set_items_to_eject(items_to_eject);
            server_data.state_updating_resumes_at = game_time + UNLOCKING_DELAY_TICKS;
            server_data.add_rewarded_player(player.uuid());
            let mut shared_data = self.shared_data.lock();
            shared_data.display_item = server_data.items_to_eject.last().cloned().unwrap_or(ItemStack::empty());
        }
        self.unlock(world, pos, state);
        true
    }

    /// Vanilla `VaultServer.unlock`: sets the vault to `UNLOCKING`.
    fn unlock(&self, world: &Arc<World>, pos: BlockPos, state: VaultState) {
        let state_id = self
            .get_block_state()
            .set_value(&BlockStateProperties::VAULT_STATE, VaultState::Unlocking);
        world.set_block_state(pos, state_id, UpdateFlags::UPDATE_ALL);
        world.play_sound(
            &sound_events::BLOCK_VAULT_INSERT_ITEM,
            SoundSource::Blocks,
            pos,
            1.0,
            1.0,
            None,
        );
        let _ = state;
    }

    /// Vanilla `VaultServer.resolveItemsToEject`.
    fn resolve_items_to_eject(
        _world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        inserted_stack: &ItemStack,
        loot_table: Identifier,
    ) -> Vec<ItemStack> {
        let Some(table) = REGISTRY.loot_tables.by_key(&loot_table) else {
            return Vec::new();
        };
        let mut rng = rand::rng();
        let mut context = LootContext::new(&mut rng)
            .with_origin(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()) + 0.5,
                f64::from(pos.z()) + 0.5,
            )
            .with_luck(player.get_luck())
            .with_this_entity(crate::entity::entity_loot_ref(player))
            .with_tool(inserted_stack);
        table.get_random_items(&mut context)
    }

    /// Vanilla `VaultServer.tick`.
    pub fn tick_vault(&self, world: &Arc<World>) {
        let pos = self.get_block_pos();
        let game_time = world.game_time();
        let state_id = self.get_block_state();
        let current_state: VaultState = state_id.get_value(&BlockStateProperties::VAULT_STATE);
        let config = self.config.lock().clone();

        // Vanilla cycles the display item every 20 ticks while active.
        if game_time % 20 == 0 && current_state == VaultState::Active {
            self.cycle_display_item(world, pos, &config);
        }

        if game_time < self.server_data.lock().state_updating_resumes_at {
            return;
        }

        let next_state = match current_state {
            VaultState::Inactive | VaultState::Active => {
                let range = if current_state == VaultState::Active {
                    config.deactivation_range
                } else {
                    config.activation_range
                };
                self.update_connected_players(world, pos, range);
                self.server_data.lock().state_updating_resumes_at = game_time + 20;
                if self.shared_data.lock().connected_players > 0 {
                    VaultState::Active
                } else {
                    VaultState::Inactive
                }
            }
            VaultState::Unlocking => {
                self.server_data.lock().state_updating_resumes_at = game_time + 20;
                VaultState::Ejecting
            }
            VaultState::Ejecting => {
                let (empty, item, progress) = {
                    let mut server_data = self.server_data.lock();
                    let empty = server_data.items_to_eject.is_empty();
                    let item = server_data.pop_next_item_to_eject();
                    let progress = server_data.ejection_progress();
                    (empty, item, progress)
                };
                if empty {
                    self.server_data.lock().total_ejections_needed = 0;
                    let range = config.deactivation_range;
                    self.update_connected_players(world, pos, range);
                    self.server_data.lock().state_updating_resumes_at = game_time + 20;
                    return;
                }
                // Vanilla `VaultState.ejectResultItem`.
                let eject_pos = DVec3::new(
                    f64::from(pos.x()) + 0.5,
                    f64::from(pos.y()) + 1.2,
                    f64::from(pos.z()) + 0.5,
                );
                let velocity = DVec3::new(
                    rand::random::<f64>() * 0.2 - 0.1,
                    2.0,
                    rand::random::<f64>() * 0.2 - 0.1,
                );
                world.spawn_item_with_velocity(eject_pos, item, velocity);
                world.level_event(3017, pos, 0, None);
                world.play_sound(
                    &sound_events::BLOCK_VAULT_EJECT_ITEM,
                    SoundSource::Blocks,
                    pos,
                    1.0,
                    0.8 + 0.4 * progress,
                    None,
                );
                // Lock `server_data` before `shared_data` (never nested): key
                // insertion locks in the same order, so a shared order avoids
                // deadlock between the tick thread and player interaction.
                let display = self
                    .server_data
                    .lock()
                    .items_to_eject
                    .last()
                    .cloned()
                    .unwrap_or(ItemStack::empty());
                self.shared_data.lock().display_item = display;
                self.server_data.lock().state_updating_resumes_at = game_time + EJECTION_DELAY_TICKS;
                VaultState::Ejecting
            }
        };

        if next_state != current_state {
            let next_state_id =
                state_id.set_value(&BlockStateProperties::VAULT_STATE, next_state);
            world.set_block_state(pos, next_state_id, UpdateFlags::UPDATE_ALL);
            self.on_state_transition(world, pos, current_state, next_state, &config);
            self.set_changed();
        }
    }

    /// Vanilla `VaultState.onTransition`.
    fn on_state_transition(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        from: VaultState,
        to: VaultState,
        config: &VaultConfig,
    ) {
        let _ = (from, config);
        match to {
            VaultState::Inactive => {
                self.shared_data.lock().display_item = ItemStack::empty();
                world.level_event(3016, pos, 0, None);
            }
            VaultState::Active => {
                world.level_event(3015, pos, 0, None);
            }
            VaultState::Unlocking | VaultState::Ejecting => {}
        }
    }

    /// Vanilla `VaultSharedData.updateConnectedPlayersWithinRange`.
    fn update_connected_players(&self, world: &Arc<World>, pos: BlockPos, range: f64) {
        let center = DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + 0.5,
            f64::from(pos.z()) + 0.5,
        );
        let mut count = 0usize;
        world.players.iter_players(|_, player| {
            if player.is_spectator() {
                return true;
            }
            if player.position().distance_squared(center) <= range * range {
                count += 1;
            }
            true
        });
        self.shared_data.lock().connected_players = count;
    }

    /// Vanilla `VaultServer.cycleDisplayItemFromLootTable`.
    fn cycle_display_item(&self, world: &Arc<World>, pos: BlockPos, config: &VaultConfig) {
        let display_item = Self::random_display_item(world, pos, &config.loot_table);
        self.shared_data.lock().display_item = display_item;
    }

    fn random_display_item(_world: &Arc<World>, pos: BlockPos, loot_table: &Identifier) -> ItemStack {
        let Some(table) = REGISTRY.loot_tables.by_key(loot_table) else {
            return ItemStack::empty();
        };
        let mut rng = rand::rng();
        let mut context = LootContext::new(&mut rng).with_origin(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + 0.5,
            f64::from(pos.z()) + 0.5,
        );
        let results = table.get_random_items(&mut context);
        results
            .into_iter()
            .filter(|stack| !stack.is_empty())
            .next()
            .unwrap_or(ItemStack::empty())
    }

    /// Vanilla `VaultServer.playInsertFailSound` with the 15-tick buffer.
    fn play_insert_fail_sound(
        &self,
        world: &Arc<World>,
        game_time: i64,
        pos: BlockPos,
        sound: steel_registry::sound_event::SoundEventRef,
    ) {
        let mut server_data = self.server_data.lock();
        if game_time >= server_data.last_insert_fail_timestamp + 15 {
            world.play_sound(sound, SoundSource::Blocks, pos, 1.0, 1.0, None);
            server_data.last_insert_fail_timestamp = game_time;
        }
    }
}

impl BlockEntity for VaultBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.base
    }

    fn tick(&self, world: &Arc<World>) {
        self.tick_vault(world);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        {
            let config = self.config.lock();
            let mut config_tag = NbtCompound::new();
            let key_item = config.key_item.clone().to_nbt_tag();
            if let NbtTag::Compound(key_item) = key_item {
                config_tag.insert(KEY_ITEM_TAG, key_item);
            }
            config_tag.insert(LOOT_TABLE_TAG, config.loot_table.to_string());
            config_tag.insert(ACTIVATION_RANGE_TAG, config.activation_range);
            config_tag.insert(DEACTIVATION_RANGE_TAG, config.deactivation_range);
            nbt.insert(CONFIG_TAG, config_tag);
        }

        let server_data = self.server_data.lock();
        let mut server_tag = NbtCompound::new();
        if !server_data.rewarded_players.is_empty() {
            let rewarded: Vec<Vec<i32>> = server_data
                .rewarded_players
                .iter()
                .map(uuid_to_ints)
                .collect();
            server_tag.insert(
                REWARDED_PLAYERS_TAG,
                NbtList::IntArray(rewarded),
            );
        }
        server_tag.insert(STATE_UPDATING_RESUMES_AT_TAG, server_data.state_updating_resumes_at);
        if !server_data.items_to_eject.is_empty() {
            let items: Vec<NbtCompound> = server_data
                .items_to_eject
                .iter()
                .filter_map(|stack| match stack.clone().to_nbt_tag() {
                    NbtTag::Compound(compound) => Some(compound),
                    _ => None,
                })
                .collect();
            server_tag.insert(ITEMS_TO_EJECT_TAG, NbtList::Compound(items));
        }
        server_tag.insert(TOTAL_EJECTIONS_NEEDED_TAG, server_data.total_ejections_needed);
        nbt.insert(SERVER_DATA_TAG, server_tag);
        drop(server_data);

        let shared_data = self.shared_data.lock();
        let mut shared_tag = NbtCompound::new();
        if !shared_data.display_item.is_empty() {
            let display = shared_data.display_item.clone().to_nbt_tag();
            if let NbtTag::Compound(display) = display {
                shared_tag.insert(DISPLAY_ITEM_TAG, display);
            }
        }
        shared_tag.insert("connected_players", shared_data.connected_players as i32);
        nbt.insert(SHARED_DATA_TAG, shared_tag);
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let view: NbtCompoundView<'_, '_> = nbt.into();

        if let Some(config_tag) = view.compound(CONFIG_TAG) {
            let mut config = self.config.lock();
            if let Some(key_item) = config_tag.compound(KEY_ITEM_TAG)
                && let Some(stack) = ItemStack::from_borrowed_compound(&key_item)
            {
                config.key_item = stack;
            }
            config.loot_table = config_tag
                .string(LOOT_TABLE_TAG)
                .and_then(|s| s.to_string().parse::<Identifier>().ok())
                .unwrap_or_else(|| Identifier::vanilla_static("chests/trial_chambers/reward"));
            config.activation_range = config_tag
                .double(ACTIVATION_RANGE_TAG)
                .unwrap_or(DEFAULT_ACTIVATION_RANGE);
            config.deactivation_range = config_tag
                .double(DEACTIVATION_RANGE_TAG)
                .unwrap_or(DEFAULT_DEACTIVATION_RANGE);
        }

        if let Some(server_tag) = view.compound(SERVER_DATA_TAG) {
            let mut server_data = self.server_data.lock();
            if let Some(list) = server_tag.list(REWARDED_PLAYERS_TAG) {
                server_data.rewarded_players = list
                    .int_arrays()
                    .map(|int_arrays| {
                        int_arrays
                            .iter()
                            .filter_map(|ints| uuid_from_ints(&ints.to_vec()))
                            .collect()
                    })
                    .unwrap_or_default();
            }
            server_data.state_updating_resumes_at = server_tag.long(STATE_UPDATING_RESUMES_AT_TAG).unwrap_or(0);
            if let Some(items) = server_tag.list(ITEMS_TO_EJECT_TAG) {
                if let Some(compounds) = items.compounds() {
                    server_data.items_to_eject = compounds
                        .into_iter()
                        .filter_map(|compound| ItemStack::from_borrowed_compound(&compound))
                        .collect();
                }
            }
            server_data.total_ejections_needed = server_tag.int(TOTAL_EJECTIONS_NEEDED_TAG).unwrap_or(0);
        }

        if let Some(shared_tag) = view.compound(SHARED_DATA_TAG) {
            let mut shared_data = self.shared_data.lock();
            if let Some(display) = shared_tag.compound(DISPLAY_ITEM_TAG)
                && let Some(stack) = ItemStack::from_borrowed_compound(&display)
            {
                shared_data.display_item = stack;
            }
            shared_data.connected_players = shared_tag.int("connected_players").unwrap_or(0) as usize;
        }
    }

    fn get_update_tag(&self) -> Option<NbtCompound> {
        // Vanilla syncs `server_data` and `shared_data`; clients render the
        // display item and ejection progress from these.
        let mut tag = self.save_custom_only();
        tag.remove(CONFIG_TAG);
        Some(tag)
    }
}

fn uuid_to_ints(uuid: &Uuid) -> Vec<i32> {
    let bytes = uuid.as_u128().to_be_bytes();
    bytes
        .chunks_exact(4)
        .map(|chunk| i32::from_be_bytes(chunk.try_into().expect("4-byte chunk")))
        .collect()
}

fn uuid_from_ints(ints: &[i32]) -> Option<Uuid> {
    if ints.len() != 4 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (index, value) in ints.iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_be_bytes());
    }
    Some(Uuid::from_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_compound;
    use simdnbt::owned::NbtCompound;
    use steel_registry::init_vanilla_registry;
    use steel_registry::vanilla_blocks;

    use super::*;

    fn template_nbt(loot_table: &str) -> NbtCompound {
        let mut config = NbtCompound::new();
        let mut key_item = NbtCompound::new();
        key_item.insert("id", "minecraft:trial_key");
        key_item.insert("count", 1_i32);
        config.insert(KEY_ITEM_TAG, key_item);
        config.insert(LOOT_TABLE_TAG, loot_table);
        let mut nbt = NbtCompound::new();
        nbt.insert(CONFIG_TAG, config);
        nbt
    }

    macro_rules! borrowed {
        ($nbt:ident, $out:ident) => {
            let mut bytes: Vec<u8> = Vec::new();
            $nbt.write(&mut bytes);
            let $out =
                read_compound(&mut Cursor::new(bytes.as_slice())).expect("test nbt should reborrow");
        };
    }

    fn vault() -> VaultBlockEntity {
        init_vanilla_registry();
        VaultBlockEntity::new(
            Weak::new(),
            BlockPos::new(1, 2, 3),
            vanilla_blocks::VAULT.default_state(),
        )
    }

    #[test]
    fn template_config_sets_key_item_and_loot_table() {
        let vault = vault();
        let nbt = template_nbt("minecraft:chests/trial_chambers/reward_ominous");
        borrowed!(nbt, compound);
        vault.load_additional(&compound);

        let config = vault.config.lock();
        assert!(!config.key_item.is_empty());
        assert_eq!(
            config.loot_table,
            Identifier::vanilla_static("chests/trial_chambers/reward_ominous")
        );
    }

    #[test]
    fn default_config_matches_vanilla() {
        let vault = vault();
        let config = vault.config.lock();
        assert!(ItemStack::is_same_item_same_components(
            &config.key_item,
            &ItemStack::with_count(&vanilla_items::TRIAL_KEY, 1)
        ));
        assert_eq!(
            config.loot_table,
            Identifier::vanilla_static("chests/trial_chambers/reward")
        );
        assert_eq!(config.activation_range, 4.0);
        assert_eq!(config.deactivation_range, 4.5);
    }

    #[test]
    fn rewarded_players_round_trip_and_evict_oldest() {
        let vault = vault();

        // Insertion-order eviction past vanilla's 128-player cap.
        let mut config = vault.config.lock();
        config.key_item = ItemStack::with_count(&vanilla_items::TRIAL_KEY, 1);
        drop(config);
        let mut server_data = vault.server_data.lock();
        let players: Vec<Uuid> = (0..=MAX_REWARD_PLAYERS)
            .map(|index| Uuid::from_u128(u128::from(index as u64)))
            .collect();
        for player_uuid in &players[..MAX_REWARD_PLAYERS] {
            server_data.rewarded_players.push(*player_uuid);
        }
        drop(server_data);
        let mut server_data = vault.server_data.lock();
        let oldest = players[0];
        let newest = players[MAX_REWARD_PLAYERS];
        server_data.add_rewarded_player(newest);
        assert_eq!(server_data.rewarded_players.len(), MAX_REWARD_PLAYERS);
        assert!(!server_data.rewarded_players.contains(&oldest));
        assert!(server_data.rewarded_players.contains(&newest));
    }

    #[test]
    fn server_state_round_trips_through_save_and_load() {
        let vault = vault();
        let nbt = template_nbt("minecraft:chests/trial_chambers/reward");
        borrowed!(nbt, compound);
        vault.load_additional(&compound);
        {
            let mut server_data = vault.server_data.lock();
            server_data.state_updating_resumes_at = 555;
            server_data.total_ejections_needed = 2;
        }

        let mut saved = NbtCompound::new();
        vault.save_additional(&mut saved);
        borrowed!(saved, saved_borrowed);

        let restored_vault = VaultBlockEntity::new(
            Weak::new(),
            BlockPos::new(1, 2, 3),
            vanilla_blocks::VAULT.default_state(),
        );
        restored_vault.load_additional(&saved_borrowed);
        let server_data = restored_vault.server_data.lock();
        assert_eq!(server_data.state_updating_resumes_at, 555);
        assert_eq!(server_data.total_ejections_needed, 2);
    }
}
