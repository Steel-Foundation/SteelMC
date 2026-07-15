//! Registry references used by fixed holder codecs.

use std::fmt::Debug;
use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::owned::NbtTag;
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::cat_sound_variant::CatSoundVariant;
use crate::cat_variant::CatVariant;
use crate::chicken_sound_variant::ChickenSoundVariant;
use crate::chicken_variant::ChickenVariant;
use crate::cow_sound_variant::CowSoundVariant;
use crate::cow_variant::CowVariant;
use crate::frog_variant::FrogVariant;
use crate::map_decoration_type::MapDecorationType;
use crate::pig_sound_variant::PigSoundVariant;
use crate::pig_variant::PigVariant;
use crate::potion::Potion;
use crate::villager_type::VillagerType;
use crate::wolf_sound_variant::WolfSoundVariant;
use crate::wolf_variant::WolfVariant;
use crate::zombie_nautilus_variant::ZombieNautilusVariant;
use crate::{REGISTRY, RegistryEntry, RegistryExt};

/// Registry operations required by [`RegistryReference`].
pub trait RegistryReferenceEntry: RegistryEntry + Debug + Send + Sync {
    /// Human-readable registry name used in codec errors.
    const REGISTRY_NAME: &'static str;

    /// Looks up an entry by its protocol registry ID.
    fn reference_by_id(id: usize) -> Option<&'static Self>;

    /// Looks up an entry by its registry key.
    fn reference_by_key(key: &Identifier) -> Option<&'static Self>;
}

/// A registry-owned value encoded by `RegistryFixedCodec` and
/// `ByteBufCodecs.holderRegistry`.
#[derive(Debug)]
pub struct RegistryReference<T: RegistryReferenceEntry> {
    value: &'static T,
}

impl<T: RegistryReferenceEntry> RegistryReference<T> {
    #[must_use]
    pub const fn new(value: &'static T) -> Self {
        Self { value }
    }

    #[must_use]
    pub const fn value(&self) -> &'static T {
        self.value
    }
}

impl<T: RegistryReferenceEntry> Clone for RegistryReference<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: RegistryReferenceEntry> Copy for RegistryReference<T> {}

impl<T: RegistryReferenceEntry> PartialEq for RegistryReference<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value.key() == other.value.key()
    }
}

impl<T: RegistryReferenceEntry> Eq for RegistryReference<T> {}

impl<T: RegistryReferenceEntry> WriteTo for RegistryReference<T> {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let id = self.value.try_id().ok_or_else(|| {
            Error::other(format!(
                "Unknown {}: {}",
                T::REGISTRY_NAME,
                self.value.key()
            ))
        })?;
        let id = i32::try_from(id).map_err(|_| {
            Error::other(format!(
                "{} id out of protocol range: {id}",
                T::REGISTRY_NAME
            ))
        })?;
        VarInt(id).write(writer)
    }
}

impl<T: RegistryReferenceEntry> ReadFrom for RegistryReference<T> {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let encoded_id = VarInt::read(data)?.0;
        let id = usize::try_from(encoded_id).map_err(|_| {
            Error::other(format!(
                "Negative {} registry id: {encoded_id}",
                T::REGISTRY_NAME
            ))
        })?;
        T::reference_by_id(id)
            .map(Self::new)
            .ok_or_else(|| Error::other(format!("Unknown {} registry id: {id}", T::REGISTRY_NAME)))
    }
}

impl<T: RegistryReferenceEntry> ToNbtTag for RegistryReference<T> {
    fn to_nbt_tag(self) -> NbtTag {
        NbtTag::String(self.value.key().to_string().into())
    }
}

impl<T: RegistryReferenceEntry> FromNbtTag for RegistryReference<T> {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let key = Identifier::from_str(&tag.string()?.to_str()).ok()?;
        T::reference_by_key(&key).map(Self::new)
    }
}

impl<T: RegistryReferenceEntry> HashComponent for RegistryReference<T> {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.value.key().to_string().hash_component(hasher);
    }
}

macro_rules! impl_registry_reference_entry {
    ($entry:ty, $registry:ident, $name:literal) => {
        impl RegistryReferenceEntry for $entry {
            const REGISTRY_NAME: &'static str = $name;

            fn reference_by_id(id: usize) -> Option<&'static Self> {
                REGISTRY.$registry.by_id(id)
            }

            fn reference_by_key(key: &Identifier) -> Option<&'static Self> {
                REGISTRY.$registry.by_key(key)
            }
        }
    };
}

