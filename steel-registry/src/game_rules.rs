use std::{
    fmt::{self, Debug, Display, Formatter},
    ptr::from_ref,
};

use rustc_hash::FxHashMap;
use serde_json::Value;
use steel_utils::{Downcast as _, DowncastType, DowncastTypeKey, ErasedType, Identifier};

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

/// Behavior required from a typed game rule value.
pub trait GameRuleValueType: DowncastType + Clone + Debug + Display + Send + Sync {}

impl<T> GameRuleValueType for T where T: DowncastType + Clone + Debug + Display + Send + Sync {}

trait ErasedGameRuleValue: ErasedType + Debug + Display + Send + Sync {
    fn clone_value(&self) -> Box<dyn ErasedGameRuleValue>;
}

impl<T: GameRuleValueType> ErasedGameRuleValue for T {
    fn clone_value(&self) -> Box<dyn ErasedGameRuleValue> {
        Box::new(self.clone())
    }
}

/// A keyed type-erased game rule value.
pub struct GameRuleValue {
    value: Box<dyn ErasedGameRuleValue>,
}

impl GameRuleValue {
    #[must_use]
    pub fn new(value: impl GameRuleValueType) -> Self {
        Self {
            value: Box::new(value),
        }
    }

    #[must_use]
    pub fn downcast_ref<T: DowncastType>(&self) -> Option<&T> {
        self.value.downcast_ref::<T>()
    }

    #[must_use]
    pub fn type_key(&self) -> DowncastTypeKey {
        self.value.downcast_type_key()
    }
}

impl Clone for GameRuleValue {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone_value(),
        }
    }
}

impl Debug for GameRuleValue {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GameRuleValue")
            .field("type_key", &self.type_key())
            .field("value", &self.value)
            .finish()
    }
}

impl Display for GameRuleValue {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(self.value.as_ref(), formatter)
    }
}

#[derive(Debug)]
struct GameRuleCodec<T: GameRuleValueType> {
    serialize: fn(&T) -> Value,
    deserialize: fn(&Value) -> Option<T>,
    command_result: fn(&T) -> i32,
    validates: fn(&GameRule<T>, &T) -> bool,
}

/// A typed game rule definition.
#[derive(Debug)]
pub struct GameRule<T: GameRuleValueType> {
    key: Identifier,
    category: GameRuleCategory,
    value_type: GameRuleType,
    default_value: T,
    min_value: Option<i32>,
    max_value: Option<i32>,
    codec: GameRuleCodec<T>,
}

impl<T: GameRuleValueType> GameRule<T> {
    #[must_use]
    pub const fn key(&self) -> &Identifier {
        &self.key
    }

    #[must_use]
    pub const fn category(&self) -> GameRuleCategory {
        self.category
    }

    #[must_use]
    pub const fn value_type(&self) -> GameRuleType {
        self.value_type
    }

    #[must_use]
    pub const fn default_value(&self) -> &T {
        &self.default_value
    }

    #[must_use]
    pub const fn min_value(&self) -> Option<i32> {
        self.min_value
    }

    #[must_use]
    pub const fn max_value(&self) -> Option<i32> {
        self.max_value
    }

    fn validates(&self, value: &T) -> bool {
        (self.codec.validates)(self, value)
    }
}

impl GameRule<bool> {
    #[must_use]
    pub const fn boolean(key: Identifier, category: GameRuleCategory, default_value: bool) -> Self {
        Self {
            key,
            category,
            value_type: GameRuleType::Bool,
            default_value,
            min_value: None,
            max_value: None,
            codec: GameRuleCodec {
                serialize: |value| Value::Bool(*value),
                deserialize: Value::as_bool,
                command_result: |value| i32::from(*value),
                validates: |_, _| true,
            },
        }
    }
}

impl GameRule<i32> {
    #[must_use]
    pub const fn integer(
        key: Identifier,
        category: GameRuleCategory,
        default_value: i32,
        min_value: Option<i32>,
        max_value: Option<i32>,
    ) -> Self {
        if let Some(min) = min_value {
            assert!(
                default_value >= min,
                "game rule default is below its minimum"
            );
        }
        if let Some(max) = max_value {
            assert!(
                default_value <= max,
                "game rule default is above its maximum"
            );
        }
        if let (Some(min), Some(max)) = (min_value, max_value) {
            assert!(min <= max, "game rule minimum is above its maximum");
        }
        Self {
            key,
            category,
            value_type: GameRuleType::Int,
            default_value,
            min_value,
            max_value,
            codec: GameRuleCodec {
                serialize: |value| Value::from(*value),
                deserialize: |value| {
                    let value = value.as_i64()?;
                    i32::try_from(value).ok()
                },
                command_result: |value| *value,
                validates: |rule, value| {
                    rule.min_value.is_none_or(|min| *value >= min)
                        && rule.max_value.is_none_or(|max| *value <= max)
                },
            },
        }
    }
}

impl<T: GameRuleValueType> PartialEq for GameRule<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<T: GameRuleValueType> Eq for GameRule<T> {}

