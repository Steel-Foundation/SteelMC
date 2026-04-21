use crate::shared_structs::{SpawnConditionEntry, insert_spawn_conditions};
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

/// Represents a full wolf variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct WolfVariant {
    pub key: Identifier,
    pub assets: WolfAssetInfo,
    pub baby_assets: WolfAssetInfo,
    pub spawn_conditions: &'static [SpawnConditionEntry],
}

/// Contains the texture resource locations for a wolf variant.
#[derive(Debug)]
pub struct WolfAssetInfo {
    pub wild: Identifier,
    pub tame: Identifier,
    pub angry: Identifier,
}

impl ToNbtTag for &WolfVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtList};
        let mut compound = NbtCompound::new();
        let mut assets = NbtCompound::new();
        let wild = self.assets.wild.to_string();
        assets.insert("wild", wild.as_str());
        let tame = self.assets.tame.to_string();
        assets.insert("tame", tame.as_str());
        let angry = self.assets.angry.to_string();
        assets.insert("angry", angry.as_str());
        compound.insert("assets", NbtTag::Compound(assets));
        let mut baby_assets = NbtCompound::new();
        let wild = self.baby_assets.wild.to_string();
        baby_assets.insert("wild", wild.as_str());
        let tame = self.baby_assets.tame.to_string();
        baby_assets.insert("tame", tame.as_str());
        let angry = self.baby_assets.angry.to_string();
        baby_assets.insert("angry", angry.as_str());
        compound.insert("baby_assets", NbtTag::Compound(baby_assets));
        insert_spawn_conditions(&mut compound, self.spawn_conditions);
        NbtTag::Compound(compound)
    }
}

crate::define_registry!(WolfVariantRegistry, WolfVariant, stem: wolf_variants);
