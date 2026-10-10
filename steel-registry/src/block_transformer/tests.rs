use std::convert::Infallible;
use std::io::Cursor;
use std::str::FromStr as _;

use serde_json::Value;
use simdnbt::ToNbtTag as _;
use simdnbt::owned::NbtTag;
use steel_utils::hash::{ComponentHasher, HashComponent as _};
use steel_utils::serial::{ReadFrom as _, WriteTo as _};
use steel_utils::{BlockPos, Identifier};

use crate::data_components::vanilla_components::BLOCK_TRANSFORMER;
use crate::item_stack::ItemStack;
use crate::loot_table::LootContext;
use crate::{REGISTRY, RegistryExt as _, init_vanilla_registry, vanilla_blocks, vanilla_items};

use super::BlockTransformerComponent;

fn nbt_json(tag: &NbtTag) -> Value {
    match tag {
        NbtTag::String(value) => Value::String(value.to_str().into_owned()),
        NbtTag::Byte(value) => Value::Bool(*value != 0),
        NbtTag::Int(value) => Value::from(*value),
        NbtTag::IntArray(values) => {
            Value::Array(values.iter().map(|value| Value::from(*value)).collect())
        }
        NbtTag::List(list) => Value::Array(list.as_nbt_tags().iter().map(nbt_json).collect()),
        NbtTag::Compound(compound) => Value::Object(
            compound
                .iter()
                .map(|(name, value)| (name.to_str().into_owned(), nbt_json(value)))
                .collect(),
        ),
        tag => panic!("unexpected transformer NBT: {tag:?}"),
    }
}

#[test]
fn registry_sync_nbt_matches_the_extracted_final_vanilla_definitions() {
    init_vanilla_registry();
    for (name, json) in [
        (
            "axe",
            include_str!(
                "../../../steel-utils/build_assets/builtin_datapacks/minecraft/block_transformer/axe.json"
            ),
        ),
        (
            "hoe",
            include_str!(
                "../../../steel-utils/build_assets/builtin_datapacks/minecraft/block_transformer/hoe.json"
            ),
        ),
        (
            "shovel",
            include_str!(
                "../../../steel-utils/build_assets/builtin_datapacks/minecraft/block_transformer/shovel.json"
            ),
        ),
    ] {
        let transformer = REGISTRY
            .block_transformers
            .by_key(&Identifier::vanilla_static(name))
            .expect("registered transformer");
        let expected: Value = serde_json::from_str(json).expect("extracted transformer JSON");
        assert_eq!(nbt_json(&transformer.to_nbt_tag()), expected, "{name}");
    }
}

#[test]
fn extracted_item_components_keep_registry_identity_on_the_wire_in_nbt_and_when_hashed() {
    init_vanilla_registry();
    let items: Value = serde_json::from_str(include_str!("../../build_assets/items.json"))
        .expect("extracted items");
    let mut checked = 0;
    for item in items["items"].as_array().expect("items array") {
        let Some(key) = item["components"]["minecraft:block_transformer"].as_str() else {
            continue;
        };
        let name = Identifier::vanilla(item["name"].as_str().expect("item name").to_owned());
        let item = REGISTRY.items.by_key(&name).expect("registered item");
        let component = item
            .components
            .get_ref(BLOCK_TRANSFORMER)
            .expect("transformer component");
        let key = Identifier::from_str(key).expect("transformer identifier");
        let expected = REGISTRY
            .block_transformers
            .by_key(&key)
            .expect("registry holder");
        assert!(std::ptr::eq(component.block_transformer, expected));

        let mut bytes = Vec::new();
        component.write(&mut bytes).expect("encode holder");
        let decoded =
            BlockTransformerComponent::read(&mut Cursor::new(&bytes)).expect("decode holder");
        assert!(std::ptr::eq(decoded.block_transformer, expected));

        let mut actual_hash = ComponentHasher::new();
        component.hash_component(&mut actual_hash);
        let mut expected_hash = ComponentHasher::new();
        expected_hash.put_string(&key.to_string());
        assert_eq!(actual_hash.finish(), expected_hash.finish());
        assert_eq!(decoded.to_nbt_tag(), key.to_string().to_nbt_tag());
        checked += 1;
    }
    assert!(checked > 0);
}

struct NoRandom;

impl rand::TryRng for NoRandom {
    type Error = Infallible;
    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        panic!("constant loot sampled randomness")
    }
    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        panic!("constant loot sampled randomness")
    }
    fn try_fill_bytes(&mut self, _bytes: &mut [u8]) -> Result<(), Self::Error> {
        panic!("constant loot sampled randomness")
    }
}

#[test]
fn rooted_dirt_interaction_loot_is_constant_and_never_samples_randomness() {
    init_vanilla_registry();
    let table = REGISTRY
        .loot_tables
        .by_key(&Identifier::vanilla_static("till/rooted_dirt"))
        .expect("interaction loot table");
    let tool = ItemStack::new(&vanilla_items::WOODEN_HOE);
    let pos = BlockPos::new(8, 64, 8);
    let mut random = NoRandom;
    let mut context = LootContext::new(&mut random)
        .with_block_state(vanilla_blocks::ROOTED_DIRT.default_state())
        .with_tool(&tool)
        .with_origin(f64::from(pos.x()), f64::from(pos.y()), f64::from(pos.z()));
    let drops = table.get_random_items(&mut context);
    assert_eq!(drops.len(), 1);
    assert!(drops[0].is(&vanilla_items::HANGING_ROOTS));
    assert_eq!(drops[0].count(), 1);
}
