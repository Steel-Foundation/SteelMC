//! Persistent player data structures.
//!
//! This module defines the data format for saving and loading player state.

use crate::entity::prelude::*;
use rustc_hash::FxHashSet;
use steel_registry::item_stack::ItemStack;
use steel_registry::stat::Stat;
use steel_utils::Identifier;
use steel_utils::types::GameType;

use super::advancement::PlayerAdvancement;
use super::advancement::progress::AdvancementProgress;
use super::{
    Player, PlayerRespawnConfig, abilities::Abilities, experience::Experience, food_data::FoodData,
    player_inventory::PlayerInventory,
};
use crate::player::stats_counter::StatState;
use crate::{
    chunk_saver::{ChunkStorage, PersistentEntity},
    entity::Entity,
    inventory::{
        container::Container,
        ender_chest::{ENDER_CHEST_SLOTS, PlayerEnderChestContainer},
    },
};

/// Current data version for player saves.
/// Increment when making breaking changes to the format.
pub const PLAYER_DATA_VERSION: i32 = 9;

/// Persistent player data saved by Steel's storage backend.
///
/// This is Steel's runtime save snapshot. Vanilla import/export should live outside
/// server runtime storage so compatibility logic does not constrain the native format.
#[derive(Debug, Clone)]
pub struct PersistentPlayerData {
    /// What the player saves as an entity: the base entity fields and its
    /// components, in the form chunk entities are saved in. It holds no
    /// passengers; a vehicle is kept in [`Self::root_vehicle`].
    pub entity: PersistentEntity,

    /// Current game mode (0=survival, 1=creative, 2=adventure, 3=spectator).
    pub game_mode: i32,

    /// Previous game mode of the player, or `None` if vanilla has not recorded one yet.
    pub prev_game_mode: Option<i32>,

    /// Player abilities (flight, invulnerability, etc.).
    pub abilities: PersistentAbilities,

    /// Main inventory items with slot indices; equipment is saved with
    /// [`Self::entity`].
    pub inventory: Vec<PersistentSlot>,

    /// Currently selected hotbar slot (0-8).
    pub selected_slot: i32,

    /// Loaded world identifier (e.g., "minecraft:overworld").
    pub world: String,

    /// Current food level (0–20, default 20).
    pub food_level: i32,

    /// Food saturation level (0.0–`food_level`, default 5.0).
    pub food_saturation_level: f32,

    /// Accumulated food exhaustion (0.0–40.0, default 0.0).
    pub food_exhaustion_level: f32,

    /// Internal tick timer for regen/starvation (default 0).
    pub food_tick_timer: i32,

    /// Data version for format migrations.
    pub data_version: i32,

    /// Current experience level
    pub experience_level: i32,

    /// Vanilla progress toward the next experience level.
    pub experience_progress: f32,

    /// Vanilla `Player.totalExperience`, updated by point grants but independent of level/progress.
    pub experience_total: i32,

    /// Saved enchantment seed (`XpSeed`); zero is rerolled when loaded.
    pub enchantment_seed: i32,

    /// Vanilla death-screen score. Point grants change it with Java `int` wrapping.
    pub score: i32,

    /// Whether this player has already seen the End credits screen.
    pub seen_credits: bool,

    /// Whether the next landing sends the mace smash burst.
    pub spawn_extra_particles_on_fall: bool,

    /// Vanilla one-player root vehicle tree stored with the player instead of chunk data.
    pub root_vehicle: Option<PersistentRootVehicle>,

    /// Vanilla per-player respawn configuration set by beds and respawn anchors.
    pub respawn_config: Option<PlayerRespawnConfig>,

    /// Vanilla in-flight ender pearls stored with the player (`ServerPlayer.enderPearls`).
    pub ender_pearls: Vec<PersistentEnderPearl>,

    /// Vanilla `EnderChest` items.
    pub ender_items: Vec<PersistentSlot>,

    /// The tracked statistics of the player with their counters.
    pub stats: Vec<PersistentStat>,

    /// Persisted advancement progress keyed by advancement identifier.
    pub advancements: Vec<(Identifier, AdvancementProgress)>,
}

