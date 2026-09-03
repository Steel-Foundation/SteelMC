//! Vanilla villager job-site acquisition, ported from Minecraft 26.2
//! `VillagerGoalPackages` core behaviors.
//!
//! Vanilla implements job-site claiming as brain behaviors over the `JOB_SITE` /
//! `POTENTIAL_JOB_SITE` memories (`GlobalPos`es). Steel villagers have no `Brain`, so the
//! memories live on [`VillagerEntity`](super::VillagerEntity) and the behaviors are
//! goal-selector goals:
//!
//! | Vanilla behavior                | Goal                        | Vanilla priority | Steel priority |
//! |---------------------------------|-----------------------------|------------------|----------------|
//! | `ValidateNearbyPoi(JOB_SITE)`   | `ValidateJobSiteGoal`       | 0                | 0              |
//! | `ValidateNearbyPoi(POTENTIAL)`  | `ValidateJobSiteGoal`       | 0                | 0              |
//! | `GoToPotentialJobSite`          | `GoToPotentialJobSiteGoal`  | 7                | 4¹             |
//! | `AcquirePoi`                    | `AcquireJobSiteGoal`        | 6                | 6              |
//! | `AssignProfessionFromJobSite`   | `AssignProfessionGoal`      | 10               | 10             |
//! | `ResetProfession`               | `ResetProfessionGoal`       | 10               | 10             |
//!
//! ¹ The vanilla numbers are brain priorities inside the core package, where the walk can
//! coexist with idle strolling. In the goal selector both goals claim `MOVE`, and a goal
//! only replaces a running goal with a *lower* priority number, so the walk target must sit
//! above the stroll (5) to be reachable, while panic (1) and avoid (2) still preempt it.
//! Empty-control goals keep their vanilla relative order; `ResetProfession` is registered
//! after `AssignProfession` so an erased `JOB_SITE` is evaluated after the assignment pass.
//!
//! Structure notes against vanilla:
//! - Vanilla memories carry a dimension (`GlobalPos`) because cross-dimension lookups must
//!   fail safely. Steel villagers are bound to a single world, so memories are plain
//!   [`BlockPos`]es and the vanilla same-dimension checks are structurally always true.
//! - Vanilla `GoToPotentialJobSite.checkExtraStartConditions` only walks during the
//!   IDLE/WORK/PLAY activities; Steel has no activity scheduler, so the walk runs whenever
//!   the memory is present (the only activities in Steel are the vanilla core package).
//! - Vanilla `PoiCompetitorScan` and `YieldJobSite` (neighbor arbitration and hand-off) are
//!   not implemented: Steel's atomic `reserve_ticket` prevents two villagers from claiming
//!   the same job site, and `PoiCompetitorScan` additionally depends on a nearest-entities
//!   sensor to break XP ties. Revisit once that sensor exists.
//! - Vanilla `WorkAtPoi` (work schedule, work sounds, trade restock) and the sleep/meet
//!   schedules are separate systems, not part of job acquisition.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use steel_registry::poi::PoiTypeRef;
use steel_registry::villager_profession::VillagerProfessionRef;
use steel_registry::{REGISTRY, RegistryEntry, RegistryExt, vanilla_villager_professions};
use steel_utils::{BlockPos, Downcast};

use steel_utils::entity_events::EntityStatus;

use super::VillagerEntity;
use crate::entity::ai::goal::{Goal, GoalControls, GoalSelector};
use crate::entity::{Entity, LivingEntity, PathfinderMob};
use crate::poi::OccupationStatus;
use crate::world::World;

