//! Domain-scoped command scoreboard state.
//!
//! This module owns the scoreboard data needed by selectors and command
//! execution: objective identity and mutability, score values and locks, and
//! team membership. Display slots and client presentation are outside this
//! command-system scope.

use std::{
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    io,
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use steel_utils::locks::{AsyncMutex, SyncRwLock};
use thiserror::Error;

use crate::{server::worlds::WorldMap, world::World};
use steel_utils::saved_data::names as saved_data_names;

/// Score holder name stored by the vanilla scoreboard.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScoreHolder {
    name: String,
}

impl ScoreHolder {
    /// Creates a score holder from its scoreboard name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    /// Returns the scoreboard name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Objective identity resolved from one domain scoreboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScoreboardObjective {
    name: String,
    read_only: bool,
}

impl ScoreboardObjective {
    /// Returns the objective name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns whether commands may change scores for this objective.
    #[must_use]
    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }
}

/// Team identity resolved from one domain scoreboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScoreboardTeam {
    name: String,
}

impl ScoreboardTeam {
    /// Returns the team name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Stored score fields used by command execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScoreboardScore {
    value: i32,
    #[serde(default = "default_score_locked")]
    locked: bool,
}

impl ScoreboardScore {
    const fn new(value: i32) -> Self {
        Self {
            value,
            locked: true,
        }
    }

    /// Returns the integer score value.
    #[must_use]
    pub const fn value(self) -> i32 {
        self.value
    }

    /// Returns whether `/trigger`-style writes are locked.
    #[must_use]
    pub const fn is_locked(self) -> bool {
        self.locked
    }
}

impl Default for ScoreboardScore {
    fn default() -> Self {
        Self::new(0)
    }
}

const fn default_score_locked() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ObjectiveState {
    read_only: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PersistentScoreboard {
    objectives: BTreeMap<String, ObjectiveState>,
    scores: BTreeMap<String, BTreeMap<String, ScoreboardScore>>,
    teams: BTreeSet<String>,
    holder_teams: BTreeMap<String, String>,
}

/// Invalid scoreboard operation or persisted state.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ScoreboardError {
    /// Objective names may not be empty.
    #[error("objective name cannot be empty")]
    EmptyObjectiveName,
    /// Team names may not be empty.
    #[error("team name cannot be empty")]
    EmptyTeamName,
    /// Score holder names may not be empty.
    #[error("score holder name cannot be empty")]
    EmptyScoreHolderName,
    /// An objective already exists.
    #[error("objective '{0}' already exists")]
    DuplicateObjective(String),
    /// A team already exists.
    #[error("team '{0}' already exists")]
    DuplicateTeam(String),
    /// The requested objective does not exist.
    #[error("objective '{0}' does not exist")]
    MissingObjective(String),
    /// The requested team does not exist.
    #[error("team '{0}' does not exist")]
    MissingTeam(String),
    /// The objective cannot be written by commands.
    #[error("objective '{0}' is read-only")]
    ReadOnlyObjective(String),
}

struct ScoreboardSaveSnapshot {
    revision: u64,
    state: PersistentScoreboard,
}

/// Command-facing scoreboard for one Steel domain.
pub struct Scoreboard {
    state: SyncRwLock<PersistentScoreboard>,
    revision: AtomicU64,
    saved_revision: AtomicU64,
}