/// A vanilla `RootVehicle` tree persisted with player data.
#[derive(Debug, Clone)]
pub struct PersistentRootVehicle {
    /// UUID of the direct vehicle the player should reattach to.
    pub attach: [u8; 16],
    /// Root vehicle entity tree.
    pub entity: PersistentEntity,
}

/// A thrown ender pearl persisted with its owning player.
///
/// Mirrors a vanilla `ender_pearls` list entry: the pearl entity plus the world
/// it lives in (`ender_pearl_dimension`), so it re-spawns in its original world.
#[derive(Debug, Clone)]
pub struct PersistentEnderPearl {
    /// Key of the world the pearl lives in.
    pub world: String,
    /// Serialized pearl entity.
    pub entity: PersistentEntity,
}

/// Persistent abilities data.
#[derive(Debug, Clone)]
pub struct PersistentAbilities {
    /// Whether the player is invulnerable to damage.
    pub invulnerable: bool,
    /// Whether the player is currently flying.
    pub flying: bool,
    /// Whether the player is allowed to fly.
    pub may_fly: bool,
    /// Whether the player can instantly break blocks (creative mode).
    pub instabuild: bool,
    /// Whether the player can place/break blocks.
    pub may_build: bool,
    /// Flying speed (default 0.05).
    pub flying_speed: f32,
    /// Walking speed (default 0.1).
    pub walking_speed: f32,
}

/// An inventory slot with its index.
#[derive(Debug, Clone)]
pub struct PersistentSlot {
    /// Slot index in the inventory.
    pub slot: i8,
    /// The item stack in this slot.
    pub item: ItemStack,
}

/// Represents a tracked stat with its counter.
#[derive(Debug, Clone)]
pub struct PersistentStat {
    /// The stat being tracked.
    pub stat: Stat,
    /// The current count of this stat.
    pub count: i32,
}

impl PersistentPlayerData {
    /// Extracts persistent data from a live player.
    #[must_use]
    pub fn from_player(player: &Player) -> Self {
        // Saving the entity reads the inventory as equipment, so it comes
        // before the inventory lock below.
        let entity = ChunkStorage::entity_alone_to_persistent(player, player.position());
        let abilities = player.abilities.lock();
        let inventory = player.inventory.lock();
        let food_data = player.food_data.lock();

        // Equipment slots are saved with the entity.
        let mut slots = Vec::new();
        for slot in 0..PlayerInventory::INVENTORY_SIZE {
            let item = inventory.get_item(slot);
            if !item.is_empty() {
                slots.push(PersistentSlot {
                    slot: slot as i8,
                    item: item.clone(),
                });
            }
        }

        let ender_chest_inventory = player.ender_chest_inventory.lock();
        let mut ender_items = Vec::new();
        for slot in 0..ENDER_CHEST_SLOTS {
            let item = ender_chest_inventory.get_item(slot);
            if !item.is_empty() {
                ender_items.push(PersistentSlot {
                    slot: slot as i8,
                    item: item.clone(),
                });
            }
        }

        let (experience_level, experience_progress, experience_total, enchantment_seed) = {
            let lock = player.experience.lock();
            (
                lock.level(),
                lock.progress(),
                lock.total_points(),
                lock.enchantment_seed(),
            )
        };
        let score = player.score();
        let root_vehicle = Self::root_vehicle_from_player(player)
            .or_else(|| player.pending_root_vehicle_for_current_world());
        let ender_pearls = Self::ender_pearls_from_player(player);
        let stats = Self::stats_from_player(player);
        let advancements = player.session.advancements.lock().persisted_progress();

        Self {
            entity,
            game_mode: player.game_mode() as i32,
            prev_game_mode: player
                .previous_game_mode()
                .map(|game_mode| game_mode as i32),
            abilities: PersistentAbilities {
                invulnerable: abilities.invulnerable,
                flying: abilities.flying,
                may_fly: abilities.may_fly,
                instabuild: abilities.instabuild,
                may_build: abilities.may_build,
                flying_speed: abilities.flying_speed,
                walking_speed: abilities.walking_speed,
            },
            inventory: slots,
            selected_slot: i32::from(inventory.get_selected_slot()),
            world: player.get_world().key.to_string(),
            food_level: food_data.food_level,
            food_saturation_level: food_data.saturation_level,
            food_exhaustion_level: food_data.exhaustion_level,
            food_tick_timer: food_data.tick_timer,
            data_version: PLAYER_DATA_VERSION,
            experience_level,
            experience_progress,
            experience_total,
            enchantment_seed,
            score,
            seen_credits: player.has_seen_credits(),
            spawn_extra_particles_on_fall: player.spawn_extra_particles_on_fall(),
            root_vehicle,
            respawn_config: player.respawn_config(),

            ender_pearls,
            ender_items,
            stats,
            advancements,
        }
    }

