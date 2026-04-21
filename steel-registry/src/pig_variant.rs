use crate::shared_structs::{SpawnConditionEntry, insert_spawn_conditions};
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

/// Represents a full pig variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct PigVariant {
    pub key: Identifier,
    pub asset_id: Identifier,
    pub baby_asset_id: Identifier,
    pub model: PigModelType,
    pub spawn_conditions: &'static [SpawnConditionEntry],
}

/// The model type for the pig, which can affect its shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PigModelType {
    #[default]
    Normal,
    Cold,
}

impl ToNbtTag for &PigVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};
        let mut compound = NbtCompound::new();
        compound.insert("asset_id", self.asset_id.clone());
        compound.insert("baby_asset_id", self.baby_asset_id.clone());
        compound.insert(
            "model",
            match self.model {
                PigModelType::Normal => "normal",
                PigModelType::Cold => "cold",
            },
        );
        insert_spawn_conditions(&mut compound, self.spawn_conditions);
        NbtTag::Compound(compound)
    }
}

crate::define_registry!(PigVariantRegistry, PigVariant, stem: pig_variants);