/// Object-safe access to game rule metadata and value codecs.
pub trait ErasedGameRule: Debug + Send + Sync {
    fn key(&self) -> &Identifier;
    fn category(&self) -> GameRuleCategory;
    fn value_type(&self) -> GameRuleType;
    fn min_value(&self) -> Option<i32>;
    fn max_value(&self) -> Option<i32>;
    fn default_erased_value(&self) -> GameRuleValue;
    fn validates_erased_value(&self, value: &GameRuleValue) -> bool;
    fn serialize_erased_value(&self, value: &GameRuleValue) -> Value;
    fn deserialize_erased_value(&self, value: &Value) -> Option<GameRuleValue>;
    fn erased_command_result(&self, value: &GameRuleValue) -> i32;
}

impl<T: GameRuleValueType> ErasedGameRule for GameRule<T> {
    fn key(&self) -> &Identifier {
        self.key()
    }

    fn category(&self) -> GameRuleCategory {
        self.category()
    }

    fn value_type(&self) -> GameRuleType {
        self.value_type()
    }

    fn min_value(&self) -> Option<i32> {
        self.min_value()
    }

    fn max_value(&self) -> Option<i32> {
        self.max_value()
    }

    fn default_erased_value(&self) -> GameRuleValue {
        GameRuleValue::new(self.default_value.clone())
    }

    fn validates_erased_value(&self, value: &GameRuleValue) -> bool {
        value
            .downcast_ref::<T>()
            .is_some_and(|value| self.validates(value))
    }

    fn serialize_erased_value(&self, value: &GameRuleValue) -> Value {
        let Some(value) = value.downcast_ref::<T>() else {
            panic!("stored value type does not match game rule {}", self.key);
        };
        (self.codec.serialize)(value)
    }

    fn deserialize_erased_value(&self, value: &Value) -> Option<GameRuleValue> {
        (self.codec.deserialize)(value).map(GameRuleValue::new)
    }

    fn erased_command_result(&self, value: &GameRuleValue) -> i32 {
        let Some(value) = value.downcast_ref::<T>() else {
            panic!("stored value type does not match game rule {}", self.key);
        };
        (self.codec.command_result)(value)
    }
}

pub type GameRuleRef<T> = &'static GameRule<T>;
pub type ErasedGameRuleRef = &'static dyn ErasedGameRule;

pub struct GameRuleRegistry {
    game_rules_by_id: Vec<ErasedGameRuleRef>,
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

    pub fn register<T: GameRuleValueType>(&mut self, rule: GameRuleRef<T>) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register game rules after registry has been frozen"
        );
        assert!(
            !self.game_rules_by_key.contains_key(rule.key()),
            "Cannot register duplicate game rule {}",
            rule.key()
        );

        let id = self.game_rules_by_id.len();
        self.game_rules_by_key.insert(rule.key().clone(), id);
        self.game_rules_by_id.push(rule);
        id
    }

    pub const fn freeze(&mut self) {
        self.allows_registering = false;
    }

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<ErasedGameRuleRef> {
        self.game_rules_by_id.get(id).copied()
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<ErasedGameRuleRef> {
        self.id_from_key(key).and_then(|id| self.by_id(id))
    }

    #[must_use]
    pub fn id_from_key(&self, key: &Identifier) -> Option<usize> {
        self.game_rules_by_key.get(key).copied()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.game_rules_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.game_rules_by_id.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, ErasedGameRuleRef)> + '_ {
        self.game_rules_by_id.iter().copied().enumerate()
    }
}

impl Default for GameRuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Stores per-world game rule values.
#[derive(Debug, Clone, Default)]
pub struct GameRuleValues {
    values: Vec<GameRuleValue>,
}

impl GameRuleValues {
    /// Creates values initialized from every registered game rule's default.
    #[must_use]
    pub fn new(registry: &GameRuleRegistry) -> Self {
        let values = registry
            .iter()
            .map(|(_, rule)| rule.default_erased_value())
            .collect();
        Self { values }
    }

    /// Gets the typed value of a game rule.
    #[must_use]
    pub fn get<T: GameRuleValueType>(&self, rule: &GameRule<T>, registry: &GameRuleRegistry) -> T {
        let value = self.get_erased(rule, registry);
        let Some(value) = value.downcast_ref::<T>() else {
            panic!("stored value type does not match game rule {}", rule.key());
        };
        value.clone()
    }

    /// Sets the typed value of a game rule.
    pub fn set<T: GameRuleValueType>(
        &mut self,
        rule: &GameRule<T>,
        value: T,
        registry: &GameRuleRegistry,
    ) -> bool {
        if !rule.validates(&value) {
            return false;
        }
        let id = Self::rule_id(rule, registry);
        self.values[id] = GameRuleValue::new(value);
        true
    }

    /// Gets a type-erased value for a dynamically selected rule.
    #[must_use]
    pub fn get_erased(
        &self,
        rule: &dyn ErasedGameRule,
        registry: &GameRuleRegistry,
    ) -> &GameRuleValue {
        let id = Self::rule_id(rule, registry);
        let Some(value) = self.values.get(id) else {
            panic!("game rule {} has no stored value", rule.key());
        };
        value
    }

