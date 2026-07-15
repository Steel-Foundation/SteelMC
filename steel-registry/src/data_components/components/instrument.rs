//! Vanilla `minecraft:instrument` item component.

use std::io::{Cursor, Result, Write};

use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::RegistryHolder;
use crate::instrument::{Instrument, InstrumentValue};

/// Instrument selected for an instrument item such as a goat horn.
#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentComponent {
    instrument: RegistryHolder<Instrument>,
}

impl InstrumentComponent {
    #[must_use]
    pub const fn new(instrument: RegistryHolder<Instrument>) -> Self {
        Self { instrument }
    }

    #[must_use]
    pub const fn instrument(&self) -> &RegistryHolder<Instrument> {
        &self.instrument
    }

    #[must_use]
    pub fn value(&self) -> &InstrumentValue {
        self.instrument.value()
    }
}

impl WriteTo for InstrumentComponent {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.instrument.write(writer)
    }
}

impl ReadFrom for InstrumentComponent {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        RegistryHolder::read(data).map(Self::new)
    }
}

impl ToNbtTag for InstrumentComponent {
    fn to_nbt_tag(self) -> simdnbt::owned::NbtTag {
        self.instrument.to_nbt_tag()
    }
}

impl FromNbtTag for InstrumentComponent {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        RegistryHolder::from_nbt_tag(tag).map(Self::new)
    }
}

impl HashComponent for InstrumentComponent {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.instrument.hash_component(hasher);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::Identifier;
    use steel_utils::codec::VarInt;
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};
    use text_components::TextComponent;

    use super::InstrumentComponent;
    use crate::RegistryHolder;
    use crate::data_components::vanilla_components::INSTRUMENT;
    use crate::instrument::InstrumentValue;
    use crate::item_stack::ItemStack;
    use crate::sound_event::SoundEventHolder;
    use crate::test_support::init_test_registry;
    use crate::{sound_events, vanilla_instruments, vanilla_items};

    fn parse_component(tag: simdnbt::owned::NbtTag) -> Option<InstrumentComponent> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        InstrumentComponent::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn registry_reference_round_trips_both_codecs() {
        init_test_registry();
        let component = InstrumentComponent::new(RegistryHolder::reference(
            &vanilla_instruments::PONDER_GOAT_HORN,
        ));

        let mut network = Vec::new();
        component
            .write(&mut network)
            .expect("registry instrument should encode");
        assert_eq!(
            InstrumentComponent::read(&mut Cursor::new(network.as_slice()))
                .expect("registry instrument should decode"),
            component
        );

        let nbt = component.clone().to_nbt_tag();
        assert_eq!(
            nbt,
            simdnbt::owned::NbtTag::String("minecraft:ponder_goat_horn".into())
        );
        assert_eq!(parse_component(nbt), Some(component));
    }

    #[test]
    fn valid_inline_instrument_round_trips_and_hashes_its_record() {
        init_test_registry();
        let value = InstrumentValue::new(
            SoundEventHolder::Direct {
                sound_id: Identifier::vanilla_static("custom_horn"),
                fixed_range: Some(32.0),
            },
            3.5,
            48.0,
            TextComponent::plain("Custom horn"),
        )
        .expect("inline instrument should be valid");
        let component = InstrumentComponent::new(RegistryHolder::direct(value));

        let mut network = Vec::new();
        component
            .write(&mut network)
            .expect("inline instrument should encode");
        assert_eq!(
            InstrumentComponent::read(&mut Cursor::new(network.as_slice()))
                .expect("inline instrument should decode"),
            component
        );

        let nbt = component.clone().to_nbt_tag();
        assert_eq!(parse_component(nbt.clone()), Some(component.clone()));
        assert_eq!(component.compute_hash(), nbt.compute_hash());
    }

    #[test]
    fn network_rejects_inline_values_that_cannot_persist() {
        init_test_registry();
        let mut network = Vec::new();
        VarInt(0)
            .write(&mut network)
            .expect("direct holder discriminator should encode");
        SoundEventHolder::registry(&sound_events::ITEM_GOAT_HORN_SOUND_0)
            .write(&mut network)
            .expect("sound event should encode");
        (-1.0_f32)
            .write(&mut network)
            .expect("duration should encode");
        16.0_f32.write(&mut network).expect("range should encode");
        TextComponent::plain("Invalid horn")
            .write(&mut network)
            .expect("description should encode");

        assert!(InstrumentComponent::read(&mut Cursor::new(network.as_slice())).is_err());
    }

    #[test]
    fn extracted_goat_horn_uses_ponder_instrument() {
        init_test_registry();
        let goat_horn = ItemStack::new(&vanilla_items::GOAT_HORN);

        assert_eq!(
            goat_horn
                .get(INSTRUMENT)
                .and_then(|component| component.instrument().as_reference()),
            Some(&vanilla_instruments::PONDER_GOAT_HORN)
        );
    }
}
