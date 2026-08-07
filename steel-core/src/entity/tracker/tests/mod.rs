use std::{
    mem,
    sync::{Arc, Weak},
};

use steel_protocol::packets::game::{AttributeSnapshot, EquipmentSlotItem};
use steel_registry::item_stack::ItemStack;
use steel_registry::{
    entity_type::EntityTypeRef, init_vanilla_registry, vanilla_entities, vanilla_items,
};
use steel_utils::BlockPos;

use super::*;
use crate::entity::{
    EntityBase, LivingEntity, LivingEntityBase, Mob,
    entities::{LeashFenceKnotEntity, PigEntity},
};
use crate::inventory::equipment::EquipmentSlot;

struct PairingTestEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    attributes: Vec<AttributeSnapshot>,
    dirty_attributes: SyncMutex<Vec<AttributeSnapshot>>,
    equipment: SyncMutex<Vec<EquipmentSlotItem>>,
    dirty_equipment: SyncMutex<Vec<EquipmentSlotItem>>,
    passengers: SyncMutex<Vec<WeakEntity>>,
    vehicle: SyncMutex<Option<WeakEntity>>,
}

impl PairingTestEntity {
    fn new(id: i32, attributes: Vec<AttributeSnapshot>) -> Arc<Self> {
        Self::new_with_type(id, &vanilla_entities::ITEM, attributes)
    }

    fn new_with_type(
        id: i32,
        entity_type: EntityTypeRef,
        attributes: Vec<AttributeSnapshot>,
    ) -> Arc<Self> {
        Arc::new(Self {
            base: EntityBase::new(id, DVec3::ZERO, entity_type.dimensions, Weak::new()),
            entity_type,
            living_base: LivingEntityBase::new(&vanilla_entities::PIG),
            attributes,
            dirty_attributes: SyncMutex::new(Vec::new()),
            equipment: SyncMutex::new(Vec::new()),
            dirty_equipment: SyncMutex::new(Vec::new()),
            passengers: SyncMutex::new(Vec::new()),
            vehicle: SyncMutex::new(None),
        })
    }

    fn shared(attributes: Vec<AttributeSnapshot>) -> SharedEntity {
        Self::new(1, attributes)
    }

    fn add_passenger(&self, passenger: &SharedEntity) {
        self.passengers.lock().push(Arc::downgrade(passenger));
    }

    fn clear_passengers(&self) {
        self.passengers.lock().clear();
    }

    fn set_vehicle(&self, vehicle: &SharedEntity) {
        *self.vehicle.lock() = Some(Arc::downgrade(vehicle));
    }

    fn set_dirty_attributes(&self, attributes: Vec<AttributeSnapshot>) {
        *self.dirty_attributes.lock() = attributes;
    }

    fn set_equipment(&self, equipment: Vec<EquipmentSlotItem>) {
        *self.equipment.lock() = equipment;
    }

    fn set_dirty_equipment(&self, equipment: Vec<EquipmentSlotItem>) {
        *self.dirty_equipment.lock() = equipment;
    }
}

crate::entity::impl_test_downcast_type!(PairingTestEntity);

impl Entity for PairingTestEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn vehicle(&self) -> Option<SharedEntity> {
        self.vehicle.lock().as_ref().and_then(Weak::upgrade)
    }

    fn passengers(&self) -> Vec<SharedEntity> {
        let mut live_passengers = Vec::new();
        self.passengers.lock().retain(|passenger| {
            let Some(entity) = passenger.upgrade() else {
                return false;
            };
            live_passengers.push(entity);
            true
        });
        live_passengers
    }
}

impl LivingEntity for PairingTestEntity {
    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
    }

    fn get_health(&self) -> f32 {
        20.0
    }

    fn set_health(&self, _health: f32) {}

    fn pack_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
        self.attributes.clone()
    }

    fn drain_dirty_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
        mem::take(&mut *self.dirty_attributes.lock())
    }

    fn pack_all_equipment(&self) -> Vec<EquipmentSlotItem> {
        self.equipment.lock().clone()
    }

    fn drain_dirty_equipment(&self) -> Vec<EquipmentSlotItem> {
        mem::take(&mut *self.dirty_equipment.lock())
    }
}

fn track_entity_for_player(tracker: &EntityTracker, entity: &SharedEntity, player_id: i32) {
    let pos = entity.position();
    let mut seen_by = FxHashSet::default();
    seen_by.insert(player_id);
    let tracked_entity = TrackedEntity {
        entity: Arc::downgrade(entity),
        server_entity: SyncMutex::new(ServerEntityMovementSyncState::new(
            pos,
            entity.velocity(),
            entity.on_ground(),
            entity.rotation(),
            entity.head_yaw(),
            entity.entity_type().update_interval,
            entity.entity_type().track_deltas,
        )),
        last_passenger_ids: SyncMutex::new(tracker.direct_tracked_passenger_ids(entity.as_ref())),
        last_leash_holder_id: SyncMutex::new(leash_holder_id(entity.as_ref())),
        tracking_range: EntityTrackingRange::from_client_chunk_range(
            entity.entity_type().client_tracking_range,
        ),
        registered_chunk: ChunkPos::from_entity_pos(pos),
        seen_by: SyncRwLock::new(seen_by),
    };
    assert!(
        tracker
            .entities
            .insert_sync(entity.id(), tracked_entity)
            .is_ok()
    );
}

fn mark_seen_by_player(tracker: &EntityTracker, entity_id: i32, player_id: i32) {
    tracker.entities.update_sync(&entity_id, |_, tracked| {
        tracked.seen_by.write().insert(player_id);
    });
}

fn assert_has_velocity_packet(
    updates: &[(i32, EntityMovementSyncPacket)],
    entity_id: i32,
    velocity: DVec3,
) {
    let has_packet = updates.iter().any(|(sent_entity_id, packet)| {
        let EntityMovementSyncPacket::Velocity(packet) = packet else {
            return false;
        };
        *sent_entity_id == entity_id
            && packet.entity_id == entity_id
            && packet.vel.x.to_bits() == velocity.x.to_bits()
            && packet.vel.y.to_bits() == velocity.y.to_bits()
            && packet.vel.z.to_bits() == velocity.z.to_bits()
    });
    assert!(
        has_packet,
        "expected velocity packet for entity {entity_id} with velocity {velocity:?}, got {updates:?}"
    );
}

mod packet_sync;
mod passenger_leash;
mod spawn_despawn;
mod visibility_range;
