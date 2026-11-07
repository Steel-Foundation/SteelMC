//! Vanilla `EndCrystal`, as far as End spike worldgen and saving need it.

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entity_data::EndCrystalEntityData;
use steel_utils::locks::CheckedMutex;
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey};

use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntityMovementEmission, EntityProps};
use crate::world::World;

/// An End crystal.
// TODO: the fire it keeps lit in the End, exploding when hurt, and the dragon
// fight callbacks.
#[entity_behavior(class = "EndCrystal")]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, save = Self::save_crystal, load = Self::load_crystal)]
pub struct EndCrystalEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(synced)]
    entity_data: CheckedMutex<EndCrystalEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `EndCrystalEntity`.
unsafe impl DowncastType for EndCrystalEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/end_crystal");
}

impl EndCrystalEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        tick: |_, _| {},
        movement_emission: |_| EntityMovementEmission::None,
        is_pickable: |_| true,
        blocks_building: |_| true,
        ..EntityProps::new()
    };

    /// Creates a new End crystal.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::from_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Reconstructs an End crystal from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::from_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        Self {
            base,
            entity_type,
            entity_data: CheckedMutex::new(EndCrystalEntityData::new()),
        }
    }

    /// The block its beam points at.
    #[must_use]
    pub fn beam_target(&self) -> Option<BlockPos> {
        *self.entity_data.lock().beam_target.get()
    }

    /// Sets the block its beam points at.
    pub fn set_beam_target(&self, target: Option<BlockPos>) {
        self.entity_data.lock().beam_target.set(target);
    }

    /// Whether the bedrock base renders.
    #[must_use]
    pub fn shows_bottom(&self) -> bool {
        *self.entity_data.lock().show_bottom.get()
    }

    /// Sets whether the bedrock base renders.
    pub fn set_show_bottom(&self, show_bottom: bool) {
        self.entity_data.lock().show_bottom.set(show_bottom);
    }

    fn save_crystal(&self, nbt: &mut NbtCompound) {
        if let Some(target) = self.beam_target() {
            nbt.insert(
                "beam_target",
                NbtTag::IntArray(vec![target.x(), target.y(), target.z()]),
            );
        }
        nbt.insert("ShowBottom", i8::from(self.shows_bottom()));
    }

    fn load_crystal(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        if let Some(target) = nbt.int_array("beam_target")
            && let [x, y, z] = target[..]
        {
            self.set_beam_target(Some(BlockPos::new(x, y, z)));
        }
        if let Some(show_bottom) = nbt.byte("ShowBottom") {
            self.set_show_bottom(show_bottom != 0);
        }
    }
}
