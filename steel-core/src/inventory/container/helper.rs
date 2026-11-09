//! NBT loading shared by containers.

use simdnbt::borrow::NbtCompound as NbtCompoundView;
use steel_registry::item_stack::ItemStack;
use steel_utils::nbt::NbtNumeric as _;

/// Loads the `Items` list into `items`, skipping entries that fail to decode
/// or whose slot is out of range.
pub fn load_all_items(nbt: NbtCompoundView<'_, '_>, items: &mut [ItemStack]) {
    let Some(entries) = nbt.list("Items").and_then(|list| list.compounds()) else {
        return;
    };
    for entry in entries {
        let Some(slot) = item_slot(&entry) else {
            continue;
        };
        if let Some(target) = items.get_mut(slot)
            && let Some(stack) = ItemStack::from_borrowed_compound(&entry)
        {
            *target = stack;
        }
    }
}

/// Reads the unsigned `Slot` byte, which is 0 when absent and `None` when not numeric.
fn item_slot(entry: &NbtCompoundView<'_, '_>) -> Option<usize> {
    let Some(tag) = entry.get("Slot") else {
        return Some(0);
    };
    tag.codec_i8().map(|slot| usize::from(slot as u8))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_compound;
    use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
    use steel_registry::{init_vanilla_registry, vanilla_items};

    use super::*;

    fn item_entry(slot: Option<NbtTag>) -> NbtCompound {
        let mut entry = NbtCompound::new();
        entry.insert("id", "minecraft:stone");
        entry.insert("count", 1);
        if let Some(slot) = slot {
            entry.insert("Slot", slot);
        }
        entry
    }

    fn load(entries: Vec<NbtCompound>) -> Vec<ItemStack> {
        init_vanilla_registry();
        let mut nbt = NbtCompound::new();
        nbt.insert("Items", NbtList::Compound(entries));
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed =
            read_compound(&mut Cursor::new(bytes.as_slice())).expect("test NBT should reborrow");

        let mut items = vec![ItemStack::empty(); 3];
        load_all_items((&borrowed).into(), &mut items);
        items
    }

    #[test]
    fn missing_slot_defaults_to_zero() {
        assert!(load(vec![item_entry(None)])[0].is(&vanilla_items::STONE));
    }

    #[test]
    fn numeric_slot_narrows_to_a_byte() {
        let items = load(vec![item_entry(Some(NbtTag::Int(0x102)))]);
        assert!(items[2].is(&vanilla_items::STONE));
    }

    #[test]
    fn negative_slot_is_unsigned_and_out_of_range() {
        let items = load(vec![item_entry(Some(NbtTag::Byte(-1)))]);
        assert!(items.iter().all(ItemStack::is_empty));
    }

    #[test]
    fn non_numeric_slot_rejects_the_entry() {
        let items = load(vec![item_entry(Some(NbtTag::String("0".into())))]);
        assert!(items.iter().all(ItemStack::is_empty));
    }
}