impl_registry_reference_entry!(VillagerType, villager_types, "villager type");
impl_registry_reference_entry!(WolfVariant, wolf_variants, "wolf variant");
impl_registry_reference_entry!(WolfSoundVariant, wolf_sound_variants, "wolf sound variant");
impl_registry_reference_entry!(PigVariant, pig_variants, "pig variant");
impl_registry_reference_entry!(PigSoundVariant, pig_sound_variants, "pig sound variant");
impl_registry_reference_entry!(CowVariant, cow_variants, "cow variant");
impl_registry_reference_entry!(CowSoundVariant, cow_sound_variants, "cow sound variant");
impl_registry_reference_entry!(ChickenVariant, chicken_variants, "chicken variant");
impl_registry_reference_entry!(
    ChickenSoundVariant,
    chicken_sound_variants,
    "chicken sound variant"
);
impl_registry_reference_entry!(
    ZombieNautilusVariant,
    zombie_nautilus_variants,
    "zombie nautilus variant"
);
impl_registry_reference_entry!(FrogVariant, frog_variants, "frog variant");
impl_registry_reference_entry!(CatVariant, cat_variants, "cat variant");
impl_registry_reference_entry!(CatSoundVariant, cat_sound_variants, "cat sound variant");
impl_registry_reference_entry!(Potion, potions, "potion");
impl_registry_reference_entry!(
    MapDecorationType,
    map_decoration_types,
    "map decoration type"
);

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::codec::VarInt;
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::{RegistryReference, RegistryReferenceEntry};
    use crate::cat_sound_variant::CatSoundVariant;
    use crate::cat_variant::CatVariant;
    use crate::chicken_sound_variant::ChickenSoundVariant;
    use crate::chicken_variant::ChickenVariant;
    use crate::cow_sound_variant::CowSoundVariant;
    use crate::cow_variant::CowVariant;
    use crate::frog_variant::FrogVariant;
    use crate::map_decoration_type::MapDecorationType;
    use crate::pig_sound_variant::PigSoundVariant;
    use crate::pig_variant::PigVariant;
    use crate::test_support::init_test_registry;
    use crate::villager_type::VillagerType;
    use crate::wolf_sound_variant::WolfSoundVariant;
    use crate::wolf_variant::WolfVariant;
    use crate::zombie_nautilus_variant::ZombieNautilusVariant;

    fn assert_codecs<T: RegistryReferenceEntry>() {
        let entry = T::reference_by_id(0).expect("vanilla registry should not be empty");
        let reference = RegistryReference::new(entry);

        let mut network = Vec::new();
        reference
            .write(&mut network)
            .expect("registry reference should encode");
        assert_eq!(
            RegistryReference::<T>::read(&mut Cursor::new(network.as_slice()))
                .expect("registry reference should decode"),
            reference
        );

        let nbt = reference.to_nbt_tag();
        assert_eq!(reference.compute_hash(), nbt.compute_hash());
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice()))
            .expect("registry reference NBT should parse");
        assert_eq!(
            RegistryReference::<T>::from_nbt_tag(borrowed.as_tag()),
            Some(reference)
        );
    }

    #[test]
    fn fixed_variant_references_share_vanilla_holder_codecs() {
        init_test_registry();
        assert_codecs::<VillagerType>();
        assert_codecs::<WolfVariant>();
        assert_codecs::<WolfSoundVariant>();
        assert_codecs::<PigVariant>();
        assert_codecs::<PigSoundVariant>();
        assert_codecs::<CowVariant>();
        assert_codecs::<CowSoundVariant>();
        assert_codecs::<ChickenVariant>();
        assert_codecs::<ChickenSoundVariant>();
        assert_codecs::<ZombieNautilusVariant>();
        assert_codecs::<FrogVariant>();
        assert_codecs::<CatVariant>();
        assert_codecs::<CatSoundVariant>();
        assert_codecs::<MapDecorationType>();
    }

    #[test]
    fn fixed_variant_references_reject_unknown_network_ids() {
        init_test_registry();
        let mut network = Vec::new();
        VarInt(i32::MAX)
            .write(&mut network)
            .expect("invalid test id should encode");
        assert!(
            RegistryReference::<VillagerType>::read(&mut Cursor::new(network.as_slice())).is_err()
        );
    }
}
