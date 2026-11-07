//! Vanilla Cow entity with variant + sound-variant parity.

pub mod abstract_cow;

use crate::entity::LivingParts as _;
use crate::entity::prelude::*;
use std::str::FromStr;
use std::sync::{Arc, LazyLock, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::cow_sound_variant::CowSoundVariantRef;
use steel_registry::cow_variant::CowVariantRef;
use steel_registry::data_components::vanilla_components::{COW_SOUND_VARIANT, COW_VARIANT};
use steel_registry::entity_type::{
    EntityAttachmentPoint, EntityAttachments, EntityDimensions, EntityTypeRef,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::CowEntityData;
use steel_registry::{REGISTRY, RegistryExt, RegistryReference};
use steel_utils::locks::CheckedMutex;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{
    BlockPos, BlockStateId, Downcast as _, DowncastType, DowncastTypeKey, Identifier,
};

use crate::entity::ageable::{self, Age, Ageable, AgeableProps};
use crate::entity::animal::{self, Breeding, BreedingProps, Parts as _};
use crate::entity::component::definition::MobDefinition;
use crate::entity::component::entries::FinalizeSpawnCtx;
use crate::entity::component::pipeline::{Flow, Stage, StageId};
use crate::entity::{
    DefaultEntityPush, EntityBase, EntityBaseLoad, EntityPose, EntityProps, LivingEntityBase,
    LivingProps, MobBase, MobProps, SharedEntity, SpawnRule, mob,
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
#[entity(hooks = Self::ENTITY, save = Self::save_cow, load = Self::load_cow)]
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
        breed_offspring: Self::breed_offspring,
    };
    const BREEDING: BreedingProps = BreedingProps::new(abstract_cow::is_food);
    const LIVING: LivingProps<Self> = LivingProps {
        hurt_sound: |cow, _| Some(cow.sound_variant().hurt_sound),
        death_sound: |cow| Some(cow.sound_variant().death_sound),
        ..abstract_cow::living_props()
    };
    const MOB: MobProps<Self> = MobProps {
        ambient_sound: |cow| Some(cow.sound_variant().ambient_sound),
        ..animal::mob_props()
    };
    const PICK_VARIANTS: StageId = StageId("cow.pick_variants");

    /// Vanilla `Cow`: the `AbstractCow` chains plus variant selection before
    /// `AgeableMob.finalizeSpawn`.
    fn define() -> MobDefinition<Self> {
        let mut definition = abstract_cow::definition::<Self>();
        definition.finalize_spawn = definition.finalize_spawn.before(
            ageable::FINALIZE_SPAWN,
            [Stage::new(Self::PICK_VARIANTS, Self::pick_spawn_variants)],
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

    /// Sets the active cow variant by registry entry.
    pub fn set_variant(&self, variant: CowVariantRef) {
        self.entity_data
            .lock()
            .variant
            .set(RegistryReference::new(variant));
    }

    /// Returns the active cow variant, falling back to temperate when invalid.
    #[must_use]
    pub fn variant(&self) -> CowVariantRef {
        self.entity_data.lock().variant.get().value()
    }

    /// Sets the active cow sound variant by registry entry.
    pub fn set_sound_variant(&self, sound_variant: CowSoundVariantRef) {
        self.entity_data
            .lock()
            .sound_variant
            .set(RegistryReference::new(sound_variant));
    }

    /// Returns the active cow sound variant (vanilla `Cow.getSoundSet`), falling
    /// back to classic when invalid.
    #[must_use]
    pub fn sound_variant(&self) -> CowSoundVariantRef {
        self.entity_data.lock().sound_variant.get().value()
    }

    fn set_variant_by_key(&self, key: &Identifier) {
        if let Some(variant) = REGISTRY.cow_variants.by_key(key) {
            self.set_variant(variant);
        }
    }

    fn set_sound_variant_by_key(&self, key: &Identifier) {
        if let Some(sound_variant) = REGISTRY.cow_sound_variants.by_key(key) {
            self.set_sound_variant(sound_variant);
        }
    }

    /// `Cow.getBreedOffspring`: a baby cow with either parent's variant.
    fn breed_offspring(
        parent: &Ageable<'_>,
        world: &Arc<World>,
        partner: &Ageable<'_>,
    ) -> Option<SharedEntity> {
        let offspring = parent.create_offspring(world)?;
        let inherited = if rand::random::<bool>() {
            parent
        } else {
            partner
        };
        if let (Some(source), Some(cow)) = (
            inherited.entity().downcast_ref::<Self>(),
            offspring.downcast_ref::<Self>(),
        ) {
            cow.set_variant(source.variant());
        }
        Some(offspring)
    }

    /// `Cow.finalizeSpawn` before `super`: biome variant and random sound variant.
    fn pick_spawn_variants(&self, ctx: &mut FinalizeSpawnCtx<'_>) -> Flow {
        let biome = ctx.world.biome_at(self.block_position());
        let (variant, sound_variant) = {
            let mut random = LegacyRandom::from_seed(rand::random());
            let variant = biome.and_then(|biome| {
                REGISTRY
                    .cow_variants
                    .select_spawn_variant(biome, &mut random)
            });
            let sound_variant = REGISTRY.cow_sound_variants.pick_random(&mut random);
            (variant, sound_variant)
        };
        if let Some(variant) = variant {
            self.set_variant(variant);
        }
        if let Some(sound_variant) = sound_variant {
            self.set_sound_variant(sound_variant);
        }
        Flow::Continue
    }
}

impl DefaultEntityPush for CowEntity {}

impl CowEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        is_hard_collision_relevant: |_| false,
        apply_implicit_item_components: Self::entity_apply_implicit_item_components,
        dimensions_for_pose: Self::entity_dimensions_for_pose,
        play_step_sound: Self::entity_play_step_sound,
        ..mob::entity_props()
    };

    fn entity_apply_implicit_item_components(&self, item_stack: &ItemStack) {
        if let Some(variant) = item_stack.get(COW_VARIANT) {
            self.set_variant(variant.value());
        }
        if let Some(sound_variant) = item_stack.get(COW_SOUND_VARIANT) {
            self.set_sound_variant(sound_variant.value());
        }
    }

    fn entity_dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let scale = self.living().scale();
        if self.animal().ageable().is_baby() {
            COW_BABY_DIMENSIONS.scale(scale)
        } else if self.entity_type.fixed {
            self.entity_type.dimensions
        } else {
            self.entity_type.dimensions.scale(scale)
        }
    }

    fn entity_play_step_sound(&self, _pos: BlockPos, _block_state: BlockStateId) {
        self.play_sound(self.sound_variant().step_sound, 0.15, 1.0);
    }

    fn save_cow(&self, nbt: &mut NbtCompound) {
        nbt.insert("variant", self.variant().key.to_string());
        nbt.insert("sound_variant", self.sound_variant().key.to_string());
    }

    fn load_cow(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        if let Some(variant) = nbt.string("variant")
            && let Ok(key) = Identifier::from_str(variant.to_str().as_ref())
        {
            self.set_variant_by_key(&key);
        }
        if let Some(sound_variant) = nbt.string("sound_variant")
            && let Ok(key) = Identifier::from_str(sound_variant.to_str().as_ref())
        {
            self.set_sound_variant_by_key(&key);
        }
    }
}

#[cfg(test)]
mod tests;
