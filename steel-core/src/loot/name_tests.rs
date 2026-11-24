use futures::executor::block_on;
use steel_registry::{
    data_components::vanilla_components::{CUSTOM_NAME, ITEM_NAME},
    init_vanilla_registry,
    item_stack::ItemStack,
    vanilla_items,
};
use steel_utils::{
    BlockPos,
    random::{Random, legacy_random::LegacyRandom},
};
use text_components::{TextComponent, content::Content};

use super::{vanilla::fixtures, *};
use crate::world::World;

fn tables() -> LootTableBehaviorRegistry {
    init_vanilla_registry();
    let mut tables = LootTableBehaviorRegistry::new();
    fixtures::register(&mut tables);
    tables
}

fn camp_names() -> Vec<TextComponent> {
    let source: serde_json::Value = serde_json::from_str(include_str!(
        "../../../steel-utils/build_assets/builtin_datapacks/minecraft/loot_table/chests/abandoned_camp_common_chest.json"
    )).expect("extracted camp table");
    source["pools"]
        .as_array()
        .expect("pools")
        .iter()
        .flat_map(|pool| pool["entries"].as_array().expect("entries"))
        .filter(|entry| entry["name"] == "minecraft:abandoned_camp_map")
        .map(|entry| {
            let name = entry["modifier"]
                .as_array()
                .expect("modifiers")
                .iter()
                .find(|modifier| modifier["type"] == "minecraft:set_name")
                .expect("map name");
            serde_json::from_value(name["name"].clone()).expect("source text component")
        })
        .collect()
}

#[test]
fn set_name_compiles_all_camp_map_names_without_resolving_their_translations() {
    let tables = tables();
    let expected = camp_names();
    assert_ne!(expected, []);
    for resumable in [false, true] {
        let mut random = LegacyRandom::from_seed(17);
        let mut untouched = LegacyRandom::from_seed(17);
        let mut context = LootContext::new(&mut random);
        context.tables = Some(&tables);
        let items = if resumable {
            block_on(fixtures::SET_NAME_CAMP_MAPS.get_random_items_async(&mut context))
        } else {
            fixtures::SET_NAME_CAMP_MAPS.get_random_items(&mut context)
        }
        .expect("named camp maps");
        assert_eq!(items.len(), expected.len());
        for (item, name) in items.iter().zip(&expected) {
            assert_eq!(item.get(ITEM_NAME), Some(name));
            assert!(item.get(CUSTOM_NAME).is_none());
        }
        assert_eq!(random.next_i64(), untouched.next_i64());
    }
}

#[test]
fn set_name_replaces_only_the_selected_component_and_preserves_nested_text_and_conditions() {
    let tables = tables();
    for resumable in [false, true] {
        let mut random = LegacyRandom::from_seed(17);
        let mut expected_random = LegacyRandom::from_seed(17);
        let mut context = LootContext::new(&mut random);
        context.tables = Some(&tables);
        let items = if resumable {
            block_on(fixtures::SET_NAME_TARGETS.get_random_items_async(&mut context))
        } else {
            fixtures::SET_NAME_TARGETS.get_random_items(&mut context)
        }
        .expect("name targets");
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].get(CUSTOM_NAME),
            Some(&TextComponent::plain("after"))
        );
        let name = items[0].get(ITEM_NAME).expect("independent item name");
        let Content::Translate(message) = &name.content else {
            panic!("unresolved translation")
        };
        assert_eq!(message.key, "filled_map.bamboo_camp_map");
        assert_eq!(message.fallback.as_deref(), Some("Camp %s"));
        let args = message.args.as_ref().expect("translation arguments");
        assert_eq!(args[0].children.len(), 1);
        assert!(matches!(args[0].children[0].content, Content::Translate(_)));
        assert_eq!(name.children, [TextComponent::plain("suffix")]);
        expected_random.next_f32();
        assert_eq!(random.next_i64(), expected_random.next_i64());
    }
}

#[test]
fn set_name_without_a_name_preserves_the_item_and_random_state() {
    let tables = tables();
    for resumable in [false, true] {
        let mut random = LegacyRandom::from_seed(17);
        let mut untouched = LegacyRandom::from_seed(17);
        let mut context = LootContext::new(&mut random);
        context.tables = Some(&tables);
        let items = if resumable {
            block_on(fixtures::SET_NAME_ABSENT.get_random_items_async(&mut context))
        } else {
            fixtures::SET_NAME_ABSENT.get_random_items(&mut context)
        }
        .expect("absent name");
        assert_eq!(items, [ItemStack::new(&vanilla_items::MAP)]);
        assert_eq!(random.next_i64(), untouched.next_i64());
    }
}

pub(crate) async fn generate_named_map(world: &World, origin: BlockPos) -> ItemStack {
    let tables = tables();
    let mut random = LegacyRandom::from_seed(17);
    let mut context = LootContext::new(&mut random)
        .with_world(world)
        .with_origin(origin.get_center());
    context.tables = Some(&tables);
    let mut items = fixtures::SET_NAME_EXPLORATION_MAP
        .get_random_items_async(&mut context)
        .await
        .expect("named exploration map");
    assert_eq!(items.len(), 1);
    let stack = items.pop().expect("named map");
    assert_eq!(stack.get(ITEM_NAME), camp_names().first());
    stack
}
