//! Vanilla command item-slot ranges.

use std::sync::LazyLock;

use text_components::TextComponent;

use super::super::brigadier::{
    CommandSyntaxError, CommandSyntaxErrorKind, StringReader, SuggestionsBuilder,
};

/// A named vanilla `SlotRange` resolved to its command slot IDs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ItemSlotRange {
    name: Box<str>,
    slots: Box<[i32]>,
}

impl ItemSlotRange {
    fn new(name: impl Into<Box<str>>, slots: impl Into<Box<[i32]>>) -> Self {
        Self {
            name: name.into(),
            slots: slots.into(),
        }
    }

    /// Returns the serialized vanilla slot-range name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// Returns the command slot IDs in this range.
    pub(crate) fn slots(&self) -> &[i32] {
        &self.slots
    }
}

pub(super) fn parse_item_slots(
    reader: &mut StringReader<'_>,
) -> Result<ItemSlotRange, CommandSyntaxError> {
    let start = reader.read_so_far().len();
    while reader.peek().is_some_and(|character| character != ' ') {
        reader.skip();
    }
    let name = &reader.input()[start..reader.read_so_far().len()];
    ITEM_SLOT_RANGES
        .iter()
        .find(|range| range.name() == name)
        .cloned()
        .ok_or_else(|| unknown_slot(reader, name))
}

pub(super) fn suggest_item_slots(builder: &mut SuggestionsBuilder<'_>) {
    let prefix = builder.remaining();
    for range in ITEM_SLOT_RANGES
        .iter()
        .filter(|range| range.name().starts_with(prefix))
    {
        builder.suggest(range.name());
    }
}

fn unknown_slot(reader: &StringReader<'_>, name: &str) -> CommandSyntaxError {
    reader.error(CommandSyntaxErrorKind::Dynamic(Box::new(
        TextComponent::from(format!("Unknown slot '{name}'")),
    )))
}

static ITEM_SLOT_RANGES: LazyLock<Vec<ItemSlotRange>> = LazyLock::new(|| {
    let mut ranges = Vec::new();
    ranges.push(ItemSlotRange::new("contents", [0].as_slice()));
    add_range(&mut ranges, "container.", 0, 54);
    add_range(&mut ranges, "hotbar.", 0, 9);
    add_range(&mut ranges, "inventory.", 9, 27);
    add_range(&mut ranges, "enderchest.", 200, 27);
    add_range(&mut ranges, "mob.inventory.", 300, 8);
    add_range(&mut ranges, "horse.", 500, 15);
    ranges.push(ItemSlotRange::new("weapon", [98].as_slice()));
    ranges.push(ItemSlotRange::new("weapon.mainhand", [98].as_slice()));
    ranges.push(ItemSlotRange::new("weapon.offhand", [99].as_slice()));
    ranges.push(ItemSlotRange::new("weapon.*", [98, 99].as_slice()));
    ranges.push(ItemSlotRange::new("armor.head", [103].as_slice()));
    ranges.push(ItemSlotRange::new("armor.chest", [102].as_slice()));
    ranges.push(ItemSlotRange::new("armor.legs", [101].as_slice()));
    ranges.push(ItemSlotRange::new("armor.feet", [100].as_slice()));
    ranges.push(ItemSlotRange::new("armor.body", [105].as_slice()));
    ranges.push(ItemSlotRange::new(
        "armor.*",
        [103, 102, 101, 100, 105].as_slice(),
    ));
    ranges.push(ItemSlotRange::new("saddle", [106].as_slice()));
    ranges.push(ItemSlotRange::new("horse.chest", [499].as_slice()));
    ranges.push(ItemSlotRange::new("player.cursor", [499].as_slice()));
    add_range(&mut ranges, "player.crafting.", 500, 4);
    ranges
});

fn add_range(ranges: &mut Vec<ItemSlotRange>, prefix: &str, offset: i32, size: i32) {
    let mut all_slots = Vec::new();
    for index in 0..size {
        let slot = offset + index;
        ranges.push(ItemSlotRange::new(
            format!("{prefix}{index}"),
            [slot].as_slice(),
        ));
        all_slots.push(slot);
    }
    ranges.push(ItemSlotRange::new(format!("{prefix}*"), all_slots));
}
