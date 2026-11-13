//! Vanilla `EndCrystal`, as far as End spike worldgen and saving need it.

use std::sync::Arc;

use crate::entity::prelude::*;
use steel_utils::nbt::NbtValueInput as _;

use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_macros::entity_behavior;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_damage_type_tags::DamageTypeTag;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::EndCrystalEntityData;
use steel_utils::BlockPos;
use steel_utils::locks::CheckedMutex;

use crate::entity::EntityTable;
use crate::entity::damage::DamageSource;
use crate::entity::{Entity, EntityBase, EntityMovementEmission, EntityProps, RemovalReason};
use crate::world::explosion::{
    DefaultExplosionDamageCalculator, ExplosionInteraction, ExplosionOptions,
};

const EXPLOSION_RADIUS: f32 = 6.0;

/// An End crystal.
// TODO: the fire it keeps lit in the End and the dragon fight callbacks.
#[entity_behavior(class = "EndCrystal")]
#[derive(Entity)]
#[entity(save = Self::save_crystal, load = Self::load_crystal)]
#[entity(key = "steel:entity/end_crystal", constructors)]
pub struct EndCrystalEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(synced)]
    entity_data: CheckedMutex<EndCrystalEntityData>,
}

impl EntityTable for EndCrystalEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        tick: Self::entity_tick,
        hurt: |crystal, _, source, _| crystal.hurt_crystal(source),
        movement_emission: |_| EntityMovementEmission::None,
        is_pickable: |_| true,
        blocks_building: |_| true,
        ..EntityProps::new()
    };
}

impl EndCrystalEntity {
    /// `EndCrystal.tick` never calls `super`.
    fn entity_tick(&self) {
        self.apply_effects_from_blocks();
        self.handle_portal();
    }
}

impl EndCrystalEntity {
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

    /// Any accepted hit destroys the crystal, so the damage amount is irrelevant.
    fn hurt_crystal(&self, source: &DamageSource) -> bool {
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
            // The crystal keeps the default block-explosion properties.
            options.immutable_block_calculator = Some(&DefaultExplosionDamageCalculator);
            world.explode(options);
        }

        // TODO: Notify the dragon fight (`EndCrystal.onDestroyedBy`) here and from `kill`
        // once the End dragon fight exists.
        true
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
        if let Some(show_bottom) = nbt.byte_value("ShowBottom") {
            self.set_show_bottom(show_bottom != 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Weak;

    use glam::DVec3;

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
        let crystal = EndCrystalEntity::new(
            &vanilla_entities::END_CRYSTAL,
            next_entity_id(),
            DVec3::new(0.5, 64.0, 0.5),
            Arc::downgrade(world),
        );
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
