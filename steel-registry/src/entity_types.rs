use rustc_hash::FxHashMap;
use steel_utils::Identifier;
use steel_utils::registry::registry_vanilla_or_custom_tag;

use crate::RegistryExt;

/// Mob category for spawn classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MobCategory {
    Monster,
    Creature,
    Ambient,
    Axolotls,
    UndergroundWaterCreature,
    WaterCreature,
    WaterAmbient,
    Misc,
}

/// Entity dimensions used for bounding box calculation.
/// Bounding box is centered on X/Z with Y at entity feet.
#[derive(Debug, Clone, Copy)]
pub struct EntityDimensions {
    pub width: f32,
    pub height: f32,
    pub eye_height: f32,
}

impl EntityDimensions {
    /// Creates new entity dimensions.
    #[must_use]
    pub const fn new(width: f32, height: f32, eye_height: f32) -> Self {
        Self {
            width,
            height,
            eye_height,
        }
    }

    /// Scale dimensions by a factor (for baby entities, etc.)
    #[must_use]
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            width: self.width * factor,
            height: self.height * factor,
            eye_height: self.eye_height * factor,
        }
    }

    /// Get the half-width for bounding box calculation.
    #[must_use]
    pub fn half_width(&self) -> f32 {
        self.width / 2.0
    }
}

/// Behavioral flags for entity collision and interaction.
#[derive(Debug, Clone, Copy)]
pub struct EntityFlags {
    pub is_pushable: bool,
    pub is_attackable: bool,
    pub is_pickable: bool,
    pub can_be_collided_with: bool,
    pub is_pushed_by_fluid: bool,
    pub can_freeze: bool,
    pub can_be_hit_by_projectile: bool,
    pub is_sensitive_to_water: bool,
    pub can_breathe_underwater: bool,
    pub can_be_seen_as_enemy: bool,
}

#[derive(Debug)]
pub struct EntityType {
    pub key: Identifier,
    pub client_tracking_range: i32,
    pub update_interval: i32,

    /// Default entity dimensions.
    pub dimensions: EntityDimensions,
    /// If true, dimensions cannot be scaled.
    pub fixed: bool,

    /// Mob category for spawn classification.
    pub mob_category: MobCategory,
    /// Whether this entity is immune to fire damage.
    pub fire_immune: bool,
    /// Whether this entity can be summoned via commands.
    pub summonable: bool,
    /// Whether this entity can spawn far from players.
    pub can_spawn_far_from_player: bool,
    /// Whether this entity type can be serialized to disk.
    /// Set to false for transient entities (lightning, fishing hooks, players).
    pub can_serialize: bool,

    /// Behavioral flags for collision and interaction.
    pub flags: EntityFlags,
}

pub type EntityTypeRef = &'static EntityType;

pub struct EntityTypeRegistry {
    types_by_id: Vec<EntityTypeRef>,
    types_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl Default for EntityTypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityTypeRegistry {
    // Creates a new, empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            types_by_id: Vec::new(),
            types_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }

    /// Registers a new entity type
    pub fn register(&mut self, entity_type: EntityTypeRef) {
        assert!(
            self.allows_registering,
            "Cannot register entity types after the registry has been frozen"
        );
        let idx = self.types_by_id.len();
        self.types_by_key.insert(entity_type.key.clone(), idx);
        self.types_by_id.push(entity_type);
    }

    /// Replaces a entity_type at a given index.
    /// Returns true if the entity_type was replaced and false if the entity_type wasn't replaced
    #[must_use]
    pub fn replace(&mut self, entity_type: EntityTypeRef, id: usize) -> bool {
        if id >= self.types_by_id.len() {
            return false;
        }
        self.types_by_id[id] = entity_type;
        true
    }

    #[must_use]
    pub fn by_id(&self, id: i32) -> Option<EntityTypeRef> {
        if id >= 0 {
            self.types_by_id.get(id as usize).copied()
        } else {
            None
        }
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<EntityTypeRef> {
        self.types_by_key
            .get(key)
            .and_then(|&idx| self.types_by_id.get(idx).copied())
    }

    /// Gets the registry ID for an entity type.
    #[must_use]
    pub fn get_id(&self, entity_type: EntityTypeRef) -> &usize {
        self.types_by_key
            .get(&entity_type.key)
            .expect("Entity type not found")
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.types_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.types_by_id.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, EntityTypeRef)> + '_ {
        self.types_by_id
            .iter()
            .enumerate()
            .map(|(id, &et)| (id, et))
    }

    pub fn register_tag(&mut self, tag: Identifier, keys: &[&'static str]) {
        assert!(
            self.allows_registering,
            "Cannot register tags after registry has been frozen"
        );

        let identifiers: Vec<Identifier> = keys
            .iter()
            .filter_map(|key| {
                let ident = registry_vanilla_or_custom_tag(key);
                self.by_key(&ident).map(|_| ident)
            })
            .collect();

        self.tags.insert(tag, identifiers);
    }

    #[must_use]
    pub fn is_in_tag(&self, entry: EntityTypeRef, tag: &Identifier) -> bool {
        self.tags
            .get(tag)
            .is_some_and(|entries| entries.contains(&entry.key))
    }

    pub fn modify_tag(
        &mut self,
        tag: &Identifier,
        f: impl FnOnce(Vec<Identifier>) -> Vec<Identifier>,
    ) {
        let existing = self.tags.remove(tag).unwrap_or_default();
        let entries = f(existing)
            .into_iter()
            .filter(|key| {
                let exists = self.types_by_key.contains_key(key);
                if !exists {
                    tracing::error!(
                        "entity type {key} not found in registry, skipping from tag {tag}"
                    );
                }
                exists
            })
            .collect();
        self.tags.insert(tag.clone(), entries);
    }

    #[must_use]
    pub fn get_tag(&self, tag: &Identifier) -> Option<Vec<EntityTypeRef>> {
        self.tags.get(tag).map(|idents| {
            idents
                .iter()
                .filter_map(|ident| self.by_key(ident))
                .collect()
        })
    }

    pub fn iter_tag(&self, tag: &Identifier) -> impl Iterator<Item = EntityTypeRef> + '_ {
        self.tags
            .get(tag)
            .into_iter()
            .flat_map(|v| v.iter().filter_map(|ident| self.by_key(ident)))
    }

    pub fn tag_keys(&self) -> impl Iterator<Item = &Identifier> {
        self.tags.keys()
    }
}

impl RegistryExt for EntityTypeRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}