impl Scoreboard {
    /// Creates an empty, clean scoreboard.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: SyncRwLock::new(PersistentScoreboard::default()),
            revision: AtomicU64::new(0),
            saved_revision: AtomicU64::new(0),
        }
    }

    fn from_persistent(state: PersistentScoreboard) -> Result<Self, ScoreboardError> {
        validate_persistent_scoreboard(&state)?;
        Ok(Self {
            state: SyncRwLock::new(state),
            revision: AtomicU64::new(0),
            saved_revision: AtomicU64::new(0),
        })
    }

    /// Adds a writable objective.
    ///
    /// # Errors
    ///
    /// Returns an error if the objective name is empty or already exists.
    pub fn add_objective(
        &self,
        name: impl Into<String>,
    ) -> Result<ScoreboardObjective, ScoreboardError> {
        self.add_objective_with_read_only(name, false)
    }

    /// Adds an objective with explicit command mutability.
    ///
    /// # Errors
    ///
    /// Returns an error if the objective name is empty or already exists.
    pub fn add_objective_with_read_only(
        &self,
        name: impl Into<String>,
        read_only: bool,
    ) -> Result<ScoreboardObjective, ScoreboardError> {
        let name = name.into();
        ensure_objective_name(&name)?;
        let mut state = self.state.write();
        if state.objectives.contains_key(&name) {
            return Err(ScoreboardError::DuplicateObjective(name));
        }
        state
            .objectives
            .insert(name.clone(), ObjectiveState { read_only });
        self.mark_dirty();
        Ok(ScoreboardObjective { name, read_only })
    }

    /// Returns an objective by name.
    #[must_use]
    pub fn objective(&self, name: &str) -> Option<ScoreboardObjective> {
        self.state
            .read()
            .objectives
            .get(name)
            .map(|objective| ScoreboardObjective {
                name: name.to_owned(),
                read_only: objective.read_only,
            })
    }

    /// Returns objective names in stable order.
    #[must_use]
    pub fn objective_names(&self) -> Vec<String> {
        self.state.read().objectives.keys().cloned().collect()
    }

    /// Adds a team.
    ///
    /// # Errors
    ///
    /// Returns an error if the team name is empty or already exists.
    pub fn add_team(&self, name: impl Into<String>) -> Result<ScoreboardTeam, ScoreboardError> {
        let name = name.into();
        ensure_team_name(&name)?;
        let mut state = self.state.write();
        if !state.teams.insert(name.clone()) {
            return Err(ScoreboardError::DuplicateTeam(name));
        }
        self.mark_dirty();
        Ok(ScoreboardTeam { name })
    }

    /// Returns a team by name.
    #[must_use]
    pub fn team(&self, name: &str) -> Option<ScoreboardTeam> {
        self.state
            .read()
            .teams
            .contains(name)
            .then(|| ScoreboardTeam {
                name: name.to_owned(),
            })
    }

    /// Returns team names in stable order.
    #[must_use]
    pub fn team_names(&self) -> Vec<String> {
        self.state.read().teams.iter().cloned().collect()
    }

    /// Returns the current team name for a score holder.
    #[must_use]
    pub fn holder_team_name(&self, holder: &ScoreHolder) -> Option<String> {
        self.state.read().holder_teams.get(holder.name()).cloned()
    }

    /// Adds a holder to a team, replacing any prior membership.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty holder or a team that no longer exists.
    pub fn add_holder_to_team(
        &self,
        holder: &ScoreHolder,
        team: &ScoreboardTeam,
    ) -> Result<(), ScoreboardError> {
        ensure_holder_name(holder.name())?;
        let mut state = self.state.write();
        if !state.teams.contains(team.name()) {
            return Err(ScoreboardError::MissingTeam(team.name().to_owned()));
        }
        if state
            .holder_teams
            .insert(holder.name().to_owned(), team.name().to_owned())
            .as_deref()
            == Some(team.name())
        {
            return Ok(());
        }
        self.mark_dirty();
        Ok(())
    }

    /// Returns tracked score holders in stable order.
    #[must_use]
    pub fn tracked_holders(&self) -> Vec<ScoreHolder> {
        self.state
            .read()
            .scores
            .keys()
            .map(|name| ScoreHolder::new(name.to_owned()))
            .collect()
    }

    /// Returns the complete score entry for a holder and objective.
    #[must_use]
    pub fn score_entry(
        &self,
        holder: &ScoreHolder,
        objective: &ScoreboardObjective,
    ) -> Option<ScoreboardScore> {
        self.state
            .read()
            .scores
            .get(holder.name())
            .and_then(|scores| scores.get(objective.name()).copied())
    }

    /// Returns the integer score for a holder and objective.
    #[must_use]
    pub fn score(&self, holder: &ScoreHolder, objective: &ScoreboardObjective) -> Option<i32> {
        self.score_entry(holder, objective)
            .map(ScoreboardScore::value)
    }

    /// Sets a holder's score, preserving its lock state when already present.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty holder, a missing objective, or a read-only objective.
    pub fn set_score(
        &self,
        holder: &ScoreHolder,
        objective: &ScoreboardObjective,
        value: i32,
    ) -> Result<(), ScoreboardError> {
        ensure_holder_name(holder.name())?;
        let mut state = self.state.write();
        ensure_writable_objective(&state, objective)?;
        let scores = state.scores.entry(holder.name().to_owned()).or_default();
        match scores.entry(objective.name().to_owned()) {
            Entry::Vacant(entry) => {
                entry.insert(ScoreboardScore::new(value));
            }
            Entry::Occupied(mut entry) => {
                if entry.get().value == value {
                    return Ok(());
                }
                entry.get_mut().value = value;
            }
        }
        self.mark_dirty();
        Ok(())
    }

    /// Changes a score's trigger lock state.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty holder or a missing objective.
    pub fn set_score_locked(
        &self,
        holder: &ScoreHolder,
        objective: &ScoreboardObjective,
        locked: bool,
    ) -> Result<(), ScoreboardError> {
        ensure_holder_name(holder.name())?;
        let mut state = self.state.write();
        ensure_objective_exists(&state, objective)?;
        let scores = state.scores.entry(holder.name().to_owned()).or_default();
        match scores.entry(objective.name().to_owned()) {
            Entry::Vacant(entry) => {
                entry.insert(ScoreboardScore { value: 0, locked });
            }
            Entry::Occupied(mut entry) => {
                if entry.get().locked == locked {
                    return Ok(());
                }
                entry.get_mut().locked = locked;
            }
        }
        self.mark_dirty();
        Ok(())
    }

    /// Returns objective names that have a score for the holder.
    #[must_use]
    pub fn holder_objectives(&self, holder: &ScoreHolder) -> BTreeSet<String> {
        self.state
            .read()
            .scores
            .get(holder.name())
            .map_or_else(BTreeSet::new, |scores| scores.keys().cloned().collect())
    }

    fn mark_dirty(&self) {
        self.revision.fetch_add(1, Ordering::Release);
    }

    fn pending_save(&self) -> Option<ScoreboardSaveSnapshot> {
        let state = self.state.read();
        let revision = self.revision.load(Ordering::Acquire);
        if revision == self.saved_revision.load(Ordering::Acquire) {
            return None;
        }
        Some(ScoreboardSaveSnapshot {
            revision,
            state: state.clone(),
        })
    }

    fn mark_saved(&self, revision: u64) {
        self.saved_revision.fetch_max(revision, Ordering::Release);
    }
}

