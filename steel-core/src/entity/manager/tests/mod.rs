use std::sync::{
    Arc, Barrier, Weak,
    atomic::{AtomicUsize, Ordering},
};

use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entities;
use steel_utils::locks::SyncMutex;
use uuid::Uuid;

use crate::entity::{Entity, EntityBase, EntityLevelCallback, InactiveEntityCallback};

use super::*;
use crate::entity::{EntityHooks, EntityProps};

struct ManagerTestEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    always_ticking: bool,
    hard_collision_relevant: bool,
}

struct DelayedFirstBoundsCallback {
    entity_id: i32,
    manager: Arc<WorldEntityManager>,
    first_callback_entered: Arc<Barrier>,
    release_first_callback: Arc<Barrier>,
    callback_count: AtomicUsize,
}

impl EntityLevelCallback for DelayedFirstBoundsCallback {
    fn validate_move(&self, _old_pos: DVec3, _new_pos: DVec3) -> Result<(), EntityMoveError> {
        Ok(())
    }

    fn on_move_committed(&self, _old_pos: DVec3, _new_pos: DVec3) -> Result<(), EntityMoveError> {
        Ok(())
    }

    fn on_bounding_box_changed(&self, _bounding_box: WorldAabb) {
        if self.callback_count.fetch_add(1, Ordering::SeqCst) == 0 {
            self.first_callback_entered.wait();
            self.release_first_callback.wait();
        }
        self.manager.commit_bounding_box_change(self.entity_id);
    }

    fn on_remove(&self, _reason: RemovalReason) {}
}

impl ManagerTestEntity {
    fn shared(id: i32, uuid: Uuid, position: DVec3) -> SharedEntity {
        Self::shared_with_type(id, uuid, position, &vanilla_entities::ITEM)
    }

    fn shared_with_type(
        id: i32,
        uuid: Uuid,
        position: DVec3,
        entity_type: EntityTypeRef,
    ) -> SharedEntity {
        Arc::new(Self {
            base: EntityBase::with_uuid(id, uuid, position, entity_type.dimensions, Weak::new()),
            entity_type,
            always_ticking: false,
            hard_collision_relevant: true,
        })
    }

    fn shared_non_hard(id: i32, uuid: Uuid, position: DVec3) -> SharedEntity {
        Arc::new(Self {
            base: EntityBase::with_uuid(
                id,
                uuid,
                position,
                vanilla_entities::ITEM.dimensions,
                Weak::new(),
            ),
            entity_type: &vanilla_entities::ITEM,
            always_ticking: false,
            hard_collision_relevant: false,
        })
    }

    fn shared_always_ticking(id: i32, uuid: Uuid, position: DVec3) -> SharedEntity {
        Arc::new(Self {
            base: EntityBase::with_uuid(
                id,
                uuid,
                position,
                vanilla_entities::ITEM.dimensions,
                Weak::new(),
            ),
            entity_type: &vanilla_entities::ITEM,
            always_ticking: true,
            hard_collision_relevant: true,
        })
    }
}

struct MovingTickTestEntity {
    base: EntityBase,
    tick_position: DVec3,
    tick_rotation: (f32, f32),
}

impl MovingTickTestEntity {
    fn shared(
        id: i32,
        uuid: Uuid,
        position: DVec3,
        tick_position: DVec3,
        tick_rotation: (f32, f32),
    ) -> SharedEntity {
        Arc::new(Self {
            base: EntityBase::with_uuid(
                id,
                uuid,
                position,
                vanilla_entities::ITEM.dimensions,
                Weak::new(),
            ),
            tick_position,
            tick_rotation,
        })
    }
}

crate::entity::impl_test_downcast_type!(MovingTickTestEntity);

impl MovingTickTestEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        tick: Self::entity_tick,
        ..EntityProps::new()
    };

    fn entity_tick(&self, _entity: &SharedEntity) {
        self.tick_default();
        if let Err(error) = self.try_set_position(self.tick_position) {
            panic!("moving tick test entity failed to move during tick: {error}");
        }
        self.set_rotation(self.tick_rotation);
    }
}

impl Entity for MovingTickTestEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }

    fn entity_hooks(&self) -> &'static dyn EntityHooks {
        &Self::ENTITY
    }
}

