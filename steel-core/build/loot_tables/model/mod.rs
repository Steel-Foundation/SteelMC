//! Build-only input types. Executable loot remains generated Rust.

use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, Visitor},
};
use std::{collections::BTreeMap, fmt, marker::PhantomData};

mod conditions;
mod entries;
mod modifiers;
mod numbers;
mod predicates;

pub(super) use conditions::*;
pub(super) use entries::*;
pub(super) use modifiers::*;
pub(super) use numbers::*;
pub(super) use predicates::*;

/// A registry reference or an inline definition, retaining inline decoding errors.
pub enum Holder<T> {
    Reference(String),
    Inline(T),
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Holder<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct HolderVisitor<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for HolderVisitor<T> {
            type Value = Holder<T>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a registry reference or inline definition")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(Holder::Reference(value.to_owned()))
            }
            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
                T::deserialize(de::value::MapAccessDeserializer::new(map)).map(Holder::Inline)
            }
        }
        deserializer.deserialize_any(HolderVisitor(PhantomData))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LootTable {
    // TODO: Validate supplied loot context parameters against the table's param set.
    #[serde(rename = "type")]
    pub param_set: Option<String>,
    pub random_sequence: Option<String>,
    #[serde(default)]
    pub pools: Vec<LootPool>,
    pub modifier: Option<Modifier>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LootPool {
    pub rolls: ContextIntProvider,
    pub bonus_rolls: Option<ContextFloatProvider>,
    pub entries: Vec<LootPoolEntry>,
    pub condition: Option<Holder<Condition>>,
    pub modifier: Option<Modifier>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds<T> {
    pub min: Option<T>,
    pub max: Option<T>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum IntRange {
    Exact(i32),
    Bounds(Bounds<i32>),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum HolderSet {
    Single(String),
    List(Vec<String>),
}

pub type StateProperties = BTreeMap<String, String>;

pub const fn one() -> i32 {
    1
}
pub const fn yes() -> bool {
    true
}
