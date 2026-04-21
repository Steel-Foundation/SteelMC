use crate::shared_structs::{SpawnConditionEntry, insert_spawn_conditions};
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

/// Represents a full zombie nautilus variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct ZombieNautilusVariant {
    pub key: Identifier,
    pub asset_id: Identifier,
    pub model: Option<&'static str>,
    pub spawn_conditions: &'static [SpawnConditionEntry],
}

impl ToNbtTag for &ZombieNautilusVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
        let mut compound = NbtCompound::new();
        let asset_id = self.asset_id.to_string();
        compound.insert("asset_id", asset_id.as_str());
        compound.insert("baby_asset_id", asset_id.as_str());
        if let Some(model) = self.model {
            compound.insert("model", model);
        }
        insert_spawn_conditions(&mut compound, self.spawn_conditions);
        NbtTag::Compound(compound)
    }
}

crate::define_registry!(
    ZombieNautilusVariantRegistry,
    ZombieNautilusVariant,
    stem: zombie_nautilus_variants,
);
