//! Minimal End Crystal entity implementation for End spike worldgen.

use std::sync::Arc;
use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_damage_type_tags::DamageTypeTag;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::EndCrystalEntityData;
use steel_utils::nbt::NbtValueInput as _;
use steel_utils::{BlockPos, locks::SyncMutex};
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityMovementEmission, EntitySyncedData, RemovalReason,
};
use crate::world::World;
use crate::world::explosion::{
    DefaultExplosionDamageCalculator, ExplosionInteraction, ExplosionOptions,
};

const EXPLOSION_RADIUS: f32 = 6.0;

/// End Crystal entity state needed by worldgen and persistence.
///
/// Steel currently implements the synchronized data and saved fields used by generated
/// End spikes, plus the explosion when destroyed. Portal handling and dragon fight
/// callbacks are still intentionally left to the broader entity/combat foundations.
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

    fn movement_emission(&self) -> EntityMovementEmission {
        EntityMovementEmission::None
    }

    fn tick(self: Arc<Self>) {
        // TODO: Implement portal handling and fire refresh.
    }

    /// Vanilla `EndCrystal.hurtServer`: any accepted hit destroys the crystal, so the
    /// damage amount is irrelevant.
    fn hurt(&self, _world: &World, source: &DamageSource, _amount: f32) -> bool {
        if self.is_invulnerable_to_base(source) {
            return false;
        }

        // Vanilla checks `instanceof EnderDragon`, which only the ender dragon type is.
        let attacker = source.causing_entity();
        if attacker.is_some_and(|entity| entity.entity_type() == &vanilla_entities::ENDER_DRAGON) {
            return false;
        }

        if self.is_removed() {
            return true;
        }

        let Some(world) = self.level() else {
            return true;
        };
        // Resolved before removal so the explosion can still credit the crystal.
        let crystal = world.get_entity_by_id(self.id());
        self.set_removed(RemovalReason::Killed);

        // A crystal destroyed by another explosion does not chain-detonate.
        if !source.is(&DamageTypeTag::IS_EXPLOSION) {
            let mut options = ExplosionOptions::new(
                self.position(),
                EXPLOSION_RADIUS,
                ExplosionInteraction::Block,
            );
            options.damage_source = attacker.map(|attacker| {
                DamageSource::explosion(crystal.clone(), Some(Arc::clone(attacker)))
            });
            options.source = crystal;
            // The crystal keeps the default block-explosion hooks.
            options.immutable_block_calculator = Some(&DefaultExplosionDamageCalculator);
            world.explode(options);
        }

        // TODO: Notify the dragon fight (`EndCrystal.onDestroyedBy`) here and from `kill`
        // once the End dragon fight exists.
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

        if let Some(show_bottom) = nbt.byte_value("ShowBottom") {
            self.set_show_bottom(show_bottom != 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use steel_registry::blocks::block_state_ext::BlockStateExt as _;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_damage_types};
    use steel_utils::ChunkPos;
    use steel_utils::types::UpdateFlags;

    use crate::behavior::init_behaviors;
    use crate::entity::next_entity_id;
    use crate::test_support::{TestWorld, fresh_test_world, insert_ready_full_chunk};

    const FLOOR: BlockPos = BlockPos::new(0, 63, 0);

    /// A crystal standing on a stone block, like one placed on obsidian.
    fn crystal_on_stone(key: &'static str) -> (TestWorld, Arc<EndCrystalEntity>) {
        init_vanilla_registry();
        init_behaviors();
        let fixture = fresh_test_world(key);
        let world = &fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        assert!(world.set_block(
            FLOOR,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        let crystal = Arc::new(EndCrystalEntity::new(
            &vanilla_entities::END_CRYSTAL,
            next_entity_id(),
            DVec3::new(0.5, 64.0, 0.5),
            Arc::downgrade(world),
        ));
        let Ok(()) = world.try_add_entity(Arc::<EndCrystalEntity>::clone(&crystal)) else {
            panic!("crystal must be added to its loaded chunk");
        };
        (fixture, crystal)
    }

    #[test]
    fn struck_end_crystal_is_destroyed_and_explodes() {
        let (fixture, crystal) = crystal_on_stone("end_crystal_struck");

        assert!(crystal.hurt(
            &fixture.world,
            &DamageSource::environment(&vanilla_damage_types::GENERIC),
            1.0,
        ));

        assert!(crystal.is_removed());
        assert!(fixture.world.get_block_state(FLOOR).is_air());
    }

    #[test]
    fn end_crystal_destroyed_by_an_explosion_does_not_chain() {
        let (fixture, crystal) = crystal_on_stone("end_crystal_no_chain");

        assert!(crystal.hurt(&fixture.world, &DamageSource::explosion(None, None), 1.0,));

        assert!(crystal.is_removed());
        assert_eq!(
            fixture.world.get_block_state(FLOOR).get_block(),
            &vanilla_blocks::STONE
        );
    }

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
