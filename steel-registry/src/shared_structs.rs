use simdnbt::ToNbtTag;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};

/// A single entry in the list of spawn conditions.
#[derive(Debug)]
pub struct SpawnConditionEntry {
    pub priority: i32,
    pub condition: Option<BiomeCondition>,
}

impl ToNbtTag for &SpawnConditionEntry {
    fn to_nbt_tag(self) -> NbtTag {
        let mut e = NbtCompound::new();
        e.insert("priority", self.priority);
        if let Some(cond) = &self.condition {
            e.insert("condition", cond.to_nbt_tag());
        }
        NbtTag::Compound(e)
    }
}

/// Defines a condition based on a biome or list of biomes.
#[derive(Debug)]
pub struct BiomeCondition {
    pub condition_type: &'static str,
    pub biomes: &'static str,
}

impl ToNbtTag for &BiomeCondition {
    fn to_nbt_tag(self) -> NbtTag {
        let mut c = NbtCompound::new();
        c.insert("type", self.condition_type);
        c.insert("biomes", self.biomes);
        NbtTag::Compound(c)
    }
}

/// Serialize a `spawn_conditions` list into the enclosing compound.
/// Matches vanilla's `[{priority, condition?}, …]` shape exactly.
pub fn insert_spawn_conditions(compound: &mut NbtCompound, entries: &[SpawnConditionEntry]) {
    let list: Vec<NbtCompound> = entries
        .iter()
        .map(|entry| {
            let mut e = NbtCompound::new();
            e.insert("priority", entry.priority);
            if let Some(cond) = &entry.condition {
                e.insert("condition", cond.to_nbt_tag());
            }
            e
        })
        .collect();
    compound.insert("spawn_conditions", NbtTag::List(NbtList::Compound(list)));
}
