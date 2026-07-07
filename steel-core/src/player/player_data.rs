//! Persistent player data structures.
//!
//! This module defines the data format for saving and loading player state.

use rustc_hash::FxHashSet;
use steel_registry::item_stack::ItemStack;
use steel_utils::types::GameType;

use crate::{
    chunk_saver::{ChunkStorage, PersistentEntity},
    entity::{Entity, EntityFireFreezeState, LivingEntity},
    inventory::container::Container,
};

use super::{Player, abilities::Abilities};

/// Current data version for player saves.
/// Increment when making breaking changes to the format.
pub const PLAYER_DATA_VERSION: i32 = 5;

/// Persistent player data saved by Steel's storage backend.
///
/// This is Steel's runtime save snapshot. Vanilla import/export should live outside
/// server runtime storage so compatibility logic does not constrain the native format.
#[derive(Debug, Clone)]
pub struct PersistentPlayerData {
    /// Position (x, y, z) in absolute world coordinates.
    pub pos: [f64; 3],

    /// Velocity (x, y, z) in blocks per tick.
    pub motion: [f64; 3],

    /// Rotation (yaw, pitch) in degrees.
    pub rotation: [f32; 2],

    /// Whether the player is on the ground.
    pub on_ground: bool,

    /// Whether the player is elytra gliding.
    pub fall_flying: bool,

    /// Vanilla `remainingFireTicks`.
    pub remaining_fire_ticks: i32,

    /// Synchronized vanilla `TicksFrozen`.
    pub ticks_frozen: i32,

    /// Vanilla `isInPowderSnow`.
    pub is_in_powder_snow: bool,

    /// Vanilla `wasInPowderSnow`.
    pub was_in_powder_snow: bool,

    /// Vanilla `hasVisualFire`.
    pub has_visual_fire: bool,

    /// Current health points.
    pub health: f32,

    /// Current game mode (0=survival, 1=creative, 2=adventure, 3=spectator).
    pub game_mode: i32,

    /// Previous game mode of the player, or `None` if vanilla has not recorded one yet.
    pub prev_game_mode: Option<i32>,

    /// Player abilities (flight, invulnerability, etc.).
    pub abilities: PersistentAbilities,

    /// Inventory items with slot indices.
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

    /// To progress to the next experience level
    pub experience_progress: f32,

    /// The checked value of the Score, cannot decrease below 0 (???)
    /// TODO: what exactly is experienceTotal
    pub experience_total: i32,

    /// A non decreasing value of the experience orbs added (/xp add, picking up orbs and advancements)
    /// this value can be negative by using (/xp add ... -x)
    pub score: i32,

    /// Vanilla `ServerPlayer.seenCredits`.
    pub seen_credits: bool,

    /// Vanilla one-player root vehicle tree stored with the player instead of chunk data.
    pub root_vehicle: Option<PersistentRootVehicle>,

    /// Vanilla in-flight ender pearls stored with the player (`ServerPlayer.enderPearls`).
    pub ender_pearls: Vec<PersistentEnderPearl>,
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

impl PersistentPlayerData {
    /// Extracts persistent data from a live player.
    #[must_use]
    pub fn from_player(player: &Player) -> Self {
        let pos = player.position();
        let (yaw, pitch) = player.rotation();
        let delta = player.velocity();
        let on_ground = player.on_ground();
        let fall_flying = player.is_fall_flying();
        let fire_freeze = player.fire_freeze_state();
        let abilities = player.abilities.lock();
        let inventory = player.inventory.lock();
        let food_data = player.food_data.lock();

        // Collect non-empty inventory slots
        let mut slots = Vec::new();
        // Main inventory (0-35) and equipment (36-42)
        for slot in 0..43 {
            let item = inventory.get_item(slot);
            if !item.is_empty() {
                slots.push(PersistentSlot {
                    slot: slot as i8,
                    item: item.clone(),
                });
            }
        }

        let (experience_level, experience_progress, experience_total, score) = {
            let lock = player.experience.lock();
            (
                lock.level(),
                lock.progress() as f32,
                lock.total_points(),
                lock.score,
            )
        };
        let root_vehicle = Self::root_vehicle_from_player(player)
            .or_else(|| player.pending_root_vehicle_for_current_world());
        let ender_pearls = Self::ender_pearls_from_player(player);

        Self {
            pos: [pos.x, pos.y, pos.z],
            motion: [delta.x, delta.y, delta.z],
            rotation: [yaw, pitch],
            on_ground,
            fall_flying,
            remaining_fire_ticks: fire_freeze.remaining_fire_ticks(),
            ticks_frozen: fire_freeze.ticks_frozen(),
            is_in_powder_snow: fire_freeze.is_in_powder_snow(),
            was_in_powder_snow: fire_freeze.was_in_powder_snow(),
            has_visual_fire: fire_freeze.has_visual_fire(),
            health: player.get_health(),
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
            score,
            seen_credits: player.has_seen_credits(),
            root_vehicle,
            ender_pearls,
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
    /// Used when the saved world no longer exists and the player must spawn at
    /// the target world's default spawn instead of stale coordinates.
    pub fn apply_to_player_without_location(&self, player: &Player) {
        self.apply_to_player_inner(player, false);
    }

    fn apply_to_player_inner(&self, player: &Player, restore_location: bool) {
        use glam::DVec3;

        if restore_location {
            // Position
            player
                .base()
                .set_position_local(DVec3::new(self.pos[0], self.pos[1], self.pos[2]));

            // Rotation
            player.set_rotation((self.rotation[0], self.rotation[1]));

            // Motion/velocity
            player.set_velocity(DVec3::new(self.motion[0], self.motion[1], self.motion[2]));

            // Ground state
            player.set_fall_flying(self.fall_flying);
            player.set_on_ground(self.on_ground);
        }

        player
            .base()
            .set_fire_freeze_state(EntityFireFreezeState::from_parts(
                self.remaining_fire_ticks,
                self.ticks_frozen,
                self.is_in_powder_snow,
                self.was_in_powder_snow,
                self.has_visual_fire,
            ));
        player.sync_base_fire_freeze_entity_data();

        // Health
        player.set_health(self.health);

        // Game mode
        player.restore_game_modes(
            self.game_mode.into(),
            self.prev_game_mode.map(GameType::from),
        );

        // Abilities
        *player.abilities.lock() = self.abilities.clone().into();

        // Inventory
        {
            let mut inventory = player.inventory.lock();
            // Clear existing inventory first
            for slot in 0..43 {
                inventory.set_item(slot, ItemStack::empty());
            }
            // Restore saved items
            for slot_data in &self.inventory {
                let slot_index = slot_data.slot as usize;
                if slot_index < 43 {
                    inventory.set_item(slot_index, slot_data.item.clone());
                }
            }
            // Restore selected slot
            let selected = self.selected_slot.clamp(0, 8) as u8;
            inventory.set_selected_slot(selected);
        }

        // Food data
        {
            let mut food = player.food_data.lock();
            food.food_level = self.food_level;
            food.saturation_level = self.food_saturation_level;
            food.exhaustion_level = self.food_exhaustion_level;
            food.tick_timer = self.food_tick_timer;
        }

        {
            let mut experience = player.experience.lock();
            experience.set_levels(self.experience_level);
            experience.set_progress(f64::from(self.experience_progress));
            experience.score = self.score;
        }
        player.set_seen_credits(self.seen_credits);
    }
}
