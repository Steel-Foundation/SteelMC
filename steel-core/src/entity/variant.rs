//! Mobs with a spawn-conditioned variant and a random sound variant (chickens,
//! cows, pigs): vanilla's `VariantUtils` plus the `finalizeSpawn` and
//! `getBreedOffspring` code those mobs share.

use std::str::FromStr;
use std::sync::Arc;

use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_registry::{RegistryEntry, RegistryExt, SoundVariantRegistry, SpawnVariantRegistry};
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{Downcast as _, DowncastType, Identifier};

use crate::entity::ageable::Ageable;
use crate::entity::{Entity, SharedEntity};
use crate::world::World;

/// The variant entry of `E`.
pub type VariantOf<E> = &'static <<E as Variants>::VariantRegistry as RegistryExt>::Entry;
/// The sound variant entry of `E`.
pub type SoundVariantOf<E> = &'static <<E as Variants>::SoundVariantRegistry as RegistryExt>::Entry;

/// A mob whose synced data holds a variant and a sound variant.
pub trait Variants: Entity + DowncastType + Sized {
    /// Where the variants come from.
    type VariantRegistry: SpawnVariantRegistry + 'static;
    /// Where the sound variants come from.
    type SoundVariantRegistry: SoundVariantRegistry + 'static;

    /// The variant registry.
    fn variant_registry() -> &'static Self::VariantRegistry;
    /// The sound variant registry.
    fn sound_variant_registry() -> &'static Self::SoundVariantRegistry;

    /// The current variant.
    fn variant(&self) -> VariantOf<Self>;
    /// Sets the variant.
    fn set_variant(&self, variant: VariantOf<Self>);
    /// The current sound variant.
    fn sound_variant(&self) -> SoundVariantOf<Self>;
    /// Sets the sound variant.
    fn set_sound_variant(&self, sound_variant: SoundVariantOf<Self>);
}

/// Saves the variant (`VariantUtils.writeVariant`) and the sound variant.
pub fn save<E: Variants>(entity: &E, nbt: &mut NbtCompound) {
    nbt.insert("variant", entity.variant().key().to_string());
    nbt.insert("sound_variant", entity.sound_variant().key().to_string());
}

/// Loads the variant (`VariantUtils.readVariant`) and the sound variant;
/// missing or unknown keys keep the current ones.
pub fn load<E: Variants>(entity: &E, nbt: BorrowedNbtCompoundView<'_, '_>) {
    if let Some(variant) =
        read_key(nbt, "variant").and_then(|key| E::variant_registry().by_key(&key))
    {
        entity.set_variant(variant);
    }
    if let Some(sound_variant) =
        read_key(nbt, "sound_variant").and_then(|key| E::sound_variant_registry().by_key(&key))
    {
        entity.set_sound_variant(sound_variant);
    }
}

fn read_key(nbt: BorrowedNbtCompoundView<'_, '_>, name: &str) -> Option<Identifier> {
    Identifier::from_str(nbt.string(name)?.to_str().as_ref()).ok()
}

/// `finalizeSpawn` before `super`: the variant for the spawn biome
/// (`VariantUtils.selectVariantToSpawn`) and a random sound variant.
pub fn pick_spawn_variants<E: Variants>(entity: &E, world: &World) {
    let biome = world.biome_at(entity.block_position());
    let mut random = LegacyRandom::from_seed(rand::random());
    let variant =
        biome.and_then(|biome| E::variant_registry().select_spawn_variant(biome, &mut random));
    let sound_variant = E::sound_variant_registry().pick_random(&mut random);
    if let Some(variant) = variant {
        entity.set_variant(variant);
    }
    if let Some(sound_variant) = sound_variant {
        entity.set_sound_variant(sound_variant);
    }
}

/// `getBreedOffspring`: a baby of the parent's type with either parent's variant.
pub fn breed_offspring<E: Variants>(
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
    if let (Some(source), Some(baby)) = (
        inherited.entity().downcast_ref::<E>(),
        offspring.downcast_ref::<E>(),
    ) {
        baby.set_variant(source.variant());
    }
    Some(offspring)
}
