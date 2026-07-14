//! Vanilla `minecraft:food` item component.

use std::io::{Cursor, Error, Result, Write};

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};

/// Nutrition restored by consuming an item.
#[derive(Debug, Clone)]
pub struct FoodProperties {
    nutrition: i32,
    saturation: f32,
    can_always_eat: bool,
}

impl PartialEq for FoodProperties {
    fn eq(&self, other: &Self) -> bool {
        self.nutrition == other.nutrition
            && java_float_equals(self.saturation, other.saturation)
            && self.can_always_eat == other.can_always_eat
    }
}

impl FoodProperties {
    /// Creates persistable food properties.
    pub fn new(nutrition: i32, saturation: f32, can_always_eat: bool) -> Result<Self> {
        if nutrition < 0 {
            return Err(Error::other("Food nutrition must be non-negative"));
        }
        Ok(Self {
            nutrition,
            saturation,
            can_always_eat,
        })
    }

    pub(crate) const fn from_extracted(
        nutrition: i32,
        saturation: f32,
        can_always_eat: bool,
    ) -> Self {
        assert!(
            nutrition >= 0,
            "extracted food nutrition must be non-negative"
        );
        Self {
            nutrition,
            saturation,
            can_always_eat,
        }
    }

    #[must_use]
    pub const fn nutrition(&self) -> i32 {
        self.nutrition
    }

    #[must_use]
    pub const fn saturation(&self) -> f32 {
        self.saturation
    }

    #[must_use]
    pub const fn can_always_eat(&self) -> bool {
        self.can_always_eat
    }
}

impl WriteTo for FoodProperties {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.nutrition).write(writer)?;
        self.saturation.write(writer)?;
        self.can_always_eat.write(writer)
    }
}

impl ReadFrom for FoodProperties {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Self::new(VarInt::read(data)?.0, f32::read(data)?, bool::read(data)?)
    }
}

impl ToNbtTag for FoodProperties {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("nutrition", self.nutrition);
        compound.insert("saturation", self.saturation);
        if self.can_always_eat {
            compound.insert("can_always_eat", true);
        }
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for FoodProperties {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let nutrition = compound.get("nutrition")?.codec_i32()?;
        let saturation = compound.get("saturation")?.codec_f32()?;
        let can_always_eat = match compound.get("can_always_eat") {
            Some(tag) => tag.codec_bool()?,
            None => false,
        };
        Self::new(nutrition, saturation, can_always_eat).ok()
    }
}

impl HashComponent for FoodProperties {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(3);
        push_hash_entry(&mut entries, "nutrition", &self.nutrition);
        push_hash_entry(&mut entries, "saturation", &self.saturation);
        if self.can_always_eat {
            push_hash_entry(&mut entries, "can_always_eat", &true);
        }
        sort_map_entries(&mut entries);
        hasher.start_map();
        for entry in &entries {
            hasher.put_raw_bytes(&entry.key_bytes);
            hasher.put_raw_bytes(&entry.value_bytes);
        }
        hasher.end_map();
    }
}

fn push_hash_entry<T: HashComponent + ?Sized>(entries: &mut Vec<HashEntry>, key: &str, value: &T) {
    let mut key_hasher = ComponentHasher::new();
    key_hasher.put_string(key);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

const fn java_float_equals(left: f32, right: f32) -> bool {
    (left.is_nan() && right.is_nan()) || left.to_bits() == right.to_bits()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::owned::{NbtCompound, NbtTag};
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::FoodProperties;
    use crate::data_components::vanilla_components::FOOD;
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, RegistryExt};

    fn parse(tag: NbtTag) -> Option<FoodProperties> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = simdnbt::borrow::read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        FoodProperties::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn food_codecs_use_required_values_and_optional_false() {
        let food = FoodProperties::new(4, 2.4, false).expect("valid food");
        let mut expected = NbtCompound::new();
        expected.insert("nutrition", 4);
        expected.insert("saturation", 2.4_f32);
        let expected = NbtTag::Compound(expected);
        assert_eq!(food.clone().to_nbt_tag(), expected);
        assert_eq!(parse(expected.clone()), Some(food.clone()));
        assert_eq!(food.compute_hash(), expected.compute_hash());

        let mut network = Vec::new();
        food.write(&mut network).expect("food should encode");
        assert_eq!(
            FoodProperties::read(&mut Cursor::new(network.as_slice())).expect("food should decode"),
            food
        );
    }

    #[test]
    fn negative_nutrition_is_rejected_for_persistable_values() {
        assert!(FoodProperties::new(-1, 0.0, false).is_err());
        let mut invalid = NbtCompound::new();
        invalid.insert("nutrition", -1);
        invalid.insert("saturation", 0.0_f32);
        assert!(parse(NbtTag::Compound(invalid)).is_none());
    }

    #[test]
    fn equality_uses_java_record_float_semantics() {
        assert_eq!(
            FoodProperties::new(1, f32::from_bits(0x7fc0_0001), false).expect("valid food"),
            FoodProperties::new(1, f32::from_bits(0x7fc0_0002), false).expect("valid food")
        );
        assert_ne!(
            FoodProperties::new(1, 0.0, false).expect("valid food"),
            FoodProperties::new(1, -0.0, false).expect("valid food")
        );
    }

    #[test]
    fn extracted_food_prototypes_keep_vanilla_values() {
        init_test_registry();
        let apple = REGISTRY
            .items
            .by_key(&steel_utils::Identifier::vanilla_static("apple"))
            .expect("apple should be registered");
        assert_eq!(
            apple.components.get(FOOD),
            Some(FoodProperties::new(4, 2.4, false).expect("valid apple food"))
        );

        let golden_apple = REGISTRY
            .items
            .by_key(&steel_utils::Identifier::vanilla_static("golden_apple"))
            .expect("golden apple should be registered");
        assert_eq!(
            golden_apple.components.get(FOOD),
            Some(FoodProperties::new(4, 9.6, true).expect("valid golden apple food"))
        );
    }
}