struct AddDuringTickTestEntity {
    base: EntityBase,
    manager: Arc<WorldEntityManager>,
    entity_to_add: SyncMutex<Option<SharedEntity>>,
}

impl AddDuringTickTestEntity {
    fn shared(
        id: i32,
        uuid: Uuid,
        position: DVec3,
        manager: Arc<WorldEntityManager>,
        entity_to_add: SharedEntity,
    ) -> SharedEntity {
        Arc::new(Self {
            base: EntityBase::with_uuid(
                id,
                uuid,
                position,
                vanilla_entities::ITEM.dimensions,
                Weak::new(),
            ),
            manager,
            entity_to_add: SyncMutex::new(Some(entity_to_add)),
        })
    }
}

crate::entity::impl_test_downcast_type!(AddDuringTickTestEntity);

impl AddDuringTickTestEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        tick: Self::entity_tick,
        ..EntityProps::new()
    };

    fn entity_tick(&self, _entity: &SharedEntity) {
        self.tick_default();
        let Some(entity) = self.entity_to_add.lock().take() else {
            return;
        };
        if let Err(error) = self
            .manager
            .add_live_entity(entity, EntityOwnership::ManagerOwned)
        {
            panic!("add-during-tick test entity failed to add live entity: {error}");
        }
    }
}

impl Entity for AddDuringTickTestEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }

    fn entity_hooks(&self) -> &'static dyn EntityHooks {
        &Self::ENTITY
    }
}

struct DespawnOnCheckTestEntity {
    base: EntityBase,
}

impl DespawnOnCheckTestEntity {
    fn shared(id: i32, uuid: Uuid, position: DVec3) -> SharedEntity {
        Arc::new(Self {
            base: EntityBase::with_uuid(
                id,
                uuid,
                position,
                vanilla_entities::ITEM.dimensions,
                Weak::new(),
            ),
        })
    }
}

crate::entity::impl_test_downcast_type!(DespawnOnCheckTestEntity);

impl DespawnOnCheckTestEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        check_despawn: Self::entity_check_despawn,
        ..EntityProps::new()
    };

    fn entity_check_despawn(&self) {
        self.set_removed(RemovalReason::Discarded);
    }
}

impl Entity for DespawnOnCheckTestEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::ITEM
    }

    fn entity_hooks(&self) -> &'static dyn EntityHooks {
        &Self::ENTITY
    }
}

crate::entity::impl_test_downcast_type!(ManagerTestEntity);

impl ManagerTestEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        is_always_ticking: |entity| entity.always_ticking,
        is_hard_collision_relevant: |entity| entity.hard_collision_relevant,
        ..EntityProps::new()
    };
}

impl Entity for ManagerTestEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn entity_hooks(&self) -> &'static dyn EntityHooks {
        &Self::ENTITY
    }
}

fn entity(id: i32, uuid_seed: u128, position: DVec3) -> SharedEntity {
    ManagerTestEntity::shared(id, Uuid::from_u128(uuid_seed), position)
}

fn assert_empty_lifecycle(changes: EntityLifecycleChanges) {
    assert!(changes.tracking_started.is_empty());
    assert!(changes.tracking_stopped.is_empty());
    assert!(changes.ticking_started.is_empty());
    assert!(changes.ticking_stopped.is_empty());
}

fn load_chunk(manager: &WorldEntityManager, chunk: ChunkPos) {
    let result = manager.on_chunk_loaded(chunk);
    assert!(result.restored.is_empty());
    assert!(result.tracking_started.is_empty());
    assert!(result.ticking_started.is_empty());
    assert!(!result.needs_save);
    assert_empty_lifecycle(manager.update_chunk_visibility(chunk, EntityVisibility::Ticking));
}

fn track_chunk(manager: &WorldEntityManager, chunk: ChunkPos) {
    let result = manager.on_chunk_loaded(chunk);
    assert!(result.restored.is_empty());
    assert!(result.tracking_started.is_empty());
    assert!(result.ticking_started.is_empty());
    assert!(!result.needs_save);
    assert_empty_lifecycle(manager.update_chunk_visibility(chunk, EntityVisibility::Tracked));
}

mod lifecycle;
mod movement_and_visibility;
mod persistence;
mod queries_and_order;
mod spatial_queries;
mod ticking;
