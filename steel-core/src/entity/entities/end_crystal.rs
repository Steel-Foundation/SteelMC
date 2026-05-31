//! Minimal End Crystal entity implementation for End spike worldgen.

use std::sync::Weak;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam::atomic::AtomicCell;
use glam::DVec3;
use simdnbt::borrow::{BaseNbtCompound as BorrowedNbtCompound, NbtCompound as NbtCompoundView};
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_registry::blocks::shapes::AABBd;
use steel_registry::entity_data::DataValue;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::EndCrystalEntityData;
use steel_utils::{BlockPos, locks::SyncMutex};
use uuid::Uuid;

use crate::entity::{Entity, EntityBase};
use crate::world::World;

/// End Crystal entity state needed by worldgen and persistence.
///
/// Steel currently implements the synchronized data and saved fields used by generated
/// End spikes. Portal handling, dragon fight callbacks, and explosion behavior are still
/// intentionally left to the broader entity/combat foundations.
pub struct EndCrystalEntity {
    base: EntityBase,
    rotation: AtomicCell<(f32, f32)>,
    entity_data: SyncMutex<EndCrystalEntityData>,
    invulnerable: AtomicBool,
}

impl EndCrystalEntity {
    /// Creates a new End Crystal entity.
    #[must_use]
    pub fn new(id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, world),
            rotation: AtomicCell::new((0.0, 0.0)),
            entity_data: SyncMutex::new(EndCrystalEntityData::new()),
            invulnerable: AtomicBool::new(false),
        }
    }

    /// Creates an End Crystal entity from saved data.
    #[must_use]
    pub fn from_saved(
        id: i32,
        position: DVec3,
        uuid: Uuid,
        rotation: (f32, f32),
        world: Weak<World>,
    ) -> Self {
        Self {
            base: EntityBase::with_uuid(id, uuid, position, world),
            rotation: AtomicCell::new(rotation),
            entity_data: SyncMutex::new(EndCrystalEntityData::new()),
            invulnerable: AtomicBool::new(false),
        }
    }

    /// Sets the optional beam target.
    pub fn set_beam_target(&self, target: Option<BlockPos>) {
        self.entity_data.lock().beam_target.set(target);
    }

    /// Returns the optional beam target.
    #[must_use]
    pub fn beam_target(&self) -> Option<BlockPos> {
        *self.entity_data.lock().beam_target.get()
    }

    /// Sets whether the crystal renders its bedrock base.
    pub fn set_show_bottom(&self, show_bottom: bool) {
        self.entity_data.lock().show_bottom.set(show_bottom);
    }

    /// Returns whether the crystal renders its bedrock base.
    #[must_use]
    pub fn shows_bottom(&self) -> bool {
        *self.entity_data.lock().show_bottom.get()
    }

    /// Sets the vanilla invulnerable flag.
    pub fn set_invulnerable(&self, invulnerable: bool) {
        self.invulnerable.store(invulnerable, Ordering::Relaxed);
    }

    /// Returns the vanilla invulnerable flag.
    #[must_use]
    pub fn is_invulnerable(&self) -> bool {
        self.invulnerable.load(Ordering::Relaxed)
    }

    /// Sets position and rotation, matching vanilla `Entity.snapTo`.
    pub fn snap_to(&self, position: DVec3, yaw: f32, pitch: f32) {
        self.set_position(position);
        self.rotation.store((yaw, pitch));
    }

    const fn nbt_bool(value: bool) -> i8 {
        if value { 1 } else { 0 }
    }
}

impl Entity for EndCrystalEntity {
    fn base(&self) -> Option<&EntityBase> {
        Some(&self.base)
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::END_CRYSTAL
    }

    fn bounding_box(&self) -> AABBd {
        let pos = self.position();
        let dims = self.entity_type().dimensions;
        let half_width = f64::from(dims.width) / 2.0;
        let height = f64::from(dims.height);
        AABBd {
            min_x: pos.x - half_width,
            min_y: pos.y,
            min_z: pos.z - half_width,
            max_x: pos.x + half_width,
            max_y: pos.y + height,
            max_z: pos.z + half_width,
        }
    }

    fn tick(&self) {
        // TODO: Implement portal handling, fire refresh, dragon fight callbacks, and explosion behavior.
    }

    fn pack_dirty_entity_data(&self) -> Option<Vec<DataValue>> {
        self.entity_data.lock().pack_dirty()
    }

    fn pack_all_entity_data(&self) -> Vec<DataValue> {
        self.entity_data.lock().pack_all()
    }

    fn rotation(&self) -> (f32, f32) {
        self.rotation.load()
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        if let Some(target) = self.beam_target() {
            nbt.insert(
                "beam_target",
                NbtTag::IntArray(vec![target.x(), target.y(), target.z()]),
            );
        }

        nbt.insert("ShowBottom", Self::nbt_bool(self.shows_bottom()));
        // TODO: Move `Invulnerable` into shared entity save data once `EntityBase` owns it.
        nbt.insert("Invulnerable", Self::nbt_bool(self.is_invulnerable()));
    }

    fn load_additional(&self, nbt: &BorrowedNbtCompound<'_>) {
        let nbt: NbtCompoundView<'_, '_> = nbt.into();

        if let Some(target) = nbt.int_array("beam_target")
            && target.len() == 3
        {
            self.set_beam_target(Some(BlockPos::new(target[0], target[1], target[2])));
        }

        if let Some(show_bottom) = nbt.byte("ShowBottom") {
            self.set_show_bottom(show_bottom != 0);
        }

        if let Some(invulnerable) = nbt.byte("Invulnerable") {
            self.set_invulnerable(invulnerable != 0);
        }
    }
}
