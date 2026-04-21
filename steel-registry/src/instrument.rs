use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;
use text_components::TextComponent;

/// Represents a musical instrument definition from a data pack JSON file,
/// primarily used for Goat Horns.
#[derive(Debug)]
pub struct Instrument {
    pub key: Identifier,
    pub sound_event: Identifier,
    pub use_duration: f32,
    pub range: f32,
    pub description: TextComponent,
}

impl ToNbtTag for &Instrument {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::NbtCompound;
        let mut compound = NbtCompound::new();
        let sound_event = self.sound_event.to_string();
        compound.insert("sound_event", sound_event.as_str());
        compound.insert("use_duration", self.use_duration);
        compound.insert("range", self.range);
        compound.insert("description", (&self.description).to_nbt_tag());
        NbtTag::Compound(compound)
    }
}

crate::define_registry!(
    InstrumentRegistry,
    Instrument,
    stem: instruments,
    tagged: "instrument",
);