/// Vanilla `AcquirePoi.SCAN_RANGE` — horizontal scan radius for candidate job sites.
const SCAN_RANGE: i32 = 48;
/// Vanilla `AcquirePoi` batch size — closest candidates considered per scan.
const SCAN_BATCH_SIZE: usize = 5;
/// Vanilla `AcquirePoi` rate — scans run every `SCAN_INTERVAL_TICKS + rand(0..20)` ticks.
const SCAN_INTERVAL_TICKS: i64 = 20;
/// Vanilla `JitteredLinearRetry` bounds for unreachable candidates.
const RETRY_MIN_INTERVAL_INCREASE: i32 = 40;
const RETRY_MAX_INTERVAL_INCREASE: i32 = 80;
const RETRY_MAX_DELAY: i32 = 400;
/// Vanilla `JitteredLinearRetry` validity — markers expire after this many ticks.
const RETRY_VALIDITY_TICKS: i64 = 400;
/// Vanilla `ValidateNearbyPoi.MAX_DISTANCE`.
const VALIDATE_MAX_DISTANCE: f64 = 16.0;
/// Vanilla `AssignProfessionFromJobSite` assignment distance.
const ASSIGN_MAX_DISTANCE: f64 = 2.0;
/// Vanilla `GoToPotentialJobSite.TICKS_UNTIL_TIMEOUT`.
const GOTO_POTENTIAL_TIMEOUT_TICKS: i64 = 1200;
/// Vanilla `GoToPotentialJobSite` walk speed modifier.
const GOTO_POTENTIAL_SPEED: f64 = 0.5;

/// The two job-site brain memories a villager tracks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct JobSiteMemories {
    /// Vanilla `JOB_SITE` — the POI this villager is employed at (ticket held).
    pub job_site: Option<BlockPos>,
    /// Vanilla `POTENTIAL_JOB_SITE` — the POI this villager claimed and is walking to.
    pub potential_job_site: Option<BlockPos>,
}

/// Which of the two job-site memories a [`ValidateJobSiteGoal`] validates.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum JobSiteMemory {
    JobSite,
    PotentialJobSite,
}

/// Registers the vanilla core-package job-site goals on a villager's goal selector.
///
/// See the module docs for the priority mapping against vanilla brain priorities.
pub(super) fn register_job_site_goals(goal_selector: &mut GoalSelector) {
    goal_selector.add_goal(0, ValidateJobSiteGoal::new(JobSiteMemory::JobSite));
    goal_selector.add_goal(0, ValidateJobSiteGoal::new(JobSiteMemory::PotentialJobSite));
    goal_selector.add_goal(4, GoToPotentialJobSiteGoal::default());
    goal_selector.add_goal(6, AcquireJobSiteGoal::default());
    goal_selector.add_goal(10, AssignProfessionGoal);
    goal_selector.add_goal(10, ResetProfessionGoal);
}

/// Returns the villager's current profession reference.
fn profession_of(villager: &VillagerEntity) -> Option<VillagerProfessionRef> {
    let id = villager.profession_id();
    let id = usize::try_from(id).ok()?;
    REGISTRY.villager_professions.by_id(id)
}

/// Resolves the POI type registered at `pos` in the villager's world.
fn poi_type_at(world: &World, pos: BlockPos) -> Option<PoiTypeRef> {
    let storage = world.poi_storage.lock();
    let type_id = storage.get_type(pos)?;
    REGISTRY.poi_types.by_id(type_id)
}

/// Vanilla `Vec3.closerToCenterThan` — Euclidean distance to the block center.
fn closer_to_center_than(pos: BlockPos, other: glam::DVec3, distance: f64) -> bool {
    let center = glam::DVec3::new(
        f64::from(pos.x()) + 0.5,
        f64::from(pos.y()) + 0.5,
        f64::from(pos.z()) + 0.5,
    );
    other.distance_squared(center) < distance * distance
}

impl VillagerEntity {
    /// Returns the current job-site memories (vanilla `JOB_SITE` / `POTENTIAL_JOB_SITE`).
    pub(crate) fn job_site_memories(&self) -> JobSiteMemories {
        *self.job_site_memories.lock()
    }

    pub(crate) fn profession_id(&self) -> i32 {
        self.entity_data.lock().villager_data.get().profession
    }

    /// Vanilla `Villager.setVillagerData`: changing the profession discards the trade
    /// offers, which the next trade rebuilds. Steel rebuilds eagerly.
    pub(crate) fn set_profession(&self, profession: VillagerProfessionRef) {
        let Some(id) = profession.try_id() else {
            return;
        };
        let changed = {
            let mut data = self.entity_data.lock();
            let mut villager_data = *data.villager_data.get();
            let changed = villager_data.profession != i32::try_from(id).unwrap_or(0);
            if changed {
                villager_data.profession = i32::try_from(id).unwrap_or(0);
                data.villager_data.set(villager_data);
            }
            changed
        };
        if changed {
            self.offers.lock().clear();
            self.update_trades();
        }
    }

