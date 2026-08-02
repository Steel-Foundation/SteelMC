//! Vanilla block-state NBT codec shape used by block entities.

use std::str::FromStr;

use simdnbt::borrow::NbtCompound as BorrowedNbtCompound;
use simdnbt::owned::NbtCompound;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::{REGISTRY, RegistryExt as _};
use steel_utils::{BlockStateId, Identifier};

#[must_use]
pub(crate) fn save(state: BlockStateId) -> NbtCompound {
    let mut encoded = NbtCompound::new();
    encoded.insert("Name", state.get_block().key.to_string());

    let state_properties = REGISTRY.blocks.get_properties(state);
    if !state_properties.is_empty() {
        let mut properties = NbtCompound::new();
        for (name, value) in state_properties {
            properties.insert(name, value);
        }
        encoded.insert("Properties", properties);
    }

    encoded
}

#[must_use]
pub(crate) fn load(encoded: BorrowedNbtCompound<'_, '_>) -> Option<BlockStateId> {
    let name = encoded.string("Name")?;
    let identifier = Identifier::from_str(name.to_str().as_ref()).ok()?;
    let block = REGISTRY.blocks.by_key(&identifier)?;

    let mut properties = Vec::new();
    if let Some(encoded_properties) = encoded.compound("Properties") {
        for (name, value) in encoded_properties.iter() {
            let value = value.string()?;
            properties.push((name.to_str().into_owned(), value.to_str().into_owned()));
        }
    }

    REGISTRY.blocks.state_id_from_block_defaulted_properties(
        block,
        properties
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str())),
    )
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use simdnbt::owned::NbtTag;
    use steel_registry::blocks::properties::{BlockStateProperties, Direction, PistonType};
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_blocks;

    use super::*;

    #[test]
    fn propertyful_block_state_round_trips_vanilla_nbt_shape() {
        init_test_registry();
        let state = vanilla_blocks::PISTON_HEAD
            .default_state()
            .set_value(&BlockStateProperties::FACING, Direction::East)
            .set_value(&BlockStateProperties::PISTON_TYPE, PistonType::Sticky)
            .set_value(&BlockStateProperties::SHORT, true);

        let encoded = save(state);
        assert_eq!(
            encoded.string("Name").map(|name| name.to_str()),
            Some("minecraft:piston_head".into())
        );
        assert!(matches!(
            encoded.get("Properties"),
            Some(NbtTag::Compound(_))
        ));

        let mut bytes = Vec::new();
        encoded.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(bytes.as_slice()))
            .expect("test NBT should reborrow");
        let view = BorrowedNbtCompound::from(&borrowed);

        assert_eq!(load(view), Some(state));
    }
}
