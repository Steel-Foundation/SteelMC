//! Vanilla Cow entity with variant + sound-variant parity.

pub mod abstract_cow;

use crate::entity::LivingTable;
use crate::entity::MobTable;
use crate::entity::ageable;
use crate::entity::prelude::*;

use steel_macros::entity_behavior;
use steel_registry::cow_sound_variant::{CowSoundVariantRef, CowSoundVariantRegistry};
use steel_registry::cow_variant::{CowVariantRef, CowVariantRegistry};
use steel_registry::data_components::vanilla_components::{COW_SOUND_VARIANT, COW_VARIANT};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::CowEntityData;
use steel_registry::{REGISTRY, RegistryReference};
use steel_utils::locks::CheckedMutex;
use steel_utils::{BlockPos, BlockStateId};

use crate::entity::EntityTable;
use crate::entity::ageable::{Age, AgeableProps};
use crate::entity::animal::{self, Breeding, BreedingProps};
use crate::entity::variant::{self, Variants};
use crate::entity::{
    DefaultEntityPush, EntityBase, EntityProps, LivingEntityBase, LivingProps, MobBase, MobProps,
    SpawnRule,
};

/// Vanilla cow entity.
#[entity_behavior(class = "Cow", spawn_rule)]
#[derive(Entity)]
#[entity(save = variant::save, load = variant::load)]
#[entity(key = "steel:entity/cow", constructors)]
pub struct CowEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    living_base: LivingEntityBase,
    #[entity(component)]
    mob_base: MobBase,
    #[entity(component)]
    age: Age,
    #[entity(component)]
    breeding: Breeding,
    #[entity(synced)]
    entity_data: CheckedMutex<CowEntityData>,
}

impl CowEntity {
    /// Vanilla `SpawnPlacements` predicate: `Animal.checkAnimalSpawnRules`.
    pub(crate) const SPAWN_RULE: SpawnRule = |entity_type, level, spawn_reason, pos, random| {
        animal::check_animal_spawn_rules(entity_type, level, spawn_reason, pos, random)
    };

    const AGEABLE: AgeableProps = AgeableProps {
        breed_offspring: variant::breed_offspring::<Self>,
    };
    const BREEDING: BreedingProps = BreedingProps::new(abstract_cow::is_food);

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let mut entity_data = CowEntityData::new();
        let living_base = LivingEntityBase::new(entity_type, &mut entity_data);
        Self {
            base,
            entity_type,
            living_base,
            mob_base: MobBase::new(abstract_cow::construct),
            age: Age::new(&Self::AGEABLE),
            breeding: Breeding::new(&Self::BREEDING),
            entity_data: CheckedMutex::new(entity_data),
        }
    }
}

impl MobTable for CowEntity {
    const MOB: MobProps<Self> = MobProps {
        ambient_sound: |cow| Some(cow.sound_variant().ambient_sound),
        // `Cow.finalizeSpawn` picks the variants before `super`.
        finalize_spawn: |cow, world, reason, group_data| {
            variant::pick_spawn_variants(cow, world);
            ageable::finalize_spawn(cow, world, reason, group_data)
        },
        ..abstract_cow::mob_props()
    };
}

impl LivingTable for CowEntity {
    const LIVING: LivingProps<Self> = LivingProps {
        hurt_sound: |cow, _| Some(cow.sound_variant().hurt_sound),
        death_sound: |cow| Some(cow.sound_variant().death_sound),
        ..abstract_cow::living_props()
    };
}

impl Variants for CowEntity {
    type VariantRegistry = CowVariantRegistry;
    type SoundVariantRegistry = CowSoundVariantRegistry;

    fn variant_registry() -> &'static CowVariantRegistry {
        &REGISTRY.cow_variants
    }

    fn sound_variant_registry() -> &'static CowSoundVariantRegistry {
        &REGISTRY.cow_sound_variants
    }

    fn variant(&self) -> CowVariantRef {
        self.entity_data.lock().variant.get().value()
    }

    fn set_variant(&self, variant: CowVariantRef) {
        self.entity_data
            .lock()
            .variant
            .set(RegistryReference::new(variant));
    }

    /// Vanilla `Cow.getSoundSet`.
    fn sound_variant(&self) -> CowSoundVariantRef {
        self.entity_data.lock().sound_variant.get().value()
    }

    fn set_sound_variant(&self, sound_variant: CowSoundVariantRef) {
        self.entity_data
            .lock()
            .sound_variant
            .set(RegistryReference::new(sound_variant));
    }
}

impl DefaultEntityPush for CowEntity {}

impl EntityTable for CowEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        apply_implicit_item_components: Self::entity_apply_implicit_item_components,
        play_step_sound: Self::entity_play_step_sound,
        ..abstract_cow::entity_props()
    };
}

impl CowEntity {
    fn entity_apply_implicit_item_components(&self, item_stack: &ItemStack) {
        if let Some(variant) = item_stack.get(COW_VARIANT) {
            self.set_variant(variant.value());
        }
        if let Some(sound_variant) = item_stack.get(COW_SOUND_VARIANT) {
            self.set_sound_variant(sound_variant.value());
        }
    }

    fn entity_play_step_sound(&self, _pos: BlockPos, _block_state: BlockStateId) {
        self.play_sound(self.sound_variant().step_sound, 0.15, 1.0);
    }
}

#[cfg(test)]
mod tests;