    /// Releases the `JOB_SITE` / `POTENTIAL_JOB_SITE` POI tickets when the stored position
    /// still holds a matching POI type (vanilla `Villager.releasePoi` for these memories).
    pub(crate) fn release_job_site_pois(&self) {
        let Some(world) = self.level() else {
            return;
        };
        let memories = self.job_site_memories();

        let held_job_site =
            profession_of(self).map_or(&[] as &[PoiTypeRef], |profession| profession.held_job_site);
        if let Some(pos) = memories.job_site
            && poi_type_at(&world, pos).is_some_and(|poi_type| held_job_site.contains(&poi_type))
        {
            let _ = world.poi_storage.lock().release_ticket(pos);
        }

        // Vanilla's POTENTIAL predicate is `VillagerProfession.ALL_ACQUIRABLE_JOBS`, which
        // is exactly the extracted acquirable set of `minecraft:none`.
        if let Some(pos) = memories.potential_job_site
            && poi_type_at(&world, pos).is_some_and(|poi_type| {
                vanilla_villager_professions::NONE
                    .acquirable_job_site
                    .contains(&poi_type)
            })
        {
            let _ = world.poi_storage.lock().release_ticket(pos);
        }
    }
}

/// Vanilla `ValidateNearbyPoi`: erases a job-site memory when the POI at the stored
/// position no longer matches the expected type.
///
/// Vanilla triggers this whenever the memory is present and the villager is within range;
/// performing the validation in `can_use` reproduces that cadence (the goal never runs, so
/// the empty controls keep it from competing with anything).
pub struct ValidateJobSiteGoal {
    memory: JobSiteMemory,
}

impl ValidateJobSiteGoal {
    pub(super) const fn new(memory: JobSiteMemory) -> Self {
        Self { memory }
    }

    fn memory_position(&self, villager: &VillagerEntity) -> Option<BlockPos> {
        let memories = villager.job_site_memories();
        match self.memory {
            JobSiteMemory::JobSite => memories.job_site,
            JobSiteMemory::PotentialJobSite => memories.potential_job_site,
        }
    }

    fn erase_memory(&self, villager: &VillagerEntity) {
        let mut memories = villager.job_site_memories.lock();
        match self.memory {
            JobSiteMemory::JobSite => memories.job_site = None,
            JobSiteMemory::PotentialJobSite => memories.potential_job_site = None,
        }
    }
}

impl Goal for ValidateJobSiteGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::EMPTY
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(villager) = mob.downcast_ref::<VillagerEntity>() else {
            return false;
        };
        let Some(pos) = self.memory_position(villager) else {
            return false;
        };
        let Some(world) = mob.level() else {
            return false;
        };
        if !closer_to_center_than(pos, mob.position(), VALIDATE_MAX_DISTANCE) {
            // Vanilla keeps the memory while the villager is far away.
            return false;
        }

        let expected: &[PoiTypeRef] = match self.memory {
            JobSiteMemory::JobSite => {
                match profession_of(villager) {
                    // A `none`/`nitwit` villager holds no job site: `heldJobSite` matches
                    // nothing, so any stored site is erased on validation.
                    Some(profession) => profession.held_job_site,
                    None => &[],
                }
            }
            JobSiteMemory::PotentialJobSite => match profession_of(villager) {
                Some(profession) => profession.acquirable_job_site,
                None => &[],
            },
        };
        if poi_type_at(&world, pos).is_none_or(|poi_type| !expected.contains(&poi_type)) {
            // Vanilla erases the memory only; the POI record (and its ticket) is already
            // gone when the block changed.
            self.erase_memory(villager);
        }
        false
    }
}

/// Vanilla `GoToPotentialJobSite`: walks to the claimed potential job site.
///
/// Vanilla's `stop` releases the potential site's ticket whenever the walk ends while the
/// memory is still present (timeout or interruption). Promotion through
/// `AssignProfessionGoal` erases the memory first, which keeps the employed villager's
/// ticket.
#[derive(Default)]
pub struct GoToPotentialJobSiteGoal {
    /// Game time at which the walk times out, captured at `start` (vanilla duration 1200).
    timeout_at: Option<i64>,
}

impl Goal for GoToPotentialJobSiteGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::MOVE | GoalControls::LOOK
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(villager) = mob.downcast_ref::<VillagerEntity>() else {
            return false;
        };
        villager.job_site_memories().potential_job_site.is_some()
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        self.timeout_at = mob
            .level()
            .map(|world| world.game_time() + GOTO_POTENTIAL_TIMEOUT_TICKS);
    }

    fn can_continue_to_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(villager) = mob.downcast_ref::<VillagerEntity>() else {
            return false;
        };
        if villager.job_site_memories().potential_job_site.is_none() {
            return false;
        }
        // Vanilla `timedOut`: the walk ends (and releases the ticket) after 1200 ticks.
        self.timeout_at.is_none_or(|timeout_at| {
            mob.level()
                .is_none_or(|world| world.game_time() <= timeout_at)
        })
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        let Some(villager) = mob.downcast_ref::<VillagerEntity>() else {
            return;
        };
        let Some(pos) = villager.job_site_memories().potential_job_site else {
            return;
        };
        // Vanilla `setWalkAndLookTargetMemories(body, pos, 0.5, 1)`.
        mob.move_to_pos(
            glam::DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()) + 0.5,
                f64::from(pos.z()) + 0.5,
            ),
            GOTO_POTENTIAL_SPEED,
        );
    }

    fn stop(&mut self, mob: &dyn PathfinderMob) {
        self.timeout_at = None;
        let Some(villager) = mob.downcast_ref::<VillagerEntity>() else {
            return;
        };
        let Some(world) = mob.level() else {
            return;
        };
        let mut memories = villager.job_site_memories.lock();
        let Some(pos) = memories.potential_job_site else {
            // The memory was promoted (or erased) before the stop — the ticket stays.
            return;
        };
        memories.potential_job_site = None;
        drop(memories);
        // Vanilla releases when any POI still exists at the position (`p -> true`).
        if world.poi_storage.lock().get_type(pos).is_some() {
            let _ = world.poi_storage.lock().release_ticket(pos);
        }
    }
}

/// Vanilla `AcquirePoi.JitteredLinearRetry` — per-position backoff for candidates the
/// villager could not path to, so unreachable POIs are retried at growing intervals.
struct JitteredLinearRetry {
    previous_attempt_timestamp: i64,
    next_scheduled_attempt_timestamp: i64,
    current_delay: i32,
}

impl JitteredLinearRetry {
    fn new(now: i64) -> Self {
        let mut retry = Self {
            previous_attempt_timestamp: 0,
            next_scheduled_attempt_timestamp: 0,
            current_delay: 0,
        };
        retry.mark_attempt(now);
        retry
    }

    fn mark_attempt(&mut self, now: i64) {
        self.previous_attempt_timestamp = now;
        let suggested_delay = self.current_delay
            + rand::random_range(RETRY_MIN_INTERVAL_INCREASE..RETRY_MAX_INTERVAL_INCREASE);
        self.current_delay = suggested_delay.min(RETRY_MAX_DELAY);
        self.next_scheduled_attempt_timestamp = now + i64::from(self.current_delay);
    }

    const fn is_still_valid(&self, now: i64) -> bool {
        now - self.previous_attempt_timestamp < RETRY_VALIDITY_TICKS
    }

    const fn should_retry(&self, now: i64) -> bool {
        now >= self.next_scheduled_attempt_timestamp
    }
}

/// Vanilla `AcquirePoi` for job sites: periodically scans for the closest acquirable POI
/// with a free ticket, path-checks it, and claims the ticket on success.
///
/// Vanilla's `AcquirePoi` is a one-shot behavior that performs its whole scan inside a
/// single evaluation and never stays running. With empty goal controls a running state is
/// unobservable, so the attempt happens in `can_use` and the goal never starts.
#[derive(Default)]
pub struct AcquireJobSiteGoal {
    /// Vanilla `nextScheduledStart` — 0 until the first scan arms the timer.
    next_scheduled_start: i64,
    /// Vanilla `batchCache` — retry markers for candidates without a reachable path.
    batch_cache: FxHashMap<BlockPos, JitteredLinearRetry>,
}

