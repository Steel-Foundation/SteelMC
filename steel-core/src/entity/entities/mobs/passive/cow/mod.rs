//! Vanilla Cow entity with variant + sound-variant parity.

pub mod abstract_cow;

use crate::entity::LivingParts as _;
use crate::entity::prelude::*;
use std::sync::{LazyLock, Weak};

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_registry::cow_sound_variant::{CowSoundVariantRef, CowSoundVariantRegistry};
use steel_registry::cow_variant::{CowVariantRef, CowVariantRegistry};
use steel_registry::data_components::vanilla_components::{COW_SOUND_VARIANT, COW_VARIANT};
use steel_registry::entity_type::{
    EntityAttachmentPoint, EntityAttachments, EntityDimensions, EntityTypeRef,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::CowEntityData;
use steel_registry::{REGISTRY, RegistryReference};
use steel_utils::locks::CheckedMutex;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::entity::ageable::{self, Age, AgeableProps};
use crate::entity::animal::{self, Breeding, BreedingProps};
use crate::entity::component::definition::MobDefinition;
use crate::entity::variant::{self, Variants};
use crate::entity::{
    DefaultEntityPush, EntityBase, EntityBaseLoad, EntityProps, LivingEntityBase, LivingProps,
    MobBase, MobProps, SpawnRule,
};
use crate::world::World;

const COW_BABY_PASSENGER_ATTACHMENTS: [EntityAttachmentPoint; 1] =
    [EntityAttachmentPoint::new(0.0, 0.75, 0.0)];

/// `Cow.BABY_DIMENSIONS`.
const COW_BABY_DIMENSIONS: EntityDimensions = EntityDimensions::new_with_attachments(
    0.45,
    0.7,
    0.69,
    EntityAttachments::new(&COW_BABY_PASSENGER_ATTACHMENTS, &[], &[], &[]),
);

/// Vanilla cow entity.
#[entity_behavior(class = "Cow", spawn_rule)]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, save = variant::save, load = variant::load)]
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

// SAFETY: This key is owned by Steel and uniquely identifies `CowEntity`.
unsafe impl DowncastType for CowEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/cow");
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
    const LIVING: LivingProps<Self> = LivingProps {
        default_dimensions: |mob, _| {
            if mob.living().is_baby() {
                COW_BABY_DIMENSIONS
            } else {
                mob.entity_type.dimensions
            }
        },
        hurt_sound: |cow, _| Some(cow.sound_variant().hurt_sound),
        death_sound: |cow| Some(cow.sound_variant().death_sound),
        ..abstract_cow::living_props()
    };
    const MOB: MobProps<Self> = MobProps {
        ambient_sound: |cow| Some(cow.sound_variant().ambient_sound),
        ..abstract_cow::mob_props()
    };
    /// Vanilla `Cow`: the `AbstractCow` chains plus variant selection before
    /// `AgeableMob.finalizeSpawn`.
    fn define() -> MobDefinition<Self> {
        let mut definition = abstract_cow::definition::<Self>();
        definition.finalize_spawn = definition.finalize_spawn.before(
            ageable::FINALIZE_SPAWN,
            [variant::pick_spawn_variants::<Self>()],
        );
        definition
    }

    fn definition() -> &'static MobDefinition<Self> {
        static DEFINITION: LazyLock<MobDefinition<CowEntity>> = LazyLock::new(CowEntity::define);
        &DEFINITION
    }

    /// Creates a new cow at runtime.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::from_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Reconstructs a cow from persisted base entity state.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::from_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let mut entity_data = CowEntityData::new();
        let living_base = LivingEntityBase::new(entity_type, &Self::LIVING, &mut entity_data);
        Self {
            base,
            entity_type,
            living_base,
            mob_base: MobBase::new(&Self::MOB, Self::definition()),
            age: Age::new(&Self::AGEABLE),
            breeding: Breeding::new(&Self::BREEDING),
            entity_data: CheckedMutex::new(entity_data),
        }
    }
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

impl CowEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        apply_implicit_item_components: Self::entity_apply_implicit_item_components,
        play_step_sound: Self::entity_play_step_sound,
        ..abstract_cow::entity_props()
    };

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
