//! Temporary mounts anchored to blocks. They use vanilla interaction entities
//! on the wire and are never persisted as ordinary interaction entities.

use std::sync::{Arc, Weak};

use glam::DVec3;
use rustc_hash::FxHashMap;
use steel_registry::{
    blocks::{behavior::PushReaction, block_state_ext::BlockStateExt},
    entity_type::{EntityDimensions, EntityTypeRef},
    vanilla_entities,
    vanilla_entity_data::InteractionEntityData,
};
use steel_utils::{
    BlockPos, BlockStateId, ChunkPos, Direction, DowncastType, DowncastTypeKey, locks::SyncMutex,
};

use crate::{
    behavior::blocks::BedBlock,
    entity::{
        Entity, EntityBase, EntitySyncedData, LivingEntity as _, RemovalReason, SharedEntity,
        next_entity_id,
    },
    player::Player,
    world::World,
};

#[derive(Default)]
pub(crate) struct BlockSeats(SyncMutex<FxHashMap<BlockPos, Weak<BlockSeat>>>);

impl BlockSeats {
    pub(crate) fn prune(&self) {
        self.0
            .lock()
            .retain(|_, seat| seat.upgrade().is_some_and(|seat| !seat.is_removed()));
    }
}

pub(crate) struct BlockSeat {
    base: EntityBase,
    data: SyncMutex<InteractionEntityData>,
    anchor: BlockPos,
    state: BlockStateId,
}

// SAFETY: Steel owns this concrete implementation and its unique key.
unsafe impl DowncastType for BlockSeat {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/block_seat");
}

impl BlockSeat {
    /// Chair dismounts use Steel's existing bed stand-up search and its above-block fallback.
    pub(crate) fn stand_up(&self, player: &Player) {
        let Some(world) = self.level() else {
            return;
        };
        if !Arc::ptr_eq(&world, &player.get_world()) || !Entity::is_alive(player) {
            return;
        }
        let pos = BedBlock::find_standup_position(&world, player, Direction::North, self.anchor)
            .unwrap_or_else(|| {
                let above = self.anchor.above();
                DVec3::new(
                    f64::from(above.x()) + 0.5,
                    f64::from(above.y()) + 0.1,
                    f64::from(above.z()) + 0.5,
                )
            });
        if let Err(error) = player.teleport_to(pos) {
            log::warn!("Unable to synchronize chair dismount: {error}");
        }
    }
}

impl Entity for BlockSeat {
    fn base(&self) -> &EntityBase {
        &self.base
    }
    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::INTERACTION
    }
    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.data)
    }
    fn is_persistent(&self) -> bool {
        false
    }
    fn no_physics(&self) -> bool {
        true
    }
    fn is_ignoring_block_triggers(&self) -> bool {
        true
    }
    fn piston_push_reaction(&self) -> PushReaction {
        PushReaction::IgnoreEntity
    }
    fn can_ride(&self, _vehicle: &dyn Entity) -> bool {
        false
    }

    fn tick(self: Arc<Self>) {
        let Some(world) = self.level() else {
            return;
        };
        let supported = world
            .chunk_map
            .with_full_chunk(ChunkPos::from_block_pos(self.anchor), |chunk| {
                chunk.get_block_state(self.anchor) == self.state
            })
            .unwrap_or(false);
        let passengers = self.passengers();
        if supported && !passengers.is_empty() {
            return;
        }
        for passenger in passengers {
            passenger.stop_riding();
        }
        self.set_removed(RemovalReason::Discarded);
    }
}

impl World {
    /// Mounts a player at an extension-authored entity origin relative to a loaded block.
    /// Passenger attachment offsets are resolved through the actual entity dimensions.
    pub fn sit_player(
        self: &Arc<Self>,
        player: &Player,
        anchor: BlockPos,
        offset: DVec3,
    ) -> Result<bool, String> {
        if !offset.is_finite() {
            return Err("seat offset must be finite".into());
        }
        if !Arc::ptr_eq(self, &player.get_world()) {
            return Err("player is in another world".into());
        }
        if player.is_passenger() || player.is_sleeping() || !Entity::is_alive(player) {
            return Ok(false);
        }
        if !player.is_within_block_interaction_range(anchor) {
            return Err("seat is outside interaction reach".into());
        }
        let state = self
            .chunk_map
            .with_full_chunk(ChunkPos::from_block_pos(anchor), |chunk| {
                chunk.get_block_state(anchor)
            })
            .ok_or("seat chunk is not loaded")?;
        if state.is_air() {
            return Err("a seat must be anchored to a block".into());
        }
        // Serialize mount attempts at one block; all invoked entity implementations
        // here are Steel-owned and do not call extension callbacks.
        let mut seats = self.block_seats.0.lock();
        if seats
            .get(&anchor)
            .and_then(Weak::upgrade)
            .is_some_and(|seat| !seat.is_removed())
        {
            return Ok(false);
        }
        let origin = DVec3::new(
            f64::from(anchor.x()),
            f64::from(anchor.y()),
            f64::from(anchor.z()),
        ) + offset;
        let mut data = InteractionEntityData::new();
        data.width.set(0.0);
        data.height.set(0.0);
        data.base.no_gravity.set(true);
        let seat = Arc::new(BlockSeat {
            base: EntityBase::new(
                next_entity_id(),
                origin,
                EntityDimensions::with_default_eye_height(0.0, 0.0),
                Arc::downgrade(self),
            ),
            data: SyncMutex::new(data),
            anchor,
            state,
        });
        seat.base.set_no_gravity(true);
        seat.base.set_invulnerable(true);
        seat.try_set_position(origin + player.vehicle_attachment_point(seat.as_ref()))
            .map_err(|error| error.to_string())?;
        let weak = Arc::downgrade(&seat);
        let entity: SharedEntity = seat;
        self.try_add_entity(Arc::clone(&entity))
            .map_err(|error| error.to_string())?;
        if !player.start_riding(&entity) {
            entity.set_removed(RemovalReason::Discarded);
            return Ok(false);
        }
        seats.insert(anchor, weak);
        Ok(true)
    }
}
