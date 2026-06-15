use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;
use text_components::TextComponent;

use crate::sound_event::SoundEventRef;

/// Represents a musical instrument definition from a data pack JSON file,
/// primarily used for Goat Horns.
#[derive(Debug)]
pub struct Instrument {
    pub key: Identifier,
    pub sound_event: SoundEventRef,
    pub use_duration: f32,
    pub range: f32,
    pub description: TextComponent,
}

impl ToNbtTag for &Instrument {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::NbtCompound;
        let mut compound = NbtCompound::new();
        let sound_event = self.sound_event.key.to_string();
        compound.insert("sound_event", sound_event.as_str());
        compound.insert("use_duration", self.use_duration);
        compound.insert("range", self.range);
        compound.insert("description", (&self.description).to_nbt_tag());
        NbtTag::Compound(compound)
    }
}

pub type InstrumentRef = &'static Instrument;

pub struct InstrumentRegistry {
    instruments_by_id: Vec<InstrumentRef>,
    instruments_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl InstrumentRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            instruments_by_id: Vec::new(),
            instruments_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    InstrumentRegistry,
    InstrumentRef,
    instruments_by_id,
    instruments_by_key,
    allows_registering
);

crate::impl_registry!(
    InstrumentRegistry,
    Instrument,
    instruments_by_id,
    instruments_by_key,
    instruments
);

crate::impl_tagged_registry!(InstrumentRegistry, instruments_by_key, "instrument");

#[cfg(test)]
mod tests {
    use simdnbt::ToNbtTag;
    use simdnbt::owned::NbtTag;

    use crate::{test_support::init_test_registry, vanilla_instruments};

    #[test]
    fn nbt_uses_sound_event_registry_key() {
        init_test_registry();

        let NbtTag::Compound(compound) = (&vanilla_instruments::PONDER_GOAT_HORN).to_nbt_tag()
        else {
            panic!("instrument did not serialize to a compound tag");
        };

        let Some(sound_event) = compound.string("sound_event") else {
            panic!("instrument NBT is missing sound_event string");
        };

        assert_eq!(
            sound_event.to_str().as_ref(),
            "minecraft:item.goat_horn.sound.0"
        );
    }
}
