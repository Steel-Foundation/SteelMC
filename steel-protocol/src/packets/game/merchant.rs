//! Wire types for `clientbound/minecraft:merchant_offers`, matching vanilla's
//! `MerchantOffer.writeToStream` / `MerchantOffer.createFromStream`.

use std::io::{Cursor, Result, Write};

use steel_macros::{ReadFrom, WriteTo};
use steel_registry::{
    REGISTRY, RegistryEntry, RegistryExt, data_component_predicate::DataComponentExactPredicate,
    item_stack::ItemStack, items::ItemRef,
};
use steel_utils::codec::VarInt;
use steel_utils::serial::{ReadFrom, WriteTo};

/// Wire form of vanilla `ItemCost`: the item a villager wants.
///
/// Unlike [`ItemStack`], the registry id precedes the count and there is no
/// empty representation. Component requirements use
/// [`DataComponentExactPredicate`] (`ItemCost.STREAM_CODEC`).
#[derive(Clone, Debug, PartialEq)]
pub struct ItemCost {
    pub item: ItemRef,
    pub count: i32,
    /// Exact component values the traded item must have; usually empty.
    pub components: DataComponentExactPredicate,
}

impl ItemCost {
    /// Mirrors vanilla `new ItemCost(item, count)`: no component requirements.
    #[must_use]
    pub fn from_stack(stack: &ItemStack) -> Self {
        debug_assert!(
            !stack.is_empty(),
            "merchant costs cannot be empty; vanilla clients reject empty stacks"
        );
        Self {
            item: stack.item(),
            count: stack.count(),
            components: DataComponentExactPredicate::EMPTY,
        }
    }
}

impl WriteTo for ItemCost {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.item.id() as i32).write(writer)?;
        VarInt(self.count).write(writer)?;
        self.components.write(writer)
    }
}

impl ReadFrom for ItemCost {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let raw_id = VarInt::read(data)?.0;
        let raw_id = usize::try_from(raw_id)
            .map_err(|_| std::io::Error::other(format!("Negative item id: {raw_id}")))?;
        let item = REGISTRY
            .items
            .by_id(raw_id)
            .ok_or_else(|| std::io::Error::other(format!("Unknown item id: {raw_id}")))?;
        let count = VarInt::read(data)?.0;
        let components = DataComponentExactPredicate::read(data)?;
        Ok(Self {
            item,
            count,
            components,
        })
    }
}

/// One trade offer, in exactly vanilla's field order:
/// cost A (`ItemCost`), result ([`ItemStack`]), optional cost B
/// (`Optional<ItemCost>`), then the trade state fields.
#[derive(ReadFrom, WriteTo, Clone, Debug, PartialEq)]
pub struct MerchantOfferPacket {
    pub cost_a: ItemCost,
    pub result: ItemStack,
    pub cost_b: Option<ItemCost>,
    pub out_of_stock: bool,
    pub uses: i32,
    pub max_uses: i32,
    pub xp: i32,
    pub special_price_diff: i32,
    pub price_multiplier: f32,
    pub demand: i32,
}

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_items};

    use super::*;

    fn sample_offer() -> MerchantOfferPacket {
        MerchantOfferPacket {
            cost_a: ItemCost {
                item: &vanilla_items::WHEAT,
                count: 16,
                components: DataComponentExactPredicate::EMPTY,
            },
            result: ItemStack::with_count(&vanilla_items::BREAD, 4),
            cost_b: None,
            out_of_stock: false,
            uses: 1,
            max_uses: 4,
            xp: 2,
            special_price_diff: -3,
            price_multiplier: 0.05,
            demand: 7,
        }
    }

    /// Pins the exact byte layout against vanilla 26.2:
    /// `[costA: id, count, predicate][result: count, id, patch]
    /// [costB present][outOfStock][uses][maxUses][xp][specialPriceDiff]
    /// [priceMultiplier][demand]`.
    #[test]
    fn offer_bytes_match_vanilla_26_2_layout() {
        init_vanilla_registry();
        let offer = sample_offer();
        let mut bytes = Vec::new();
        offer.write(&mut bytes).expect("offer encodes");

        let mut expected = Vec::new();
        // Cost A as ItemCost: item id first, then count, then predicate list.
        VarInt(vanilla_items::WHEAT.id() as i32)
            .write(&mut expected)
            .unwrap();
        VarInt(16).write(&mut expected).unwrap();
        VarInt(0).write(&mut expected).unwrap();
        // Result as non-empty ItemStack: count first, then item id and patch.
        VarInt(4).write(&mut expected).unwrap();
        VarInt(vanilla_items::BREAD.id() as i32)
            .write(&mut expected)
            .unwrap();
        VarInt(0).write(&mut expected).unwrap();
        VarInt(0).write(&mut expected).unwrap();
        // No second cost, not out of stock.
        expected.push(0);
        expected.push(0);
        // uses, maxUses, xp, specialPriceDiff as big-endian i32...
        for value in [1i32, 4, 2, -3] {
            expected.extend_from_slice(&value.to_be_bytes());
        }
        // ...then priceMultiplier f32 and demand i32.
        expected.extend_from_slice(&0.05f32.to_be_bytes());
        expected.extend_from_slice(&7i32.to_be_bytes());

        assert_eq!(bytes, expected);
    }

    #[test]
    fn offer_with_second_cost_round_trips() {
        init_vanilla_registry();
        let mut offer = sample_offer();
        offer.cost_b = Some(ItemCost::from_stack(&ItemStack::new(
            &vanilla_items::EMERALD,
        )));

        let mut bytes = Vec::new();
        offer.write(&mut bytes).expect("offer encodes");
        let decoded =
            MerchantOfferPacket::read(&mut Cursor::new(bytes.as_slice())).expect("decodes");

        assert_eq!(decoded, offer);
        assert_eq!(decoded.cost_a.count, 16);
        assert!(!decoded.result.is_empty());
        let second = decoded.cost_b.expect("second cost present");
        assert_eq!(second.item.id(), vanilla_items::EMERALD.id());
        assert_eq!(second.count, 1);
    }

    #[test]
    fn offer_round_trips_without_second_cost() {
        init_vanilla_registry();
        let offer = sample_offer();

        let mut bytes = Vec::new();
        offer.write(&mut bytes).expect("offer encodes");
        // Absent second cost is a single zero byte right before the state tail
        // (two bools + five ints + one float).
        assert_eq!(bytes[bytes.len() - 26], 0);

        let decoded =
            MerchantOfferPacket::read(&mut Cursor::new(bytes.as_slice())).expect("decodes");
        assert_eq!(decoded, offer);
    }

    #[test]
    fn from_stack_keeps_cost_semantics_and_rejects_empty_in_debug() {
        init_vanilla_registry();
        let stack = ItemStack::with_count(&vanilla_items::EMERALD, 3);
        let cost = ItemCost::from_stack(&stack);
        assert_eq!(cost.item.id(), vanilla_items::EMERALD.id());
        assert_eq!(cost.count, 3);
        assert!(cost.components.is_empty());
    }
}
