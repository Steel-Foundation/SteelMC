//! Brain memory storage

use glam::DVec3;
use rustc_hash::FxHashMap;
use steel_utils::BlockPos;

use crate::entity::{PathfinderMob, SharedEntity};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MemoryModuleType {
    WalkTarget,
    LookTarget,
    NearestVisibleLivingEntities,
    Home,
    JobSite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryStatus {
    ValuePresent,
    ValueAbsent,
    Registered,
}

#[derive(Clone)]
pub(crate) enum PositionTracker {
    Block(BlockPos),
    Entity {
        entity: SharedEntity,
        track_eye_height: bool,
    },
}

impl PositionTracker {
    #[must_use]
    pub(crate) fn current_position(&self) -> DVec3 {
        match self {
            Self::Block(pos) => DVec3::new(
                f64::from(pos.x()) + 0.5,
                f64::from(pos.y()) + 0.5,
                f64::from(pos.z()) + 0.5,
            ),
            Self::Entity {
                entity,
                track_eye_height,
            } => {
                if *track_eye_height {
                    let position = entity.position();
                    DVec3::new(position.x, entity.get_eye_y(), position.y)
                } else {
                    entity.position()
                }
            }
        }
    }

    pub(crate) fn is_visivle_by(&self, _mob: &dyn PathfinderMob) -> bool {
        match self {
            Self::Block(_) => true,
            Self::Entity { entity, .. } => entity.is_alive(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct WalkTarget {
    target: PositionTracker,
    speed_modifier: f32,
    close_enough_dist: i32,
}

impl WalkTarget {
    #[must_use]
    pub(crate) const fn new(
        target: PositionTracker,
        speed_modifier: f32,
        close_enough_dist: i32,
    ) -> Self {
        Self {
            target,
            speed_modifier,
            close_enough_dist,
        }
    }

    #[must_use]
    pub(crate) const fn from_block(
        pos: BlockPos,
        speed_modifier: f32,
        close_enough_dist: i32,
    ) -> Self {
        Self::new(
            PositionTracker::Block(pos),
            speed_modifier,
            close_enough_dist,
        )
    }

    #[must_use]
    pub(crate) const fn target(&self) -> &PositionTracker {
        &self.target
    }

    #[must_use]
    pub(crate) const fn speed_modifier(&self) -> f32 {
        self.speed_modifier
    }

    #[must_use]
    pub(crate) const fn close_enough_dist(&self) -> i32 {
        self.close_enough_dist
    }
}

#[derive(Clone)]
enum MemoryValue {
    WalkTarget(WalkTarget),
    LookTarget(PositionTracker),
    LivingEntities(Vec<SharedEntity>),
    Home(BlockPos),
    JobSite(BlockPos),
}

impl MemoryValue {
    fn is_empty_collection(&self) -> bool {
        matches!(self, Self::LivingEntities(entities) if entities.is_empty())
    }
}

#[derive(Clone)]
struct ExpirableValue {
    value: MemoryValue,
    time_to_live: i64,
}

impl ExpirableValue {
    const fn of(value: MemoryValue) -> Self {
        Self {
            value,
            time_to_live: i64::MAX,
        }
    }

    const fn of_with_expiry(value: MemoryValue, time_to_live: i64) -> Self {
        Self {
            value,
            time_to_live,
        }
    }

    const fn can_expire(&self) -> bool {
        self.time_to_live != i64::MAX
    }

    const fn has_expired(&self) -> bool {
        self.time_to_live <= 0
    }

    const fn tick(&mut self) {
        if self.can_expire() {
            self.time_to_live -= 1;
        }
    }

    const fn value(&self) -> &MemoryValue {
        &self.value
    }
}

pub(crate) struct Memories {
    memories: FxHashMap<MemoryModuleType, Option<ExpirableValue>>,
}

impl Memories {
    #[must_use]
    pub(crate) fn new(types: impl IntoIterator<Item = MemoryModuleType>) -> Self {
        Self {
            memories: types.into_iter().map(|ty| (ty, None)).collect(),
        }
    }

    #[must_use]
    pub(crate) fn check_memory(&self, ty: MemoryModuleType, status: MemoryStatus) -> bool {
        let Some(slot) = self.memories.get(&ty) else {
            return false;
        };
        match status {
            MemoryStatus::Registered => true,
            MemoryStatus::ValuePresent => slot.is_some(),
            MemoryStatus::ValueAbsent => slot.is_none(),
        }
    }

    #[must_use]
    pub(crate) fn has_value(&self, ty: MemoryModuleType) -> bool {
        self.check_memory(ty, MemoryStatus::ValuePresent)
    }

    #[must_use]
    pub(crate) fn home(&self) -> Option<BlockPos> {
        if let Some(MemoryValue::Home(pos)) = self.get(MemoryModuleType::Home) {
            Some(*pos)
        } else {
            None
        }
    }

    pub(crate) fn set_home(&mut self, pos: BlockPos) {
        self.set_internal(
            MemoryModuleType::Home,
            Some(ExpirableValue::of(MemoryValue::Home(pos))),
        );
    }

    #[must_use]
    pub(crate) fn job_site(&self) -> Option<BlockPos> {
        if let Some(MemoryValue::JobSite(pos)) = self.get(MemoryModuleType::JobSite) {
            Some(*pos)
        } else {
            None
        }
    }

    pub(crate) fn set_job_stite(&mut self, pos: BlockPos) {
        self.set_internal(
            MemoryModuleType::JobSite,
            Some(ExpirableValue::of(MemoryValue::JobSite(pos))),
        );
    }

    #[must_use]
    pub(crate) fn nearest_visible_living_entities(&self) -> Option<&[SharedEntity]> {
        if let Some(MemoryValue::LivingEntities(entities)) =
            self.get(MemoryModuleType::NearestVisibleLivingEntities)
        {
            Some(entities.as_slice())
        } else {
            None
        }
    }

    pub(crate) fn set_nearest_visible_living_entities(&mut self, entities: Vec<SharedEntity>) {
        self.set_internal(
            MemoryModuleType::NearestVisibleLivingEntities,
            Some(ExpirableValue::of(MemoryValue::LivingEntities(entities))),
        );
    }

    pub(crate) fn forget_outdated(&mut self) {
        for slot in self.memories.values_mut() {
            if let Some(value) = slot {
                if value.has_expired() {
                    *slot = None;
                } else {
                    value.tick();
                }
            }
        }
    }

    pub(crate) fn erase(&mut self, ty: MemoryModuleType) {
        self.set_internal(ty, None);
    }

    #[must_use]
    pub(crate) fn walk_target(&self) -> Option<&WalkTarget> {
        if let Some(MemoryValue::WalkTarget(walk_target)) = self.get(MemoryModuleType::WalkTarget) {
            Some(walk_target)
        } else {
            None
        }
    }

    #[must_use]
    pub(crate) fn look_target(&self) -> Option<&PositionTracker> {
        if let Some(MemoryValue::LookTarget(look_target)) = self.get(MemoryModuleType::LookTarget) {
            Some(look_target)
        } else {
            None
        }
    }

    pub(crate) fn set_walk_target(&mut self, walk_target: WalkTarget) {
        self.set_internal(
            MemoryModuleType::WalkTarget,
            Some(ExpirableValue::of(MemoryValue::WalkTarget(walk_target))),
        );
    }

    pub(crate) fn set_walk_target_with_expiry(
        &mut self,
        walk_target: WalkTarget,
        time_to_live: i64,
    ) {
        self.set_internal(
            MemoryModuleType::WalkTarget,
            Some(ExpirableValue::of_with_expiry(
                MemoryValue::WalkTarget(walk_target),
                time_to_live,
            )),
        );
    }

    pub(crate) fn set_look_target(&mut self, look_target: PositionTracker) {
        self.set_internal(
            MemoryModuleType::LookTarget,
            Some(ExpirableValue::of(MemoryValue::LookTarget(look_target))),
        );
    }

    fn get(&self, ty: MemoryModuleType) -> Option<&MemoryValue> {
        self.memories.get(&ty)?.as_ref().map(ExpirableValue::value)
    }

    fn set_internal(&mut self, ty: MemoryModuleType, value: Option<ExpirableValue>) {
        if let Some(slot) = self.memories.get_mut(&ty) {
            *slot = value.filter(|expirable| !expirable.value().is_empty_collection());
        }
    }
}
