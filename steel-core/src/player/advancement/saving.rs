//! Vanilla `PlayerAdvancements` JSON:
//! `{"<advancement>": {"criteria": {"<name>": "<obtained>"}, "done": bool}, "DataVersion": N}`.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;
use std::time::SystemTime;

use chrono::{DateTime, Local};
use rustc_hash::FxHashMap;
use serde::de::{self, IgnoredAny, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use steel_utils::Identifier;
use steel_utils::version::DATA_VERSION;

use crate::player::advancement::progress::{AdvancementProgress, CriterionProgress};

const DATA_VERSION_KEY: &str = "DataVersion";
/// Vanilla `AdvancementProgress.OBTAINED_TIME_FORMAT` (`yyyy-MM-dd HH:mm:ss Z`).
const OBTAINED_TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S %z";

/// A player's persisted advancement progress, in file order.
pub(crate) struct PersistedAdvancements {
    pub(crate) entries: Vec<(Identifier, AdvancementProgress)>,
}

/// Borrowed variant used to serialize a save without cloning the entries.
pub(crate) struct PersistedAdvancementsRef<'a> {
    pub(crate) entries: &'a [(Identifier, AdvancementProgress)],
}

impl Serialize for PersistedAdvancementsRef<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len() + 1))?;
        for (advancement, progress) in self.entries {
            map.serialize_entry(advancement, progress)?;
        }
        map.serialize_entry(DATA_VERSION_KEY, &DATA_VERSION)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for PersistedAdvancements {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EntriesVisitor(PhantomData<PersistedAdvancements>);

        impl<'de> Visitor<'de> for EntriesVisitor {
            type Value = PersistedAdvancements;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a map of advancement progress")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::new();
                while let Some(key) = access.next_key::<Cow<'de, str>>()? {
                    // TODO: Run vanilla's advancement data fixers for older `DataVersion`s.
                    if key == DATA_VERSION_KEY {
                        access.next_value::<IgnoredAny>()?;
                        continue;
                    }
                    let advancement = key.parse::<Identifier>().map_err(de::Error::custom)?;
                    entries.push((advancement, access.next_value()?));
                }
                Ok(PersistedAdvancements { entries })
            }
        }

        deserializer.deserialize_map(EntriesVisitor(PhantomData))
    }
}

/// Vanilla `AdvancementProgress.CODEC`: obtained criteria only, plus the derived `done`.
impl Serialize for AdvancementProgress {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let criteria: BTreeMap<&str, String> = self
            .criteria()
            .iter()
            .filter_map(|(name, criterion)| {
                criterion
                    .0
                    .map(|obtained| (&**name, format_obtained(obtained)))
            })
            .collect();

        let mut map = serializer.serialize_map(None)?;
        if !criteria.is_empty() {
            map.serialize_entry("criteria", &criteria)?;
        }
        map.serialize_entry("done", &self.is_done())?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for AdvancementProgress {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        /// `done` is derived from the criteria, so vanilla ignores it when loading.
        #[derive(Deserialize)]
        struct Stored {
            #[serde(default)]
            criteria: FxHashMap<Cow<'static, str>, String>,
        }

        let stored = Stored::deserialize(deserializer)?;
        let criteria = stored
            .criteria
            .into_iter()
            .map(|(name, obtained)| {
                parse_obtained(&obtained)
                    .map(|obtained| (name, CriterionProgress(Some(obtained))))
                    .map_err(de::Error::custom)
            })
            .collect::<Result<_, _>>()?;
        Ok(Self::new(criteria))
    }
}

/// Formats in the system time zone, as vanilla's `OBTAINED_TIME_CODEC` does.
fn format_obtained(obtained: SystemTime) -> String {
    DateTime::<Local>::from(obtained)
        .format(OBTAINED_TIME_FORMAT)
        .to_string()
}

fn parse_obtained(obtained: &str) -> Result<SystemTime, chrono::ParseError> {
    DateTime::parse_from_str(obtained, OBTAINED_TIME_FORMAT).map(SystemTime::from)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use serde_json::Value;

    use super::*;

    /// `2026-11-04 10:00:00 +0100` from [`VANILLA_FILE`], i.e. 09:00 UTC.
    fn vanilla_obtained() -> SystemTime {
        UNIX_EPOCH + Duration::from_hours(498_273)
    }

    const VANILLA_FILE: &str = r#"{
        "minecraft:story/root": {
            "criteria": { "crafting_table": "2026-11-04 10:00:00 +0100" },
            "done": true
        },
        "minecraft:recipes/misc/charcoal": { "done": false },
        "DataVersion": 5023
    }"#;

    #[test]
    fn loads_a_vanilla_advancements_file() {
        let file: PersistedAdvancements =
            serde_json::from_str(VANILLA_FILE).expect("vanilla file should parse");

        assert_eq!(file.entries.len(), 2);
        let (root, root_progress) = &file.entries[0];
        assert_eq!(root.to_string(), "minecraft:story/root");
        let obtained = root_progress.criteria()["crafting_table"].0;
        assert_eq!(obtained, Some(vanilla_obtained()));
        assert!(file.entries[1].1.criteria().is_empty());
    }

    #[test]
    fn saves_in_the_vanilla_shape() {
        let obtained = vanilla_obtained();
        let mut criteria = FxHashMap::default();
        criteria.insert(
            Cow::Borrowed("crafting_table"),
            CriterionProgress(Some(obtained)),
        );
        criteria.insert(Cow::Borrowed("never"), CriterionProgress(None));
        let entries = [(
            Identifier::vanilla_static("story/root"),
            AdvancementProgress::new(criteria),
        )];

        let json = serde_json::to_value(PersistedAdvancementsRef { entries: &entries })
            .expect("save should serialize");

        assert_eq!(json[DATA_VERSION_KEY], Value::from(DATA_VERSION));
        let root = &json["minecraft:story/root"];
        assert!(root["done"].is_boolean());
        let saved_criteria = root["criteria"].as_object().expect("criteria object");
        assert_eq!(saved_criteria.len(), 1, "only obtained criteria are saved");
        let saved = saved_criteria["crafting_table"]
            .as_str()
            .expect("time string");
        assert_eq!(parse_obtained(saved).ok(), Some(obtained));
    }
}
