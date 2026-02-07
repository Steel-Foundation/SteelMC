use crate::RegistryExt;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use steel_utils::Identifier;

/// Categories for game rules, used for organization in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameRuleCategory {
    Chat,
    Drops,
    Misc,
    Mobs,
    Player,
    Spawning,
    Updates,
}

/// The type of a game rule value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameRuleType {
    Bool,
    Int,
}

/// A game rule value - either a boolean or an integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GameRuleValue {
    Bool(bool),
    Int(i32),
}

impl GameRuleValue {
    /// Returns the boolean value if this is a Bool variant.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            Self::Int(_) => None,
        }
    }

    /// Returns the integer value if this is an Int variant.
    #[must_use]
    pub fn as_int(&self) -> Option<i32> {
        match self {
            Self::Bool(_) => None,
            Self::Int(i) => Some(*i),
        }
    }

    /// Returns true if this is a Bool variant.
    #[must_use]
    pub fn is_bool(&self) -> bool {
        matches!(self, Self::Bool(_))
    }

    /// Returns true if this is an Int variant.
    #[must_use]
    pub fn is_int(&self) -> bool {
        matches!(self, Self::Int(_))
    }

    /// Returns true if this value matches the given type.
    #[must_use]
    pub fn matches_type(&self, value_type: GameRuleType) -> bool {
        matches!(
            (self, value_type),
            (Self::Bool(_), GameRuleType::Bool) | (Self::Int(_), GameRuleType::Int)
        )
    }
}

impl std::fmt::Display for GameRuleValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(i) => write!(f, "{i}"),
        }
    }
}

/// A game rule definition.
#[derive(Debug)]
pub struct GameRule {
    /// The key/name of the game rule (e.g., "keep_inventory").
    pub key: Identifier,
    /// The category this game rule belongs to.
    pub category: GameRuleCategory,
    /// The type of this game rule (bool or int).
    pub value_type: GameRuleType,
    /// The default value of this game rule.
    pub default_value: GameRuleValue,
    /// Minimum value for integer game rules (None means no limit).
    pub min_value: Option<i32>,
    /// Maximum value for integer game rules (None means no limit).
    pub max_value: Option<i32>,
}

pub type GameRuleRef = &'static GameRule;

pub struct GameRuleRegistry {
    game_rules_by_id: Vec<GameRuleRef>,
    game_rules_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl GameRuleRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            game_rules_by_id: Vec::new(),
            game_rules_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, game_rule: GameRuleRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register game rules after the registry has been frozen"
        );

        let id = self.game_rules_by_id.len();
        self.game_rules_by_key.insert(game_rule.key.clone(), id);
        self.game_rules_by_id.push(game_rule);
        id
    }

    /// Replaces a gamerule at a given index.
    /// Returns true if the gamerule was replaced and false if the gamerule wasn't replaced
    #[must_use]
    pub fn replace(&mut self, gamerule: GameRuleRef, id: usize) -> bool {
        if id >= self.game_rules_by_id.len() {
            return false;
        }
        self.game_rules_by_id[id] = gamerule;
        true
    }

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<GameRuleRef> {
        self.game_rules_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, game_rule: GameRuleRef) -> &usize {
        self.game_rules_by_key
            .get(&game_rule.key)
            .expect("Game rule not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<GameRuleRef> {
        self.game_rules_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, GameRuleRef)> + '_ {
        self.game_rules_by_id
            .iter()
            .enumerate()
            .map(|(id, &gr)| (id, gr))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.game_rules_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.game_rules_by_id.is_empty()
    }

    /// Gets the ID of a game rule by its key.
    #[must_use]
    pub fn get_id_by_key(&self, key: &Identifier) -> Option<usize> {
        self.game_rules_by_key.get(key).copied()
    }
}

impl RegistryExt for GameRuleRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for GameRuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Stores per-world game rule values.
///
/// This is separate from the registry - the registry holds static definitions,
/// while `GameRuleValues` holds the actual mutable values for a specific world.
#[derive(Debug, Clone, Default)]
pub struct GameRuleValues {
    /// Values indexed by game rule registry ID.
    values: Vec<GameRuleValue>,
}

impl GameRuleValues {
    /// Creates a new `GameRuleValues` with all default values from the registry.
    #[must_use]
    pub fn new(registry: &GameRuleRegistry) -> Self {
        let values = registry
            .iter()
            .map(|(_, rule)| rule.default_value)
            .collect();
        Self { values }
    }

    /// Gets the value of a game rule.
    #[must_use]
    pub fn get(&self, rule: GameRuleRef, registry: &GameRuleRegistry) -> GameRuleValue {
        let id = *registry.get_id(rule);
        self.values[id]
    }

    /// Sets the value of a game rule.
    ///
    /// Returns `true` if the value was set, `false` if the type didn't match
    /// or the value is out of bounds.
    pub fn set(
        &mut self,
        rule: GameRuleRef,
        value: GameRuleValue,
        registry: &GameRuleRegistry,
    ) -> bool {
        if !value.matches_type(rule.value_type) {
            return false;
        }
        // Check bounds for integer values
        if let GameRuleValue::Int(v) = value {
            if let Some(min) = rule.min_value
                && v < min
            {
                return false;
            }
            if let Some(max) = rule.max_value
                && v > max
            {
                return false;
            }
        }
        let id = *registry.get_id(rule);
        self.values[id] = value;
        true
    }

    /// Gets a game rule value by name.
    #[must_use]
    pub fn get_by_name(&self, name: &str, registry: &GameRuleRegistry) -> Option<GameRuleValue> {
        let key = Identifier::vanilla(name.to_string());
        let id = registry.get_id_by_key(&key)?;
        self.values.get(id).copied()
    }

    /// Sets a game rule value by name.
    ///
    /// Returns `true` if the game rule was found and set, `false` if the rule
    /// doesn't exist, the value type doesn't match, or the value is out of bounds.
    pub fn set_by_name(
        &mut self,
        name: &str,
        value: GameRuleValue,
        registry: &GameRuleRegistry,
    ) -> bool {
        let key = Identifier::vanilla(name.to_string());
        if let Some(rule) = registry.by_key(&key) {
            self.set(rule, value, registry)
        } else {
            false
        }
    }
}
