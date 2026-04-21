use crate::shared_structs::{SpawnConditionEntry, insert_spawn_conditions};
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

/// Represents a full frog variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct FrogVariant {
    pub key: Identifier,
    pub asset_id: Identifier,
    pub spawn_conditions: &'static [SpawnConditionEntry],
}

impl ToNbtTag for &FrogVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::NbtCompound;
        let mut compound = NbtCompound::new();
        let asset_id = self.asset_id.to_string();
        compound.insert("asset_id", asset_id.as_str());
        compound.insert("baby_asset_id", asset_id.as_str());
        insert_spawn_conditions(&mut compound, self.spawn_conditions);
        NbtTag::Compound(compound)
    }
}

crate::define_registry!(FrogVariantRegistry, FrogVariant, stem: frog_variants);
