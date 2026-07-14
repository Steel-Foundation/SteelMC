//! Leash fence knot entity foundation.

use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::sound_events;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entities;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey, WorldAabb};

use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityBaseState, EntityCapabilities, LeashFenceKnot,
    RemovalReason, SharedEntity, next_entity_id,
};
use crate::world::World;

/// Vanilla leash knot attached to a fence block.
#[entity_behavior(class = "LeashFenceKnotEntity")]
pub struct LeashFenceKnotEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    block_pos: SyncMutex<BlockPos>,
    check_interval: SyncMutex<i32>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `LeashFenceKnotEntity`.
unsafe impl DowncastType for LeashFenceKnotEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/leash_fence_knot");
}

impl LeashFenceKnotEntity {
    /// Creates a fresh leash knot from the generic entity factory path.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_attached(
            entity_type,
            id,
            BlockPos::new(
                position.x.floor() as i32,
                position.y.floor() as i32,
                position.z.floor() as i32,
            ),
            world,
        )
    }

    /// Creates a fresh leash knot attached to `block_pos`.
    #[must_use]
    pub fn new_attached(
        entity_type: EntityTypeRef,
        id: i32,
        block_pos: BlockPos,
        world: Weak<World>,
    ) -> Self {
        Self {
            base: EntityBase::new_with_state(
                id,
                EntityBaseState::new_with_bounding_box(
                    Self::knot_center(block_pos),
                    entity_type.dimensions,
                    Self::knot_bounding_box(entity_type, block_pos),
                ),
                world,
            ),
            entity_type,
            block_pos: SyncMutex::new(block_pos),
            check_interval: SyncMutex::new(0),
        }
    }

    /// Creates a leash knot from persistent entity data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        let position = load.position;
        let block_pos = BlockPos::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        );
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            block_pos: SyncMutex::new(block_pos),
            check_interval: SyncMutex::new(0),
        }
    }

    /// Returns the fence block this knot is attached to.
    #[must_use]
    pub fn block_pos(&self) -> BlockPos {
        *self.block_pos.lock()
    }

    /// Returns true when the backing fence block still supports this knot.
    #[must_use]
    pub fn survives(&self) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        world
            .get_block_state(self.block_pos())
            .get_block()
            .has_tag(&BlockTag::FENCES)
    }

    /// Finds an existing leash knot at `pos`.
    #[must_use]
    pub fn get_knot(world: &World, pos: BlockPos) -> Option<SharedEntity> {
        let search_box = WorldAabb::new(
            f64::from(pos.x()) - 1.0,
            f64::from(pos.y()) - 1.0,
            f64::from(pos.z()) - 1.0,
            f64::from(pos.x()) + 1.0,
            f64::from(pos.y()) + 1.0,
            f64::from(pos.z()) + 1.0,
        );
        world
            .get_entities_in_aabb_matching(&search_box, |entity| {
                entity
                    .as_leash_fence_knot()
                    .is_some_and(|knot| knot.leash_fence_pos() == pos)
            })
            .into_iter()
            .next()
    }

    /// Gets or creates a leash knot at `pos`.
    #[must_use]
    pub fn get_or_create_knot(world: &Arc<World>, pos: BlockPos) -> Option<SharedEntity> {
        if let Some(knot) = Self::get_knot(world.as_ref(), pos) {
            return Some(knot);
        }

        let knot: SharedEntity = Arc::new(Self::new_attached(
            &vanilla_entities::LEASH_KNOT,
            next_entity_id(),
            pos,
            Arc::downgrade(world),
        ));
        if let Err(error) = world.try_add_entity(Arc::clone(&knot)) {
            log::warn!("Failed to spawn leash knot entity: {error}");
            return None;
        }

        Some(knot)
    }

    fn should_check_survival(&self) -> bool {
        let mut check_interval = self.check_interval.lock();
        if *check_interval == 100 {
            *check_interval = 0;
            true
        } else {
            *check_interval += 1;
            false
        }
    }

    fn play_drop_sound(&self) {
        self.play_sound(&sound_events::ITEM_LEAD_UNTIED, 1.0, 1.0);
    }

    fn knot_center(block_pos: BlockPos) -> DVec3 {
        DVec3::new(
            f64::from(block_pos.x()) + 0.5,
            f64::from(block_pos.y()) + 0.375,
            f64::from(block_pos.z()) + 0.5,
        )
    }

    fn knot_bounding_box(entity_type: EntityTypeRef, block_pos: BlockPos) -> WorldAabb {
        let center = Self::knot_center(block_pos);
        let half_width = f64::from(entity_type.dimensions.width) / 2.0;
        let height = f64::from(entity_type.dimensions.height);
        WorldAabb::new(
            center.x - half_width,
            center.y,
            center.z - half_width,
            center.x + half_width,
            center.y + height,
            center.z + half_width,
        )
    }
}

