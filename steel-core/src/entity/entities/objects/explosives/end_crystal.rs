//! Vanilla `EndCrystal`, as far as End spike worldgen and saving need it.

use crate::entity::component::definition::Definition;
use crate::entity::component::pipeline::{Stage, StageId};
use crate::entity::prelude::*;

use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_entity_data::EndCrystalEntityData;
use steel_utils::BlockPos;
use steel_utils::locks::CheckedMutex;

use crate::entity::{Entity, EntityBase, EntityMovementEmission, EntityProps};

/// An End crystal.
// TODO: the fire it keeps lit in the End, exploding when hurt, and the dragon
// fight callbacks.
#[entity_behavior(class = "EndCrystal")]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, save = Self::save_crystal, load = Self::load_crystal)]
#[entity(key = "steel:entity/end_crystal", constructors, definition = Self::define)]
pub struct EndCrystalEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(synced)]
    entity_data: CheckedMutex<EndCrystalEntityData>,
}

impl EndCrystalEntity {
    const TICK: StageId = StageId("end_crystal.tick");

    /// `EndCrystal.tick` never calls `super`.
    fn define() -> Definition<Self> {
        Definition::replacing_tick([Stage::effect(Self::TICK, |crystal: &Self| {
            crystal.apply_effects_from_blocks();
            crystal.handle_portal();
        })])
    }

    const ENTITY: EntityProps<Self> = EntityProps {
        movement_emission: |_| EntityMovementEmission::None,
        is_pickable: |_| true,
        blocks_building: |_| true,
        ..EntityProps::new()
    };

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
