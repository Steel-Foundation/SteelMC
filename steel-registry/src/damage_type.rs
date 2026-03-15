use crate::RegistryExt;
use crate::timeline::TimelineRef;
use rustc_hash::FxHashMap;
use steel_utils::Identifier;
use steel_utils::registry::registry_vanilla_or_custom_tag;

/// Represents a damage type definition from a data pack JSON file.
#[derive(Debug)]
pub struct DamageType {
    pub key: Identifier,
    pub message_id: &'static str,
    pub scaling: DamageScaling,
    pub exhaustion: f32,
    pub effects: DamageEffects,
    pub death_message_type: DeathMessageType,
}

/// How the damage scales with difficulty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageScaling {
    Always,
    WhenCausedByLivingNonPlayer,
    Never,
}

/// The sound effects played when an entity is damaged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageEffects {
    Hurt,
    Thorns,
    Drowning,
    Burning,
    Poking,
    Freezing,
}

/// How the death message is formatted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathMessageType {
    Default,
    FallVariants,
    IntentionalGameDesign,
}

impl DamageType {
    pub fn to_nbt(&self) -> simdnbt::owned::NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};
        let mut compound = NbtCompound::new();
        compound.insert("message_id", self.message_id);
        compound.insert(
            "scaling",
            match self.scaling {
                DamageScaling::Always => "always",
                DamageScaling::WhenCausedByLivingNonPlayer => "when_caused_by_living_non_player",
                DamageScaling::Never => "never",
            },
        );
        compound.insert("exhaustion", self.exhaustion);
        compound.insert(
            "effects",
            match self.effects {
                DamageEffects::Hurt => "hurt",
                DamageEffects::Thorns => "thorns",
                DamageEffects::Drowning => "drowning",
                DamageEffects::Burning => "burning",
                DamageEffects::Poking => "poking",
                DamageEffects::Freezing => "freezing",
            },
        );
        compound.insert(
            "death_message_type",
            match self.death_message_type {
                DeathMessageType::Default => "default",
                DeathMessageType::FallVariants => "fall_variants",
                DeathMessageType::IntentionalGameDesign => "intentional_game_design",
            },
        );
        NbtTag::Compound(compound)
    }
}

pub type DamageTypeRef = &'static DamageType;

pub struct DamageTypeRegistry {
    damage_types_by_id: Vec<DamageTypeRef>,
    damage_types_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl DamageTypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            damage_types_by_id: Vec::new(),
            damage_types_by_key: FxHashMap::default(),
            allows_registering: true,
            tags: FxHashMap::default(),
        }
    }

    pub fn register(&mut self, damage_type: DamageTypeRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register damage types after the registry has been frozen"
        );

        let id = self.damage_types_by_id.len();
        self.damage_types_by_key.insert(damage_type.key.clone(), id);
        self.damage_types_by_id.push(damage_type);
        id
    }

    /// Replaces damage at a given index.
    /// Returns true if the damage was replaced and false if the damage wasn't replaced
    #[must_use]
    pub fn replace(&mut self, damage: DamageTypeRef, id: usize) -> bool {
        if id >= self.damage_types_by_id.len() {
            return false;
        }
        self.damage_types_by_id[id] = damage;
        true
    }

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<DamageTypeRef> {
        self.damage_types_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, damage_type: DamageTypeRef) -> &usize {
        self.damage_types_by_key
            .get(&damage_type.key)
            .expect("Damage type not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<DamageTypeRef> {
        self.damage_types_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, DamageTypeRef)> + '_ {
        self.damage_types_by_id
            .iter()
            .enumerate()
            .map(|(id, &dt)| (id, dt))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.damage_types_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.damage_types_by_id.is_empty()
    }

    /// Registers a tag with a list of damate_type keys.
    /// Damage type keys that don't exist in the registry are silently skipped.
    pub fn register_tag(&mut self, tag: Identifier, timeline_keys: &[&'static str]) {
        assert!(
            self.allows_registering,
            "Cannot register tags after registry has been frozen"
        );

        let identifier: Vec<Identifier> = timeline_keys
            .iter()
            .filter_map(|key| {
                let ident = registry_vanilla_or_custom_tag(key);
                // Only include if the item actually exists
                self.by_key(&ident).map(|_| ident)
            })
            .collect();

        self.tags.insert(tag, identifier);
    }

    /// Checks if a fluid is in a given tag.
    #[must_use]
    pub fn is_in_tag(&self, timeline: TimelineRef, tag: &Identifier) -> bool {
        self.tags
            .get(tag)
            .is_some_and(|timelines| timelines.contains(&timeline.key))
    }

    /// Gives the access to all blocks to delete and add new entries
    pub fn modify_tag(
        &mut self,
        tag: &Identifier,
        f: impl FnOnce(Vec<Identifier>) -> Vec<Identifier>,
    ) {
        let existing = self.tags.remove(tag).unwrap_or_default();
        let timelines = f(existing)
            .into_iter()
            .filter(|timeline| {
                let exists = self.damage_types_by_key.contains_key(timeline);
                if !exists {
                    tracing::error!(
                        "timeline {timeline} not found in registry, skipping from tag {tag}"
                    );
                }
                exists
            })
            .collect();
        self.tags.insert(tag.clone(), timelines);
    }

    /// Gets all damage_types in a tag.
    #[must_use]
    pub fn get_tag(&self, tag: &Identifier) -> Option<Vec<DamageTypeRef>> {
        self.tags.get(tag).map(|idents| {
            idents
                .iter()
                .filter_map(|ident| self.by_key(ident))
                .collect()
        })
    }

    /// Iterates over all damage_type in a tag.
    pub fn iter_tag(&self, tag: &Identifier) -> impl Iterator<Item = DamageTypeRef> + '_ {
        self.tags
            .get(tag)
            .into_iter()
            .flat_map(|v| v.iter().filter_map(|ident| self.by_key(ident)))
    }

    /// Returns an iterator over all tag keys.
    pub fn tag_keys(&self) -> impl Iterator<Item = &Identifier> {
        self.tags.keys()
    }
}

impl RegistryExt for DamageTypeRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for DamageTypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}
