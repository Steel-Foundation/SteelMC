//! represent the different struct related to the progress of an advancement
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::time::SystemTime;
use steel_registry::advancement::AdvancementRequirement;
use steel_registry::advancement::registry::AdvancementRef;

/// represent a map of all progress made for advancement of a specific player
#[derive(Debug, Default)]
pub struct AdvancementProgressMap {
    pub(crate) map: BTreeMap<AdvancementRef, AdvancementProgress>,
}

impl AdvancementProgressMap {
    /// Gets a mutable reference to the current progress for a given advancement. Creates the state entry if missing.
    pub fn get_mut_or_start_progress(
        &mut self,
        advancement: AdvancementRef,
    ) -> &mut AdvancementProgress {
        self.map.entry(advancement).or_insert_with(|| {
            let mut progress = AdvancementProgress::default();
            progress.update(&advancement.requirements);
            progress
        })
    }

    /// clear all the progress made
    #[inline]
    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// insert a new advancement to the progress map
    #[inline]
    pub fn insert(&mut self, advancement: AdvancementRef, progress: AdvancementProgress) {
        self.map.insert(advancement, progress);
    }

    /// return the len of the map
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// return `true` if empty
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// return whether the advancement has a progress instance
    #[must_use]
    #[inline]
    pub fn has_progress(&self, advancement: AdvancementRef) -> bool {
        self.map.contains_key(&advancement)
    }

    /// return the progress of a specific advancement
    #[must_use]
    #[inline]
    pub fn get_progress(&self, advancement: AdvancementRef) -> Option<&AdvancementProgress> {
        self.map.get(&advancement)
    }
}

/// Represents the progress of a given advancement for a player.
///
/// Tracks whether the advancement has been fully completed. In the future,
/// this will also track specific criteria progress.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdvancementProgress {
    /// Indicates the different progress of all criteria currently only a boolean
    pub criteria: FxHashMap<Cow<'static, str>, CriterionProgress>,
    /// The Requirement for the Advancement to be mark as complete
    pub requirements: AdvancementRequirement,
}

impl AdvancementProgress {
    /// Returns `true` if the advancement is done.
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.requirements.test(|s| self.is_criterion_done(s))
    }

    /// Check if a criterion his mark has complete
    fn is_criterion_done(&self, criterion: &str) -> bool {
        self.criteria
            .get(criterion)
            .is_some_and(CriterionProgress::is_done)
    }

    /// Returns `true` if the advancement has any progress. Currently just returns if it is fully complete.
    #[must_use]
    pub fn has_progress(&self) -> bool {
        for value in self.criteria.values() {
            if value.is_done() {
                return true;
            }
        }
        false
    }

    /// grant a specific criterion
    pub fn grant_progress(&mut self, name: &str) -> bool {
        if let Some(value) = self.criteria.get_mut(name)
            && !value.is_done()
        {
            value.grant();
            true
        } else {
            false
        }
    }

    /// revoke a specific criterion
    pub fn revoke_progress(&mut self, name: &str) -> bool {
        if let Some(value) = self.criteria.get_mut(name)
            && value.is_done()
        {
            value.revoke();
            true
        } else {
            false
        }
    }

    /// update the progress to match a new `AdvancementRequirement` list
    pub fn update(&mut self, requirements: &AdvancementRequirement) {
        let names = requirements.names();
        self.criteria.retain(|key, _criterion| names.contains(key));
        for name in names {
            self.criteria.entry(name).or_default();
        }
        self.requirements = requirements.clone();
    }

    /// return the remaining criteria
    #[inline]
    pub(crate) fn get_remaining_criteria(&self) -> impl Iterator<Item = &str> {
        self.criteria
            .iter()
            .filter(|&(_id, criterion)| !criterion.is_done())
            .map(|(id, _criterion)| &**id)
    }

    /// get the completed criteria
    #[inline]
    pub(crate) fn get_completed_criteria(&self) -> impl Iterator<Item = &str> {
        self.criteria
            .iter()
            .filter(|&(_id, criterion)| criterion.is_done())
            .map(|(id, _criterion)| &**id)
    }
}
/// represent a timestamp to when the criterion has been completed or None if it still un completed
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct CriterionProgress(pub Option<SystemTime>);

impl CriterionProgress {
    /// grant the progress
    pub fn grant(&mut self) {
        self.0 = Some(SystemTime::now());
    }

    /// revoke the progress
    pub const fn revoke(&mut self) {
        self.0 = None;
    }

    /// return whether the criterion has been completed
    #[must_use]
    pub const fn is_done(&self) -> bool {
        self.0.is_some()
    }
}
