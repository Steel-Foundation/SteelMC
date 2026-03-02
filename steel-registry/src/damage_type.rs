use crate::RegistryExt;
use rustc_hash::FxHashMap;
use steel_utils::Identifier;

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

pub type DamageTypeRef = &'static DamageType;

pub struct DamageTypeRegistry {
    damage_types_by_id: Vec<DamageTypeRef>,
    damage_types_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl DamageTypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            damage_types_by_id: Vec::new(),
            damage_types_by_key: FxHashMap::default(),
            allows_registering: true,
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