impl Goal for AcquireJobSiteGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::EMPTY
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(villager) = mob.downcast_ref::<VillagerEntity>() else {
            return false;
        };
        let memories = villager.job_site_memories();
        if memories.job_site.is_some() || memories.potential_job_site.is_some() {
            return false;
        }
        // Vanilla `onlyIfAdult`; baby villagers do not look for job sites.
        if LivingEntity::is_baby(villager) {
            return false;
        }
        let Some(world) = mob.level() else {
            return false;
        };
        let now = world.game_time();

        if self.next_scheduled_start == 0 {
            self.next_scheduled_start = now + rand::random_range(0..SCAN_INTERVAL_TICKS);
            return false;
        }
        if now < self.next_scheduled_start {
            return false;
        }
        self.next_scheduled_start =
            now + SCAN_INTERVAL_TICKS + rand::random_range(0..SCAN_INTERVAL_TICKS);

        self.batch_cache
            .retain(|_, retry| retry.is_still_valid(now));

        let Some(profession) = profession_of(villager) else {
            return false;
        };
        let mut candidates = self.gather_candidates(
            &world,
            profession.acquirable_job_site,
            mob.block_position(),
            now,
        );
        candidates.truncate(SCAN_BATCH_SIZE);
        if candidates.is_empty() {
            return false;
        }

        if Self::claim_reachable_candidate(villager, &world, mob, &candidates) {
            self.batch_cache.clear();
        } else {
            self.cache_unreachable(&candidates, now);
        }
        false
    }
}

impl AcquireJobSiteGoal {
    /// Vanilla `findAllClosestFirstWithType(..., 48, HAS_SPACE)`: square query, spherical
    /// distance filter, closest-first, then the retry-cache filter.
    fn gather_candidates(
        &mut self,
        world: &Arc<World>,
        acquirable: &[PoiTypeRef],
        center: BlockPos,
        now: i64,
    ) -> Vec<(BlockPos, PoiTypeRef)> {
        let mut candidates: Vec<(BlockPos, usize)> = {
            let storage = world.poi_storage.lock();
            let mut candidates: Vec<(BlockPos, usize)> = storage.get_in_square(
                &|type_id| {
                    REGISTRY
                        .poi_types
                        .by_id(type_id)
                        .is_some_and(|poi_type| acquirable.contains(&poi_type))
                },
                center,
                SCAN_RANGE,
                OccupationStatus::Free,
            );
            let radius_squared = f64::from(SCAN_RANGE * SCAN_RANGE);
            candidates.retain(|(pos, _)| {
                let delta = pos.0 - center.0;
                f64::from(delta.x * delta.x + delta.y * delta.y + delta.z * delta.z)
                    <= radius_squared
            });
            candidates.sort_by_key(|(pos, _)| {
                let delta = pos.0 - center.0;
                delta.x * delta.x + delta.y * delta.y + delta.z * delta.z
            });
            candidates
        };
        candidates.retain(|(pos, _)| match self.batch_cache.get_mut(pos) {
            None => true,
            Some(retry) if retry.should_retry(now) => {
                retry.mark_attempt(now);
                true
            }
            Some(_) => false,
        });
        candidates
            .into_iter()
            .filter_map(|(pos, type_id)| {
                REGISTRY
                    .poi_types
                    .by_id(type_id)
                    .map(|poi_type| (pos, poi_type))
            })
            .collect()
    }