impl Default for Scoreboard {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_persistent_scoreboard(state: &PersistentScoreboard) -> Result<(), ScoreboardError> {
    for name in state.objectives.keys() {
        ensure_objective_name(name)?;
    }
    for name in &state.teams {
        ensure_team_name(name)?;
    }
    for (holder, scores) in &state.scores {
        ensure_holder_name(holder)?;
        for objective in scores.keys() {
            if !state.objectives.contains_key(objective) {
                return Err(ScoreboardError::MissingObjective(objective.clone()));
            }
        }
    }
    for (holder, team) in &state.holder_teams {
        ensure_holder_name(holder)?;
        if !state.teams.contains(team) {
            return Err(ScoreboardError::MissingTeam(team.clone()));
        }
    }
    Ok(())
}

const fn ensure_objective_name(name: &str) -> Result<(), ScoreboardError> {
    if name.is_empty() {
        Err(ScoreboardError::EmptyObjectiveName)
    } else {
        Ok(())
    }
}

const fn ensure_team_name(name: &str) -> Result<(), ScoreboardError> {
    if name.is_empty() {
        Err(ScoreboardError::EmptyTeamName)
    } else {
        Ok(())
    }
}

const fn ensure_holder_name(name: &str) -> Result<(), ScoreboardError> {
    if name.is_empty() {
        Err(ScoreboardError::EmptyScoreHolderName)
    } else {
        Ok(())
    }
}

fn ensure_objective_exists(
    state: &PersistentScoreboard,
    objective: &ScoreboardObjective,
) -> Result<ObjectiveState, ScoreboardError> {
    state
        .objectives
        .get(objective.name())
        .copied()
        .ok_or_else(|| ScoreboardError::MissingObjective(objective.name().to_owned()))
}

fn ensure_writable_objective(
    state: &PersistentScoreboard,
    objective: &ScoreboardObjective,
) -> Result<(), ScoreboardError> {
    if ensure_objective_exists(state, objective)?.read_only {
        Err(ScoreboardError::ReadOnlyObjective(
            objective.name().to_owned(),
        ))
    } else {
        Ok(())
    }
}

/// Loaded command scoreboards keyed by Steel domain.
pub struct DomainScoreboards {
    scoreboards: BTreeMap<String, Scoreboard>,
    save_lock: AsyncMutex<()>,
}

impl DomainScoreboards {
    /// Loads one scoreboard through each domain's default world saved-data boundary.
    pub async fn load(worlds: &WorldMap) -> io::Result<Self> {
        let mut domains = worlds.domain_names().collect::<Vec<_>>();
        domains.sort_unstable();
        let mut scoreboards = BTreeMap::new();
        for domain in domains {
            let world = domain_default_world(worlds, domain)?;
            let persistent: PersistentScoreboard = world
                .saved_data
                .load_or_default(saved_data_names::SCOREBOARD)
                .await
                .map_err(|error| scoreboard_io_error(domain, error))?;
            let scoreboard = Scoreboard::from_persistent(persistent).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid scoreboard for domain '{domain}': {error}"),
                )
            })?;
            scoreboards.insert(domain.to_owned(), scoreboard);
        }
        Ok(Self {
            scoreboards,
            save_lock: AsyncMutex::new(()),
        })
    }

    /// Returns the scoreboard for a domain.
    #[must_use]
    pub fn get(&self, domain: &str) -> Option<&Scoreboard> {
        self.scoreboards.get(domain)
    }

    /// Saves every dirty domain scoreboard and returns the number written.
    pub async fn save(&self, worlds: &WorldMap) -> io::Result<usize> {
        let _save_guard = self.save_lock.lock().await;
        let mut saved = 0;
        for (domain, scoreboard) in &self.scoreboards {
            let Some(snapshot) = scoreboard.pending_save() else {
                continue;
            };
            let world = domain_default_world(worlds, domain)?;
            world
                .saved_data
                .save(saved_data_names::SCOREBOARD, &snapshot.state)
                .await
                .map_err(|error| scoreboard_io_error(domain, error))?;
            scoreboard.mark_saved(snapshot.revision);
            saved += 1;
        }
        Ok(saved)
    }
}

