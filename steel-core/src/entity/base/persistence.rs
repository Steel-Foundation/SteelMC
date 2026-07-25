use std::{collections::BTreeSet, sync::Weak};

use glam::DVec3;
use simdnbt::owned::NbtCompound;
use text_components::TextComponent;
use uuid::Uuid;

use super::{DEFAULT_MAX_AIR_SUPPLY, EntityFireFreezeState, MAX_ENTITY_TAGS};
use crate::world::World;

/// Shared vanilla entity save data that is not part of the movement snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityBaseSaveData {
    /// Synchronized vanilla `Air`/air supply value.
    pub air_supply: i32,
    /// Vanilla dimension-change portal cooldown.
    pub portal_cooldown: i32,
    /// Shared vanilla `NoGravity` flag.
    pub no_gravity: bool,
    /// Shared vanilla `Invulnerable` flag.
    pub invulnerable: bool,
    /// Optional synchronized vanilla custom name.
    pub custom_name: Option<TextComponent>,
    /// Synchronized vanilla custom-name visibility flag.
    pub custom_name_visible: bool,
    /// Synchronized vanilla silent flag.
    pub silent: bool,
    /// Server-owned vanilla glowing tag, projected into the shared flags byte.
    pub glowing: bool,
    /// Vanilla scoreboard tags.
    pub tags: BTreeSet<String>,
    /// Vanilla custom data component payload.
    pub custom_data: NbtCompound,
}

impl EntityBaseSaveData {
    /// Creates default vanilla base save data.
    #[must_use]
    pub fn new() -> Self {
        Self {
            air_supply: DEFAULT_MAX_AIR_SUPPLY,
            portal_cooldown: 0,
            no_gravity: false,
            invulnerable: false,
            custom_name: None,
            custom_name_visible: false,
            silent: false,
            glowing: false,
            tags: BTreeSet::new(),
            custom_data: NbtCompound::new(),
        }
    }

    /// Adds a scoreboard tag, respecting vanilla's per-entity tag limit.
    pub fn add_tag(&mut self, tag: String) -> bool {
        if self.tags.len() >= MAX_ENTITY_TAGS && !self.tags.contains(&tag) {
            return false;
        }
        self.tags.insert(tag)
    }
}

impl Default for EntityBaseSaveData {
    fn default() -> Self {
        Self::new()
    }
}

/// Base fields restored from persistent entity data.
///
/// Vanilla loads these fields through `Entity.load` before type-specific
/// entity data. Keeping them bundled makes the load boundary explicit and
/// prevents constructor signatures from drifting as base state grows.
#[derive(Debug, Clone)]
pub struct EntityBaseLoad {
    /// Fresh runtime ID from `next_entity_id()`.
    pub id: i32,
    /// Restored entity position.
    pub position: DVec3,
    /// Persisted entity UUID.
    pub uuid: Uuid,
    /// Restored velocity.
    pub velocity: DVec3,
    /// Restored yaw and pitch.
    pub rotation: (f32, f32),
    /// Restored accumulated fall distance.
    pub fall_distance: f64,
    /// Restored vanilla fire/freeze state.
    pub fire_freeze: EntityFireFreezeState,
    /// Restored ground-contact flag.
    pub on_ground: bool,
    /// Restored shared vanilla save data.
    pub save_data: EntityBaseSaveData,
    /// World reference for the loaded entity.
    pub world: Weak<World>,
}
