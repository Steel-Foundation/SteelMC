//! Persistent player data structures and NBT serialization.
//!
//! This module defines the data format for saving and loading player state.
//! The format is designed to be vanilla-compatible where possible.

use std::sync::atomic::Ordering;

use simdnbt::{
    ToNbtTag,
    borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView},
    owned::{NbtCompound, NbtList, NbtTag},
};
use steel_registry::item_stack::ItemStack;
use steel_utils::types::GameType;

use crate::inventory::container::Container;

use super::{Player, abilities::Abilities};

/// Current data version for player saves.
/// Increment when making breaking changes to the format.
pub const PLAYER_DATA_VERSION: i32 = 1;

/// Persistent player data that can be serialized to/from NBT.
///
/// This structure mirrors vanilla Minecraft's player data format where possible,
/// allowing for potential compatibility with vanilla tools.
///
/// # TODO: Missing vanilla fields
/// The following fields should be added once their systems are implemented:
/// - Food data: `foodLevel`, `foodSaturationLevel`, `foodExhaustionLevel`, `foodTickTimer`
/// - Experience: `XpP` (progress), `XpLevel`, `XpTotal`, `XpSeed`
/// - Active potion effects: `active_effects` (List)
/// - Score: `Score` (Int)
/// - Ender chest inventory: `EnderItems` (List)
/// - Last death location: `LastDeathLocation` (`GlobalPos`)
/// - Respawn position: `SpawnX`, `SpawnY`, `SpawnZ`, `SpawnDimension`, `SpawnForced`, `SpawnAngle`
#[derive(Debug, Clone)]
pub struct PersistentPlayerData {
    /// Position (x, y, z) in absolute world coordinates.
    /// NBT tag: `Pos` (`DoubleList`)
    pub pos: [f64; 3],

    /// Velocity (x, y, z) in blocks per tick.
    /// NBT tag: `Motion` (`DoubleList`)
    pub motion: [f64; 3],

    /// Rotation (yaw, pitch) in degrees.
    /// NBT tag: `Rotation` (`FloatList`)
    pub rotation: [f32; 2],

    /// Whether the player is on the ground.
    /// NBT tag: `OnGround` (Byte)
    pub on_ground: bool,

    /// Whether the player is elytra gliding.
    /// NBT tag: `FallFlying` (Byte)
    pub fall_flying: bool,

    /// Current health points.
    /// NBT tag: `Health` (Float)
    pub health: f32,

    /// Current game mode (0=survival, 1=creative, 2=adventure, 3=spectator).
    /// NBT tag: `playerGameType` (Int)
    pub game_mode: i32,

    /// Player abilities (flight, invulnerability, etc.).
    /// NBT tag: `abilities` (Compound)
    pub abilities: PersistentAbilities,

    /// Inventory items with slot indices.
    /// NBT tag: `Inventory` (List of Compounds)
    pub inventory: Vec<PersistentSlot>,

    /// Currently selected hotbar slot (0-8).
    /// NBT tag: `SelectedItemSlot` (Int)
    pub selected_slot: i32,

    /// Dimension identifier (e.g., "minecraft:overworld").
    /// NBT tag: `Dimension` (String)
    pub dimension: String,

    /// Data version for format migrations.
    /// NBT tag: `DataVersion` (Int)
    pub data_version: i32,
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
        let pos = *player.position.lock();
        let (yaw, pitch) = player.rotation.load();
        let delta = *player.delta_movement.lock();
        let abilities = player.abilities.lock();
        let inventory = player.inventory.lock();
        let entity_data = player.entity_data.lock();

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