fn domain_default_world<'a>(worlds: &'a WorldMap, domain: &str) -> io::Result<&'a World> {
    worlds
        .default_world(domain)
        .map(AsRef::as_ref)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("domain '{domain}' has no loaded default world"),
            )
        })
}

fn scoreboard_io_error(domain: &str, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("scoreboard I/O failed for domain '{domain}': {error}"),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        env::temp_dir,
        time::{SystemTime, UNIX_EPOCH},
    };
    use tokio::fs;

    use steel_utils::saved_data::{SavedDataManager, names as saved_data_names};

    use super::{
        AsyncMutex, DomainScoreboards, PersistentScoreboard, ScoreHolder, Scoreboard,
        ScoreboardError,
    };

    #[test]
    fn score_value_and_lock_state_are_independent() {
        let scoreboard = Scoreboard::new();
        let objective = scoreboard
            .add_objective("kills")
            .expect("objective should be added");
        let holder = ScoreHolder::new("Steve");

        scoreboard
            .set_score(&holder, &objective, 7)
            .expect("score should be writable");
        scoreboard
            .set_score_locked(&holder, &objective, false)
            .expect("score lock should change");
        scoreboard
            .set_score(&holder, &objective, 9)
            .expect("score should remain writable");

        let entry = scoreboard
            .score_entry(&holder, &objective)
            .expect("score should exist");
        assert_eq!(entry.value(), 9);
        assert!(!entry.is_locked());
    }

    #[test]
    fn read_only_objective_rejects_score_writes() {
        let scoreboard = Scoreboard::new();
        let objective = scoreboard
            .add_objective_with_read_only("health", true)
            .expect("objective should be added");

        assert_eq!(
            scoreboard.set_score(&ScoreHolder::new("Steve"), &objective, 20),
            Err(ScoreboardError::ReadOnlyObjective("health".to_owned()))
        );
    }

    #[test]
    fn team_assignment_replaces_prior_membership() {
        let scoreboard = Scoreboard::new();
        let red = scoreboard
            .add_team("red")
            .expect("red team should be added");
        let blue = scoreboard
            .add_team("blue")
            .expect("blue team should be added");
        let holder = ScoreHolder::new("Steve");

        scoreboard
            .add_holder_to_team(&holder, &red)
            .expect("holder should join red");
        scoreboard
            .add_holder_to_team(&holder, &blue)
            .expect("holder should move to blue");

        assert_eq!(
            scoreboard.holder_team_name(&holder).as_deref(),
            Some("blue")
        );
    }

    #[tokio::test]
    async fn persisted_scoreboard_round_trips_and_becomes_clean_after_save() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after Unix epoch")
            .as_nanos();
        let path = temp_dir().join(format!("steel-scoreboard-{unique}"));
        let manager = SavedDataManager::new(Some(&path));
        let scoreboard = Scoreboard::new();
        let objective = scoreboard
            .add_objective("kills")
            .expect("objective should be added");
        let holder = ScoreHolder::new("Steve");
        scoreboard
            .set_score(&holder, &objective, 5)
            .expect("score should be writable");

        let snapshot = scoreboard
            .pending_save()
            .expect("scoreboard should be dirty");
        manager
            .save(saved_data_names::SCOREBOARD, &snapshot.state)
            .await
            .expect("scoreboard should save");
        scoreboard.mark_saved(snapshot.revision);
        assert!(scoreboard.pending_save().is_none());

        let persistent: PersistentScoreboard = manager
            .load_or_default(saved_data_names::SCOREBOARD)
            .await
            .expect("scoreboard should load");
        let restored = Scoreboard::from_persistent(persistent).expect("scoreboard should validate");
        let restored_objective = restored
            .objective("kills")
            .expect("objective should persist");
        assert_eq!(restored.score(&holder, &restored_objective), Some(5));

        fs::remove_dir_all(path)
            .await
            .expect("temporary scoreboard directory should be removed");
    }

    #[test]
    fn mutation_after_snapshot_remains_dirty_when_snapshot_is_marked_saved() {
        let scoreboard = Scoreboard::new();
        scoreboard
            .add_objective("kills")
            .expect("objective should be added");
        let snapshot = scoreboard
            .pending_save()
            .expect("scoreboard should be dirty");

        scoreboard
            .add_objective("deaths")
            .expect("second objective should be added");
        scoreboard.mark_saved(snapshot.revision);

        let pending = scoreboard
            .pending_save()
            .expect("newer mutation should remain dirty");
        assert!(pending.revision > snapshot.revision);
        assert!(pending.state.objectives.contains_key("deaths"));
    }

    #[test]
    fn domains_keep_independent_scoreboards() {
        let scoreboards = DomainScoreboards {
            scoreboards: [
                ("alpha".to_owned(), Scoreboard::new()),
                ("beta".to_owned(), Scoreboard::new()),
            ]
            .into_iter()
            .collect(),
            save_lock: AsyncMutex::new(()),
        };
        scoreboards
            .get("alpha")
            .expect("alpha scoreboard should exist")
            .add_objective("kills")
            .expect("alpha objective should be added");

        assert!(
            scoreboards
                .get("beta")
                .expect("beta scoreboard should exist")
                .objective("kills")
                .is_none()
        );
    }
}