    /// Vanilla `findPathToPois` + `poiManager.take`: path to the candidates and claim the
    /// reached target's ticket into the `POTENTIAL_JOB_SITE` memory.
    fn claim_reachable_candidate(
        villager: &VillagerEntity,
        world: &Arc<World>,
        mob: &dyn PathfinderMob,
        candidates: &[(BlockPos, PoiTypeRef)],
    ) -> bool {
        let Some(profession) = profession_of(villager) else {
            return false;
        };
        let acquirable = profession.acquirable_job_site;

        // Vanilla reach range is the largest candidate valid range.
        let reach_range = candidates
            .iter()
            .map(|(_, poi_type)| i32::try_from(poi_type.search_distance).unwrap_or(1))
            .max()
            .unwrap_or(1)
            .max(1);
        let targets: Vec<BlockPos> = candidates.iter().map(|(pos, _)| *pos).collect();
        let Some(path) = mob.create_path_to_targets(world, &targets, reach_range) else {
            return false;
        };
        if !path.can_reach() {
            return false;
        }

        let target_pos = path.target();
        let claimed = {
            let mut storage = world.poi_storage.lock();
            storage.get_type(target_pos).is_some_and(|type_id| {
                REGISTRY
                    .poi_types
                    .by_id(type_id)
                    .is_some_and(|poi_type| acquirable.contains(&poi_type))
            }) && storage.reserve_ticket(target_pos)
        };
        if !claimed {
            return false;
        }
        villager.job_site_memories.lock().potential_job_site = Some(target_pos);
        true
    }

    fn cache_unreachable(&mut self, candidates: &[(BlockPos, PoiTypeRef)], now: i64) {
        for (pos, _) in candidates {
            self.batch_cache
                .entry(*pos)
                .or_insert_with(|| JitteredLinearRetry::new(now));
        }
    }
}

/// Vanilla `AssignProfessionFromJobSite`: promotes the potential job site into an actual
/// employment and grants the matching profession.
///
/// Like `AcquirePoi` this is a one-shot behavior, so the promotion happens in `can_use`.
pub struct AssignProfessionGoal;

impl Goal for AssignProfessionGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::EMPTY
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(villager) = mob.downcast_ref::<VillagerEntity>() else {
            return false;
        };
        let Some(pos) = villager.job_site_memories().potential_job_site else {
            return false;
        };
        if !closer_to_center_than(pos, mob.position(), ASSIGN_MAX_DISTANCE) {
            return false;
        }

        {
            let mut memories = villager.job_site_memories.lock();
            memories.potential_job_site = None;
            memories.job_site = Some(pos);
        }
        // Vanilla broadcasts entity event 14 (happy villager particles).
        villager.broadcast_entity_event(EntityStatus::VillagerHappy);

        // Vanilla assigns the first registered profession (registry order) whose held job
        // site matches the POI type at the position.
        let Some(world) = mob.level() else {
            return false;
        };
        let Some(poi_type) = poi_type_at(&world, pos) else {
            return false;
        };
        let matches = (0..REGISTRY.villager_professions.len())
            .filter_map(|id| REGISTRY.villager_professions.by_id(id))
            .find(|profession| profession.held_job_site.contains(&poi_type));
        if let Some(profession) = matches {
            villager.set_profession(profession);
        }
        false
    }
}

/// Vanilla `ResetProfession`: an employed villager that lost its `JOB_SITE` returns to
/// `minecraft:none` when it is still at the bottom trade tier and has no career XP.
pub struct ResetProfessionGoal;

impl Goal for ResetProfessionGoal {
    fn controls(&self) -> GoalControls {
        GoalControls::EMPTY
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(villager) = mob.downcast_ref::<VillagerEntity>() else {
            return false;
        };
        if villager.job_site_memories().job_site.is_some() {
            return false;
        }
        let Some(profession) = profession_of(villager) else {
            return false;
        };
        if profession.key == vanilla_villager_professions::NONE.key
            || profession.key == vanilla_villager_professions::NITWIT.key
        {
            return false;
        }
        if villager.entity_data.lock().villager_data.get().level > 1 {
            return false;
        }
        if villager.villager_xp() != 0 {
            return false;
        }

        villager.set_profession(&vanilla_villager_professions::NONE);
        false
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::{vanilla_blocks, vanilla_entities, vanilla_villager_professions};
    use steel_utils::ChunkPos;
    use steel_utils::types::UpdateFlags;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::entity::{EntityOwnership, Mob as _, SharedEntity, next_entity_id};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};
    use steel_registry::init_vanilla_registry;

    const SITE: BlockPos = BlockPos::new(8, 64, 8);
    /// A villager standing right next to the site (within the 2.0 assignment distance).
    const VILLAGER_POSITION: DVec3 = DVec3::new(8.5, 64.0, 9.5);

