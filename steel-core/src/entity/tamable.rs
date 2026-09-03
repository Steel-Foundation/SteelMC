//! Shared vanilla `TamableAnimal` state and hooks.

use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_utils::UuidExt;
use steel_utils::entity_events::EntityStatus;
use uuid::Uuid;

use crate::entity::{Animal, Entity};
use crate::player::Player;

/// Vanilla `TamableAnimal.DATA_FLAGS_ID` sitting bit.
const FLAG_SITTING: i8 = 0x01;
/// Vanilla `TamableAnimal.DATA_FLAGS_ID` tamed bit.
const FLAG_TAME: i8 = 0x04;

/// Vanilla-shaped behavior shared by entities that extend `TamableAnimal`.
pub trait TamableAnimal: Animal {
    /// Returns vanilla `TamableAnimal.DATA_FLAGS_ID`.
    fn tamable_flags(&self) -> i8;

    /// Sets vanilla `TamableAnimal.DATA_FLAGS_ID`.
    fn set_tamable_flags(&self, flags: i8);

    /// Returns vanilla `TamableAnimal.getOwnerUUID`.
    fn owner_uuid(&self) -> Option<Uuid>;

    /// Sets vanilla `TamableAnimal.setOwnerUUID`.
    fn set_owner_uuid(&self, owner: Option<Uuid>);

    /// Returns vanilla `TamableAnimal.isTame`.
    fn is_tame(&self) -> bool {
        self.tamable_flags() & FLAG_TAME != 0
    }

    /// Sets vanilla `TamableAnimal.setTame` without extra taming side effects.
    fn set_tame(&self, tame: bool) {
        let mut flags = self.tamable_flags();
        if tame {
            flags |= FLAG_TAME;
        } else {
            flags &= !FLAG_TAME;
        }
        self.set_tamable_flags(flags);
    }

    /// Returns vanilla `TamableAnimal.isOrderedToSit`.
    fn is_ordered_to_sit(&self) -> bool {
        self.tamable_flags() & FLAG_SITTING != 0
    }

    /// Sets vanilla `TamableAnimal.setOrderedToSit`.
    fn set_ordered_to_sit(&self, sitting: bool) {
        let mut flags = self.tamable_flags();
        if sitting {
            flags |= FLAG_SITTING;
        } else {
            flags &= !FLAG_SITTING;
        }
        self.set_tamable_flags(flags);
    }

    /// Returns whether `player` is this animal's owner.
    fn is_owned_by(&self, player: &Player) -> bool {
        self.owner_uuid() == Some(player.uuid())
    }

    /// Vanilla `TamableAnimal.tame`: mark tamed, assign owner, persist, hearts.
    fn tame(&self, player: &Player) {
        self.set_tame(true);
        self.set_owner_uuid(Some(player.uuid()));
        self.set_persistence_required();
        self.broadcast_entity_event(EntityStatus::TamingSucceeded);
    }

    /// Saves vanilla tamable fields (`Owner`, `Sitting`).
    fn save_tamable(&self, nbt: &mut NbtCompound) {
        nbt.insert("Sitting", i8::from(self.is_ordered_to_sit()));
        if let Some(owner) = self.owner_uuid() {
            nbt.insert("Owner", NbtTag::IntArray(owner.to_int_array().to_vec()));
        }
    }

    /// Loads vanilla tamable fields. A persisted owner implies the animal is tamed.
    fn load_tamable(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.set_ordered_to_sit(nbt.byte("Sitting").is_some_and(|value| value != 0));
        if let Some(owner) = nbt
            .int_array("Owner")
            .and_then(|arr| Uuid::from_int_array(&arr))
        {
            self.set_owner_uuid(Some(owner));
            self.set_tame(true);
        }
    }
}
