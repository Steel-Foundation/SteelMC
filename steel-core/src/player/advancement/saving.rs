use crate::player::advancement::progress::{AdvancementProgress, CriterionProgress};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use steel_utils::Identifier;
use steel_utils::serial::vec_as_map;

/// JSON representation of a player's persisted advancement progress.
#[derive(Deserialize)]
pub(crate) struct PersistedAdvancements {
    /// Progress keyed by advancement identifier.
    #[serde(with = "vec_as_map")]
    pub(crate) entries: Vec<(Identifier, AdvancementProgress)>,
}

/// Borrowed variant used to serialize a save without cloning the entries.
#[derive(Serialize)]
pub(crate) struct PersistedAdvancementsRef<'a> {
    /// Progress keyed by advancement identifier.
    #[serde(with = "vec_as_map")]
    pub(crate) entries: &'a [(Identifier, AdvancementProgress)],
}

impl Serialize for AdvancementProgress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let done = self
            .criteria()
            .iter()
            .filter(|(_key, criterion)| criterion.is_done());
        serializer.collect_map(done)
    }
}

impl<'de> Deserialize<'de> for AdvancementProgress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let criteria =
            FxHashMap::<Cow<'static, str>, CriterionProgress>::deserialize(deserializer)?;
        Ok(Self::new(criteria))
    }
}