        Self {
            pos: [pos.x, pos.y, pos.z],
            motion: [delta.x, delta.y, delta.z],
            rotation: [yaw, pitch],
            on_ground: player.on_ground.load(Ordering::Relaxed),
            fall_flying: player.fall_flying.load(Ordering::Relaxed),
            health: *entity_data.health.get(),
            game_mode: player.game_mode.load() as i32,
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
            dimension: player.world.dimension.key.to_string(),
            data_version: PLAYER_DATA_VERSION,
        }
    }

    /// Serializes the player data to an NBT compound.
    #[must_use]
    pub fn to_nbt(&self) -> NbtCompound {
        let mut compound = NbtCompound::new();

        // Position
        let pos_list = NbtList::from(vec![
            NbtTag::Double(self.pos[0]),
            NbtTag::Double(self.pos[1]),
            NbtTag::Double(self.pos[2]),
        ]);
        compound.insert("Pos", pos_list);

        // Motion
        let motion_list = NbtList::from(vec![
            NbtTag::Double(self.motion[0]),
            NbtTag::Double(self.motion[1]),
            NbtTag::Double(self.motion[2]),
        ]);
        compound.insert("Motion", motion_list);

        // Rotation
        let rotation_list = NbtList::from(vec![
            NbtTag::Float(self.rotation[0]),
            NbtTag::Float(self.rotation[1]),
        ]);
        compound.insert("Rotation", rotation_list);

        // Simple fields
        compound.insert("OnGround", i8::from(self.on_ground));
        compound.insert("FallFlying", i8::from(self.fall_flying));
        compound.insert("Health", self.health);
        compound.insert("playerGameType", self.game_mode);
        compound.insert("SelectedItemSlot", self.selected_slot);
        compound.insert("Dimension", self.dimension.clone());
        compound.insert("DataVersion", self.data_version);

        // Abilities compound
        compound.insert("abilities", self.abilities.to_nbt());

        // Inventory list
        let inventory_list: Vec<NbtTag> = self
            .inventory
            .iter()
            .map(|slot| {
                let mut item_compound = match slot.item.clone().to_nbt_tag() {
                    NbtTag::Compound(c) => c,
                    _ => NbtCompound::new(),
                };
                item_compound.insert("Slot", slot.slot);
                NbtTag::Compound(item_compound)
            })
            .collect();
        compound.insert("Inventory", NbtList::from(inventory_list));

        compound
    }

    /// Deserializes player data from an NBT compound.
    ///
    /// Returns `None` if required fields are missing or invalid.
    #[must_use]
    pub fn from_nbt(nbt: &BorrowedNbtCompound<'_>) -> Option<Self> {
        // Convert to view type to access accessor methods
        let nbt: NbtCompoundView<'_, '_> = nbt.into();
        // Position
        let pos_list = nbt.list("Pos")?;
        let pos = [
            pos_list.doubles()?.first().copied()?,
            pos_list.doubles()?.get(1).copied()?,
            pos_list.doubles()?.get(2).copied()?,
        ];

        // Motion (optional, default to zero)
        let motion =
            nbt.list("Motion")
                .and_then(|l| l.doubles())
                .map_or([0.0, 0.0, 0.0], |doubles| {
                    [
                        doubles.first().copied().unwrap_or(0.0),
                        doubles.get(1).copied().unwrap_or(0.0),
                        doubles.get(2).copied().unwrap_or(0.0),
                    ]
                });

        // Rotation (optional, default to zero)
        let rotation = nbt
            .list("Rotation")
            .and_then(|l| l.floats())
            .map_or([0.0, 0.0], |floats| {
                [
                    floats.first().copied().unwrap_or(0.0),
                    floats.get(1).copied().unwrap_or(0.0),
                ]
            });

        // Simple fields with defaults
        let on_ground = nbt.byte("OnGround") != Some(0);
        let fall_flying = nbt.byte("FallFlying").is_some_and(|b| b != 0);
        let health = nbt.float("Health").unwrap_or(20.0);
        let game_mode = nbt.int("playerGameType").unwrap_or(0);
        let selected_slot = nbt.int("SelectedItemSlot").unwrap_or(0);
        let dimension = nbt.string("Dimension").map_or_else(
            || "minecraft:overworld".to_string(),
            |s| s.to_str().to_string(),
        );
        let data_version = nbt.int("DataVersion").unwrap_or(0);

        // Abilities
        let abilities = nbt
            .compound("abilities")
            .map(|c| PersistentAbilities::from_nbt(&c))
            .unwrap_or_default();

        // Inventory
        let mut inventory = Vec::new();
        if let Some(inv_list) = nbt.list("Inventory")
            && let Some(compounds) = inv_list.compounds()
        {
            for item_compound in compounds {
                let slot = item_compound.byte("Slot").unwrap_or(0);
                if let Some(item) = ItemStack::from_borrowed_compound(&item_compound) {
                    inventory.push(PersistentSlot { slot, item });
                }
            }
        }

        Some(Self {
            pos,
            motion,
            rotation,
            on_ground,
            fall_flying,
            health,
            game_mode,
            abilities,
            inventory,
            selected_slot,
            dimension,
            data_version,
        })
    }
}

impl PersistentAbilities {
    /// Serializes abilities to an NBT compound.
    #[must_use]
    pub fn to_nbt(&self) -> NbtCompound {
        let mut compound = NbtCompound::new();
        compound.insert("invulnerable", i8::from(self.invulnerable));
        compound.insert("flying", i8::from(self.flying));
        compound.insert("mayfly", i8::from(self.may_fly));
        compound.insert("instabuild", i8::from(self.instabuild));
        compound.insert("mayBuild", i8::from(self.may_build));
        compound.insert("flySpeed", self.flying_speed);
        compound.insert("walkSpeed", self.walking_speed);
        compound
    }

    /// Deserializes abilities from an NBT compound.
    #[must_use]
    pub fn from_nbt(nbt: &NbtCompoundView<'_, '_>) -> Self {
        Self {
            invulnerable: nbt.byte("invulnerable").is_some_and(|b| b != 0),
            flying: nbt.byte("flying").is_some_and(|b| b != 0),
            may_fly: nbt.byte("mayfly").is_some_and(|b| b != 0),
            instabuild: nbt.byte("instabuild").is_some_and(|b| b != 0),
            may_build: nbt.byte("mayBuild") != Some(0),
            flying_speed: nbt.float("flySpeed").unwrap_or(0.05),
            walking_speed: nbt.float("walkSpeed").unwrap_or(0.1),
        }
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
        use steel_utils::math::Vector3;

        // Position
        *player.position.lock() = Vector3::new(self.pos[0], self.pos[1], self.pos[2]);

        // Rotation
        player.rotation.store((self.rotation[0], self.rotation[1]));

        // Motion/velocity
        *player.delta_movement.lock() =
            Vector3::new(self.motion[0], self.motion[1], self.motion[2]);

        // Ground state
        player.on_ground.store(self.on_ground, Ordering::Relaxed);
        player
            .fall_flying
            .store(self.fall_flying, Ordering::Relaxed);

        // Health
        player.entity_data.lock().health.set(self.health);

        // Game mode
        let game_mode = match self.game_mode {
            1 => GameType::Creative,
            2 => GameType::Adventure,
            3 => GameType::Spectator,
            _ => GameType::Survival,
        };
        player.game_mode.store(game_mode);

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
    }
}