    /// Snapshots the player's live in-flight ender pearls for persistence.
    fn ender_pearls_from_player(player: &Player) -> Vec<PersistentEnderPearl> {
        let mut seen = FxHashSet::default();
        let mut pearls = player
            .ender_pearls()
            .iter()
            .filter_map(|pearl| {
                let world = pearl.level()?.key.to_string();
                let entity = ChunkStorage::entity_tree_to_persistent(pearl)?;
                seen.insert(entity.uuid);
                Some(PersistentEnderPearl { world, entity })
            })
            .collect::<Vec<_>>();
        pearls.extend(
            player
                .pending_ender_pearls()
                .into_iter()
                .filter(|pearl| seen.insert(pearl.entity.uuid)),
        );
        pearls
    }

    /// Snapshots the player's tracked stats and their counters for persistence.
    fn stats_from_player(player: &Player) -> Vec<PersistentStat> {
        player
            .stats()
            .into_iter()
            .map(|(stat, count)| PersistentStat { stat, count })
            .collect()
    }

    fn root_vehicle_from_player(player: &Player) -> Option<PersistentRootVehicle> {
        let vehicle = player.vehicle()?;
        let root_vehicle = player.root_vehicle()?;
        if root_vehicle.id() == player.id() || !root_vehicle.has_exactly_one_player_passenger() {
            return None;
        }

        let entity = ChunkStorage::entity_tree_to_persistent(&root_vehicle)?;
        Some(PersistentRootVehicle {
            attach: *vehicle.uuid().as_bytes(),
            entity,
        })
    }
}

impl Player {
    /// Resets domain-scoped gameplay data to the defaults used for a new player.
    pub(crate) fn reset_domain_data_for_first_visit(&self) {
        use glam::DVec3;

        self.set_velocity(DVec3::ZERO);
        self.set_on_ground(false);
        *self.abilities.lock() = Abilities::default();
        *self.inventory.lock() = PlayerInventory::new();
        if !ChunkStorage::restore_entity(
            self,
            &ChunkStorage::new_persistent_entity(self.entity_type()),
            false,
        ) {
            log::error!(
                "Could not reset the entity state of player {}",
                self.gameprofile.name()
            );
        }
        *self.ender_chest_inventory.lock() = PlayerEnderChestContainer::new();
        *self.food_data.lock() = FoodData::new();
        self.session.stats.lock().reset();
        *self.session.advancements.lock() = PlayerAdvancement::default();

        let mut experience = Experience::default();
        experience.dirty = true;
        *self.experience.lock() = experience;

        self.set_score(0);
        self.set_seen_credits(false);
        self.set_spawn_extra_particles_on_fall(false);
        self.set_respawn_position(None, false);
    }
}

impl Default for PersistentAbilities {
    fn default() -> Self {
        Self {
            invulnerable: false,
            flying: false,
            may_fly: false,
            instabuild: false,
            may_build: true,
            flying_speed: 0.05,
            walking_speed: 0.1,
        }
    }
}

impl From<&Abilities> for PersistentAbilities {
    fn from(abilities: &Abilities) -> Self {
        Self {
            invulnerable: abilities.invulnerable,
            flying: abilities.flying,
            may_fly: abilities.may_fly,
            instabuild: abilities.instabuild,
            may_build: abilities.may_build,
            flying_speed: abilities.flying_speed,
            walking_speed: abilities.walking_speed,
        }
    }
}

impl From<PersistentAbilities> for Abilities {
    fn from(persistent: PersistentAbilities) -> Self {
        Self {
            invulnerable: persistent.invulnerable,
            flying: persistent.flying,
            may_fly: persistent.may_fly,
            instabuild: persistent.instabuild,
            may_build: persistent.may_build,
            flying_speed: persistent.flying_speed,
            walking_speed: persistent.walking_speed,
        }
    }
}

