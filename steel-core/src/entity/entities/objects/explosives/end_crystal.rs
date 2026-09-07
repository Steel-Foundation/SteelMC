//! Minimal End Crystal entity implementation for End spike worldgen.

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_damage_type_tags::DamageTypeTag;
use steel_registry::vanilla_entity_data::EndCrystalEntityData;
use steel_registry::{vanilla_damage_types, vanilla_entities};
use steel_utils::{BlockPos, locks::SyncMutex};
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::entity::damage::DamageSource;
use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntitySyncedData, RemovalReason};
use crate::world::World;
use crate::world::explosion::ExplosionInteraction;

/// Radius of the blast a destroyed crystal sets off.
const EXPLOSION_RADIUS: f32 = 6.0;

/// End Crystal entity state needed by worldgen and persistence.
///
/// Steel currently implements the synchronized data and saved fields used by generated
/// End spikes, plus the blast a destroyed crystal sets off. Portal handling and the
/// dragon-fight callbacks are still left to the broader foundations.
#[entity_behavior(class = "EndCrystal")]
pub struct EndCrystalEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<EndCrystalEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `EndCrystalEntity`.
unsafe impl DowncastType for EndCrystalEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/end_crystal");
}

impl EndCrystalEntity {
    /// Creates a new End Crystal entity.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(EndCrystalEntityData::new()),
        }
    }

    /// Creates an End Crystal entity from saved data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(EndCrystalEntityData::new()),
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

    const fn nbt_bool(value: bool) -> i8 {
        if value { 1 } else { 0 }
    }
}

impl Entity for EndCrystalEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(&self) {
        // TODO: Implement portal handling and fire refresh.
    }

    /// Mirrors vanilla `EndCrystal.hurtServer`.
    ///
    /// The crystal has no health: any blow that lands at all destroys it outright and
    /// sets off a blast, which is why the damage amount is ignored.
    fn hurt(&self, world: &World, source: &DamageSource, _amount: f32) -> bool {
        if self.is_invulnerable_to_base(source) {
            return false;
        }

        let attacker = source
            .causing_entity_id
            .and_then(|id| world.get_entity_by_id(id));
        // Vanilla tests `instanceof EnderDragon`. That type lives on another branch, so
        // the registry entry identifies it instead; either way a dragon cannot break the
        // crystals that heal it.
        if attacker
            .as_ref()
            .is_some_and(|entity| entity.entity_type() == &vanilla_entities::ENDER_DRAGON)
        {
            return false;
        }

        if self.is_removed() {
            return true;
        }

        let Some(world) = self.level() else {
            return true;
        };
        // Taken before removal, because the blast reports the crystal as its source.
        let crystal = world.get_entity_by_id(self.id());
        self.set_removed(RemovalReason::Killed);

        // A crystal destroyed by another blast is removed without detonating, which is
        // what stops a ring of them chaining endlessly.
        if !source.is(&DamageTypeTag::IS_EXPLOSION) {
            let damage_source = attacker.map(|attacker| {
                DamageSource::environment(&vanilla_damage_types::EXPLOSION)
                    .with_direct_entity(self.id())
                    .with_causing_entity(attacker.id())
                    .with_source_position(self.position())
            });
            world.explode(
                crystal,
                damage_source,
                None,
                self.position(),
                EXPLOSION_RADIUS,
                false,
                ExplosionInteraction::Block,
            );
        }

        // TODO: Report to `EnderDragonFight::on_crystal_destroyed` once the fight exists.
        true
    }

    fn is_pickable(&self) -> bool {
        true
    }

    fn blocks_building(&self) -> bool {
        true
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        if let Some(target) = self.beam_target() {
            nbt.insert(
                "beam_target",
                NbtTag::IntArray(vec![target.x(), target.y(), target.z()]),
            );
        }

        nbt.insert("ShowBottom", Self::nbt_bool(self.shows_bottom()));
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        if let Some(target) = nbt.int_array("beam_target")
            && target.len() == 3
        {
            self.set_beam_target(Some(BlockPos::new(target[0], target[1], target[2])));
        }

        if let Some(show_bottom) = nbt.byte("ShowBottom") {
            self.set_show_bottom(show_bottom != 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use steel_registry::vanilla_entities;

    #[test]
    fn end_crystal_does_not_duplicate_shared_invulnerable_state() {
        let crystal = EndCrystalEntity::new(
            &vanilla_entities::END_CRYSTAL,
            1,
            DVec3::new(1.5, 2.5, 3.5),
            Weak::new(),
        );
        crystal.set_invulnerable(true);

        let mut nbt = NbtCompound::new();
        crystal.save_additional(&mut nbt);

        assert_eq!(nbt.byte("Invulnerable"), None);
    }

    #[test]
    fn end_crystal_is_pickable_like_vanilla() {
        let crystal = EndCrystalEntity::new(
            &vanilla_entities::END_CRYSTAL,
            1,
            DVec3::new(1.5, 2.5, 3.5),
            Weak::new(),
        );

        assert!(crystal.is_pickable());
    }

    #[test]
    fn end_crystal_blocks_building_like_vanilla() {
        let crystal =
            EndCrystalEntity::new(&vanilla_entities::END_CRYSTAL, 1, DVec3::ZERO, Weak::new());

        assert!(crystal.blocks_building());
    }
}