    fn spawn_villager(world: &Arc<World>) -> Arc<VillagerEntity> {
        insert_ready_full_chunk(world, ChunkPos::from_block_pos(SITE));
        for x in 5..=11 {
            for z in 5..=11 {
                assert!(world.set_block(
                    BlockPos::new(x, 63, z),
                    vanilla_blocks::DIRT.default_state(),
                    UpdateFlags::UPDATE_NONE,
                ));
            }
        }

        let villager = Arc::new(VillagerEntity::new(
            &vanilla_entities::VILLAGER,
            next_entity_id(),
            VILLAGER_POSITION,
            Arc::downgrade(world),
        ));
        villager.set_on_ground(true);
        let villager_arc: Arc<VillagerEntity> = Arc::clone(&villager);
        let shared: SharedEntity = villager_arc;
        world
            .entity_manager()
            .add_live_entity(shared, EntityOwnership::External)
            .expect("villager should register into the test world");
        villager
    }

    /// Drives the goal selector, advancing game time until `until` succeeds.
    fn run_goal_passes(
        world: &Arc<World>,
        villager: &VillagerEntity,
        max_passes: usize,
        mut until: impl FnMut() -> bool,
    ) {
        for _ in 0..max_passes {
            if until() {
                return;
            }
            let next_game_time = world.game_time() + 40;
            world.level_data.write().set_game_time(next_game_time);
            villager.mob_base().goal_selector().lock().tick(villager);
        }
        panic!("job-site condition not reached within {max_passes} selector passes");
    }

    #[test]
    fn placing_job_site_block_employs_nearby_unemployed_villager() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("villager_job_site_claim");
        world.level_data.write().set_game_time(1000);

        let villager = spawn_villager(&world);
        let none_id = i32::try_from(
            vanilla_villager_professions::NONE
                .try_id()
                .expect("none id"),
        )
        .expect("none id fits i32");
        let farmer_id = i32::try_from(
            vanilla_villager_professions::FARMER
                .try_id()
                .expect("farmer id"),
        )
        .expect("farmer id fits i32");
        assert_eq!(villager.profession_id(), none_id);

        // The user-facing scenario: a job-site block is placed next to the villager.
        assert!(world.set_block(
            SITE,
            vanilla_blocks::COMPOSTER.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));

        run_goal_passes(&world, &villager, 100, || {
            villager.profession_id() == farmer_id
        });

        // The villager holds the composter: JOB_SITE memory set, ticket taken, trades
        // available.
        let memories = villager.job_site_memories();
        assert_eq!(memories.job_site, Some(SITE));
        assert_eq!(memories.potential_job_site, None);
        assert!(
            !world.poi_storage.lock().reserve_ticket(SITE),
            "the employed villager should hold the site's only ticket"
        );
        assert!(!villager.offers.lock().is_empty());
    }

    #[test]
    fn breaking_the_job_site_block_resets_the_villager() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("villager_job_site_reset");
        world.level_data.write().set_game_time(1000);

        let villager = spawn_villager(&world);
        let none_id = i32::try_from(
            vanilla_villager_professions::NONE
                .try_id()
                .expect("none id"),
        )
        .expect("none id fits i32");
        let farmer_id = i32::try_from(
            vanilla_villager_professions::FARMER
                .try_id()
                .expect("farmer id"),
        )
        .expect("farmer id fits i32");

        assert!(world.set_block(
            SITE,
            vanilla_blocks::COMPOSTER.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        run_goal_passes(&world, &villager, 100, || {
            villager.profession_id() == farmer_id
        });

        world.set_block(
            SITE,
            vanilla_blocks::AIR.default_state(),
            UpdateFlags::UPDATE_NONE,
        );

        run_goal_passes(&world, &villager, 100, || {
            villager.profession_id() == none_id
        });

        assert_eq!(villager.job_site_memories().job_site, None);
        assert!(
            villager.offers.lock().is_empty(),
            "an unemployed villager holds no offers"
        );
        assert!(world.poi_storage.lock().get_type(SITE).is_none());
    }
}
