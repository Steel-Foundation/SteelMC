//! Leash fence knot entity foundation.

use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::sound_events;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entities;
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey, WorldAabb};

use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, RemovalReason, SharedEntity, next_entity_id,
};
use crate::world::World;

/// Vanilla leash knot attached to a fence block.
#[entity_behavior(class = "leash_fence_knot_entity", identifier = "leash_knot")]
pub struct LeashFenceKnotEntity {
    base: Weak<EntityBase>,
    /// blockpos of the entity
    pub block_pos: BlockPos,
    check_interval: i32,
}

// SAFETY: This key is owned by Steel and uniquely identifies `LeashFenceKnotEntity`.
unsafe impl DowncastType for LeashFenceKnotEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/leash_fence_knot");
}

impl LeashFenceKnotEntity {
    fn build(base: Weak<EntityBase>, block_pos: BlockPos) -> Self {
        Self {
            base,
            block_pos,
            check_interval: 0,
        }
    }

    /// Creates a fresh leash knot. Used by the generated factory.
    ///
    /// The attachment block position is derived from `position`.
    #[must_use]
    pub fn new(id: i32, position: DVec3, world: Weak<World>) -> SharedEntity {
        let block_pos = BlockPos::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        );
        EntityBase::pack_with(
            id,
            Self::knot_center(block_pos),
            vanilla_entities::LEASH_KNOT.dimensions,
            world,
            |base| Self::build(base, block_pos),
        )
    }

    /// Creates a fresh leash knot attached to `block_pos`.
    #[must_use]
    pub fn new_attached(block_pos: BlockPos) -> SharedEntity {
        EntityBase::pack_with(
            next_entity_id(),
            Self::knot_center(block_pos),
            vanilla_entities::LEASH_KNOT.dimensions,
            Weak::new(),
            |base| Self::build(base, block_pos),
        )
    }

    /// Creates a leash knot from persistent entity data.
    #[must_use]
    pub fn from_saved(load: EntityBaseLoad) -> SharedEntity {
        let position = load.position;
        let block_pos = BlockPos::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        );
        EntityBase::pack_loaded_with(load, vanilla_entities::LEASH_KNOT.dimensions, |base| {
            Self::build(base, block_pos)
        })
    }

    /// Returns true when the backing fence block still supports this knot.
    #[must_use]
    pub fn survives(&self) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        world
            .get_block_state(self.block_pos)
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
                let mut entity = entity.lock_entity();
                entity
                    .downcast::<Self>()
                    .is_some_and(|knot| knot.block_pos == pos)
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

        let knot: SharedEntity = Self::new_attached(pos);
        if let Err(error) = world.try_add_entity(Arc::clone(&knot)) {
            log::warn!("Failed to spawn leash knot entity: {error}");
            return None;
        }

        Some(knot)
    }

    fn should_check_survival(&mut self) -> bool {
        if self.check_interval == 100 {
            self.check_interval = 0;
            true
        } else {
            self.check_interval += 1;
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

    /// Todo
    #[allow(unused)]
    fn knot_bounding_box(block_pos: BlockPos) -> WorldAabb {
        let center = Self::knot_center(block_pos);
        let half_width = f64::from(vanilla_entities::LEASH_KNOT.dimensions.width) / 2.0;
        let height = f64::from(vanilla_entities::LEASH_KNOT.dimensions.height);
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
    fn base_weak(&self) -> &Weak<EntityBase> {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::LEASH_KNOT
    }

    fn spawn_position(&self) -> DVec3 {
        let block_pos = self.block_pos;
        DVec3::new(
            f64::from(block_pos.x()),
            f64::from(block_pos.y()),
            f64::from(block_pos.z()),
        )
    }

    fn tick(&mut self) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use simdnbt::owned::NbtCompound;

    #[test]
    fn leash_knot_uses_vanilla_position_and_bounding_box() {
        let knot = LeashFenceKnotEntity::new_attached(BlockPos::new(4, 65, -9));

        assert_eq!(knot.position(), DVec3::new(4.5, 65.375, -8.5));
        assert_eq!(
            knot.bounding_box(),
            LeashFenceKnotEntity::knot_bounding_box(BlockPos::new(4, 65, -9))
        );
    }

    #[test]
    fn leash_knot_spawn_packet_uses_attached_block_pos() {
        let knot = LeashFenceKnotEntity::new_attached(BlockPos::new(4, 65, -9));

        assert_eq!(
            knot.with_entity(|e| e.spawn_position()),
            DVec3::new(4.0, 65.0, -9.0)
        );
    }

    #[test]
    fn leash_knot_saves_no_type_specific_block_pos() {
        let knot = LeashFenceKnotEntity::new_attached(BlockPos::new(4, 65, -9));

        let mut nbt = NbtCompound::new();
        knot.with_entity(|e| e.save_additional(&mut nbt));

        assert!(nbt.is_empty());
    }

    #[test]
    fn leash_knot_survival_check_matches_vanilla_interval() {
        let knot = LeashFenceKnotEntity::new_attached(BlockPos::new(4, 65, -9));

        {
            let mut knot = knot.lock_entity();
            let knot: &mut LeashFenceKnotEntity = knot.downcast().unwrap();
            for _ in 0..100 {
                assert!(!knot.should_check_survival());
            }
            assert!(knot.should_check_survival());
            assert!(!knot.should_check_survival());
        }
    }
}