    /// Sets a type-erased value for a dynamically selected rule.
    pub fn set_erased(
        &mut self,
        rule: &dyn ErasedGameRule,
        value: GameRuleValue,
        registry: &GameRuleRegistry,
    ) -> bool {
        if !rule.validates_erased_value(&value) {
            return false;
        }
        let id = Self::rule_id(rule, registry);
        self.values[id] = value;
        true
    }

    /// Loads a serialized value for a game rule selected by name.
    pub fn set_serialized_by_name(
        &mut self,
        name: &str,
        value: &Value,
        registry: &GameRuleRegistry,
    ) -> bool {
        let Ok(key) = name.parse::<Identifier>() else {
            return false;
        };
        let Some(rule) = registry.by_key(&key) else {
            return false;
        };
        let Some(value) = rule.deserialize_erased_value(value) else {
            return false;
        };
        self.set_erased(rule, value, registry)
    }

    fn rule_id(rule: &dyn ErasedGameRule, registry: &GameRuleRegistry) -> usize {
        let Some(id) = registry.id_from_key(rule.key()) else {
            panic!("game rule {} is not registered", rule.key());
        };
        let Some(registered_rule) = registry.by_id(id) else {
            panic!("game rule registry has no entry for {}", rule.key());
        };
        assert!(
            from_ref(rule).cast::<()>() == from_ref(registered_rule).cast::<()>(),
            "game rule {} is not the registered definition",
            rule.key()
        );
        id
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{GameRule, GameRuleCategory, GameRuleRegistry, GameRuleValue, GameRuleValues};
    use steel_utils::Identifier;

    static BOOL_RULE: GameRule<bool> = GameRule::boolean(
        Identifier::vanilla_static("test_bool"),
        GameRuleCategory::Misc,
        true,
    );
    static INT_RULE: GameRule<i32> = GameRule::integer(
        Identifier::vanilla_static("test_int"),
        GameRuleCategory::Misc,
        3,
        Some(0),
        Some(10),
    );
    static CUSTOM_BOOL_RULE: GameRule<bool> = GameRule::boolean(
        Identifier::new_static("steel", "test_bool"),
        GameRuleCategory::Misc,
        true,
    );

    fn registry() -> GameRuleRegistry {
        let mut registry = GameRuleRegistry::new();
        registry.register(&BOOL_RULE);
        registry.register(&INT_RULE);
        registry.register(&CUSTOM_BOOL_RULE);
        registry.freeze();
        registry
    }

    #[test]
    fn typed_values_use_defaults_and_update_without_callsite_downcasts() {
        let registry = registry();
        let mut values = GameRuleValues::new(&registry);

        assert!(values.get(&BOOL_RULE, &registry));
        assert_eq!(values.get(&INT_RULE, &registry), 3);
        assert!(values.set(&BOOL_RULE, false, &registry));
        assert!(values.set(&INT_RULE, 7, &registry));
        assert!(!values.get(&BOOL_RULE, &registry));
        assert_eq!(values.get(&INT_RULE, &registry), 7);
    }

    #[test]
    fn typed_and_erased_sets_enforce_rule_bounds_and_value_type() {
        let registry = registry();
        let mut values = GameRuleValues::new(&registry);

        assert!(!values.set(&INT_RULE, 11, &registry));
        assert_eq!(values.get(&INT_RULE, &registry), 3);

        let Some(int_rule) = registry.by_key(INT_RULE.key()) else {
            panic!("test int rule should be registered");
        };
        assert!(!values.set_erased(int_rule, GameRuleValue::new(false), &registry));
        assert_eq!(values.get(&INT_RULE, &registry), 3);
    }

    #[test]
    fn erased_rule_codecs_round_trip_serialized_values() {
        let registry = registry();
        let mut values = GameRuleValues::new(&registry);

        assert!(values.set_serialized_by_name("test_bool", &Value::Bool(false), &registry));
        assert!(values.set_serialized_by_name("test_int", &Value::from(8), &registry));
        assert!(values.set_serialized_by_name("steel:test_bool", &Value::Bool(false), &registry,));
        assert!(!values.set_serialized_by_name("test_int", &Value::from(12), &registry));

        for (_, rule) in registry.iter() {
            let serialized = rule.serialize_erased_value(values.get_erased(rule, &registry));
            let Some(decoded) = rule.deserialize_erased_value(&serialized) else {
                panic!("serialized game rule should decode");
            };
            assert!(rule.validates_erased_value(&decoded));
        }

        assert!(!values.get(&BOOL_RULE, &registry));
        assert_eq!(values.get(&INT_RULE, &registry), 8);
        assert!(!values.get(&CUSTOM_BOOL_RULE, &registry));
    }

    #[test]
    #[should_panic(expected = "is not the registered definition")]
    fn same_key_does_not_allow_forging_different_rule_metadata() {
        let registry = registry();
        let values = GameRuleValues::new(&registry);
        let forged_rule = GameRule::integer(
            INT_RULE.key().clone(),
            GameRuleCategory::Misc,
            3,
            None,
            None,
        );

        let _ = values.get(&forged_rule, &registry);
    }
}
