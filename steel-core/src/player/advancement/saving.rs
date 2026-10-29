use crate::player::advancement::PlayerAdvancement;
use crate::player::advancement::progress::{
    AdvancementProgress, AdvancementProgressMap, CriterionProgress,
};
use rustc_hash::FxHashMap;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::fs;
use std::path::PathBuf;
use steel_registry::REGISTRY;
use steel_registry::advancement::registry::AdvancementRef;
use steel_registry::advancement::{Advancement, AdvancementRequirement};
use steel_utils::Identifier;
use steel_utils::serial::vec_as_map;

/// Errors that can occur when saving or loading advancement data.
#[derive(Debug, thiserror::Error)]
pub enum AdvancementDataError {
    #[error("IO error: {0}")]
    Io(std::io::Error),
    #[error("JSON error: {0}")]
    Json(serde_json::Error),
}

impl PlayerAdvancement {
    /// Saves the player's advancement progress to disk as JSON.
    pub async fn save(&self, path: &PathBuf) -> Result<(), AdvancementDataError> {
        let json = serde_json::to_string_pretty(&AdvancementProgressData::from(self.progress))
            .map_err(AdvancementDataError::Json)?;
        let Some(parent) = path.parent() else {
            return Ok(());
        };
        if let Err(e) = fs::create_dir_all(parent) {
            tracing::error!("Failed to create player advancement directory : {e}");
            return Err(AdvancementDataError::Io(e));
        }
        fs::write(path, json).map_err(AdvancementDataError::Io)?;
        Ok(())
    }

    /// Loads the player's advancement progress from disk.
    pub fn load(&mut self, path: &PathBuf) -> Result<(), AdvancementDataError> {
        if !path.exists() {
            return Ok(());
        }

        let json = fs::read(path).map_err(AdvancementDataError::Io)?;

        let loaded_data: AdvancementProgressData =
            serde_json::from_slice(&json).map_err(AdvancementDataError::Json)?;

        self.progress.clear();
        for (advancement_id, progress) in loaded_data.data {
            if let Some(advancement) = REGISTRY.advancements.value_by_key(&advancement_id) {
                self.progress.start_progress(advancement, progress);
                self.progress_changed.insert(advancement);
                self.mark_for_visibility_update(advancement);
            } else {
                tracing::warn!(
                    "Ignored advancement '{}' in progress file {} - it doesn't exist anymore?",
                    advancement_id,
                    path.display()
                );
            }
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct AdvancementProgressData {
    #[serde(with = "vec_as_map")]
    data: Vec<(Identifier, AdvancementProgress)>,
}

impl Serialize for AdvancementProgress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let map: FxHashMap<Cow<'static, str>, CriterionProgress> = self
            .criteria
            .iter()
            .filter(|(_key, criteria)| criteria.is_done())
            .collect();
        map.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AdvancementProgress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let criteria =
            FxHashMap::<Cow<'static, str>, CriterionProgress>::deserialize(deserializer)?;
        Ok(Self {
            criteria,
            requirements: AdvancementRequirement::default(),
        })
    }
}

impl From<AdvancementProgressMap> for AdvancementProgressData {
    fn from(progress: Self) -> Self {
        let data: Vec<(Identifier, AdvancementProgress)> = progress
            .data
            .into_iter()
            .filter(|(_advancement, progress)| progress.has_progress())
            .collect();
        Self { data }
    }
}