impl PersistentPlayerData {
    /// Applies the saved data to a player.
    ///
    /// This restores position, rotation, inventory, abilities, etc.
    pub fn apply_to_player(&self, player: &Player) {
        self.apply_to_player_inner(player, true);
    }

    /// Applies saved gameplay state without restoring world-local location data.
    ///
    /// Used when the saved world is unavailable or differs from an explicitly
    /// selected world, which must use the target spawn instead.
    pub fn apply_to_player_without_location(&self, player: &Player) {
        self.apply_to_player_inner(player, false);
    }

    fn apply_to_player_inner(&self, player: &Player, restore_location: bool) {
        let Self {
            entity,
            game_mode,
            prev_game_mode,
            abilities,
            inventory,
            selected_slot,
            // The caller resolves the world before applying.
            world: _,
            food_level,
            food_saturation_level,
            food_exhaustion_level,
            food_tick_timer,
            data_version: _,
            experience_level,
            experience_progress,
            experience_total,
            enchantment_seed,
            score,
            seen_credits,
            spawn_extra_particles_on_fall,
            // The vehicle and pearls are restored once the player is in the world.
            root_vehicle: _,
            respawn_config,
            ender_pearls: _,
            ender_items,
            stats,
            advancements,
        } = self;

        if !ChunkStorage::restore_entity(player, entity, restore_location) {
            log::error!(
                "Could not restore the saved entity state of player {}",
                player.gameprofile.name()
            );
        }
        if !restore_location {
            // Gliding and sleeping belong to the place the player was saved at.
            player.set_fall_flying(false);
            player.living().clear_sleeping_pos();
        }
        player.set_respawn_position(respawn_config.clone(), false);

        // Game mode
        player.restore_game_modes((*game_mode).into(), prev_game_mode.map(GameType::from));

        // Abilities
        *player.abilities.lock() = abilities.clone().into();

        // Inventory
        {
            let mut player_inventory = player.inventory.lock();
            // The equipment slots were restored with the entity.
            for slot in 0..PlayerInventory::INVENTORY_SIZE {
                player_inventory.set_item(slot, ItemStack::empty());
            }
            for slot_data in inventory {
                let slot_index = slot_data.slot as usize;
                if slot_index < PlayerInventory::INVENTORY_SIZE {
                    player_inventory.set_item(slot_index, slot_data.item.clone());
                }
            }
            // Restore selected slot
            let selected = (*selected_slot).clamp(0, 8) as u8;
            player_inventory.set_selected_slot(selected);
        }

        {
            let mut ender_chest_inventory = player.ender_chest_inventory.lock();
            for slot in 0..ENDER_CHEST_SLOTS {
                ender_chest_inventory.set_item(slot, ItemStack::empty());
            }
            for slot_data in ender_items {
                let slot_index = slot_data.slot as usize;
                if slot_index < ENDER_CHEST_SLOTS {
                    ender_chest_inventory.set_item(slot_index, slot_data.item.clone());
                }
            }
        }

        // Food data
        {
            let mut food = player.food_data.lock();
            food.food_level = *food_level;
            food.saturation_level = *food_saturation_level;
            food.exhaustion_level = *food_exhaustion_level;
            food.tick_timer = *food_tick_timer;
        }

        {
            let mut experience = player.experience.lock();
            *experience =
                Experience::from_parts(*experience_level, *experience_progress, *experience_total)
                    .with_loaded_enchantment_seed(*enchantment_seed, rand::random);
        }
        player.set_score(*score);
        player.set_seen_credits(*seen_credits);
        player.set_spawn_extra_particles_on_fall(*spawn_extra_particles_on_fall);

        // Statistics
        {
            let mut counters = player.session.stats.lock();

            // This resets all counters to zero, but also marks them as non-persistent. However, they still need
            // to be sent to the client the next time they are queried.
            // This is important so that stale data from the previous domain doesn't stay in the client's cache.
            // We then populate the counter with all the serialized stat counters from the new domain.
            counters.reset();

            for PersistentStat { stat, count } in stats {
                counters.stats.insert(*stat, (*count, StatState::Dirty));
            }
        }

        player
            .session
            .advancements
            .lock()
            .apply_persisted_progress(advancements.iter().cloned());
    }
}
