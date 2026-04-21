use crate::shared_structs::{SpawnConditionEntry, insert_spawn_conditions};
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

/// Represents a full cow variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct CowVariant {
    pub key: Identifier,
    pub asset_id: Identifier,
    pub baby_asset_id: Identifier,
    pub model: CowModelType,
    pub spawn_conditions: &'static [SpawnConditionEntry],
}

/// The model type for the cow, which can affect its shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CowModelType {
    #[default]
    Normal,
    Cold,
    Warm,
}

impl ToNbtTag for &CowVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
        let mut compound = NbtCompound::new();
        compound.insert("asset_id", self.asset_id.clone());
        compound.insert("baby_asset_id", self.baby_asset_id.clone());
        compound.insert(
            "model",
            match self.model {
                CowModelType::Normal => "normal",
                CowModelType::Cold => "cold",
                CowModelType::Warm => "warm",
            },
        );
        insert_spawn_conditions(&mut compound, self.spawn_conditions);
        NbtTag::Compound(compound)
    }
}

crate::define_registry!(CowVariantRegistry, CowVariant, stem: cow_variants);