impl Entity for LeashFenceKnotEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn spawn_position(&self) -> DVec3 {
        let block_pos = self.block_pos();
        DVec3::new(
            f64::from(block_pos.x()),
            f64::from(block_pos.y()),
            f64::from(block_pos.z()),
        )
    }

    fn capabilities(&self) -> EntityCapabilities<'_> {
        EntityCapabilities::none().with_leash_fence_knot(self)
    }

    fn notify_leashee_removed(&self, _leashable: &dyn Entity) {
        if self.level().is_some() && self.leashables_leashed_to().is_empty() {
            self.set_removed(RemovalReason::Discarded);
        }
    }

    fn tick(&self) {
        if self.level().is_none() {
            return;
        }
        self.check_below_world();
        if self.should_check_survival() && !self.is_removed() && !self.survives() {
            self.set_removed(RemovalReason::Discarded);
            self.play_drop_sound();
        }
    }

    fn is_pickable(&self) -> bool {
        true
    }
}

impl LeashFenceKnot for LeashFenceKnotEntity {
    fn leash_fence_pos(&self) -> BlockPos {
        self.block_pos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use simdnbt::owned::NbtCompound;

    #[test]
    fn leash_knot_uses_vanilla_position_and_bounding_box() {
        let knot = LeashFenceKnotEntity::new_attached(
            &vanilla_entities::LEASH_KNOT,
            1,
            BlockPos::new(4, 65, -9),
            Weak::new(),
        );

        assert_eq!(knot.position(), DVec3::new(4.5, 65.375, -8.5));
        assert_eq!(
            knot.bounding_box(),
            LeashFenceKnotEntity::knot_bounding_box(
                &vanilla_entities::LEASH_KNOT,
                BlockPos::new(4, 65, -9)
            )
        );
    }

    #[test]
    fn leash_knot_spawn_packet_uses_attached_block_pos() {
        let knot = LeashFenceKnotEntity::new_attached(
            &vanilla_entities::LEASH_KNOT,
            1,
            BlockPos::new(4, 65, -9),
            Weak::new(),
        );

        assert_eq!(knot.spawn_position(), DVec3::new(4.0, 65.0, -9.0));
    }

    #[test]
    fn leash_knot_saves_no_type_specific_block_pos() {
        let knot = LeashFenceKnotEntity::new_attached(
            &vanilla_entities::LEASH_KNOT,
            1,
            BlockPos::new(4, 65, -9),
            Weak::new(),
        );

        let mut nbt = NbtCompound::new();
        knot.save_additional(&mut nbt);

        assert!(nbt.is_empty());
    }

    #[test]
    fn leash_knot_survival_check_matches_vanilla_interval() {
        let knot = LeashFenceKnotEntity::new_attached(
            &vanilla_entities::LEASH_KNOT,
            1,
            BlockPos::new(4, 65, -9),
            Weak::new(),
        );

        for _ in 0..100 {
            assert!(!knot.should_check_survival());
        }
        assert!(knot.should_check_survival());
        assert!(!knot.should_check_survival());
    }
}
