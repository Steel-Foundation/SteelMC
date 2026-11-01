//! Conversion between [`ItemSpec`] and host item stacks, and inventory slot access.
use super::{events::parse_text, players::find};
use crate::{
    PlayerId,
    items::{DataEntry, EnchantmentLevel, Glint, ItemSpec},
};
use simdnbt::owned::{NbtCompound, NbtTag};
use stabby::{string::String as ApiString, vec::Vec as ApiVec};
use steel_core::{
    inventory::container::Container,
    player::{Player, player_inventory::Prediction},
    server::Server as CoreServer,
};
use steel_registry::{
    REGISTRY, RegistryExt,
    data_components::{
        components::{CustomData, ItemEnchantments, ItemLore},
        vanilla_components::{
            CUSTOM_DATA, CUSTOM_NAME, ENCHANTMENT_GLINT_OVERRIDE, ENCHANTMENTS, LORE, UNBREAKABLE,
        },
    },
    item_stack::ItemStack,
};
use steel_utils::Identifier;

/// Compound inside `minecraft:custom_data` holding every extension's entries.
const EXTENSION_DATA: &str = "steel_extensions";

/// Builds a host stack from a spec.
pub(crate) fn to_stack(spec: &ItemSpec) -> Result<ItemStack, String> {
    if spec.is_empty() {
        return Ok(ItemStack::empty());
    }
    let key: Identifier = spec.item.as_str().parse().map_err(str::to_owned)?;
    let item = REGISTRY
        .items
        .by_key(&key)
        .ok_or_else(|| format!("unknown item {key}"))?;
    let mut stack = ItemStack::with_count(item, spec.count.clamp(1, 99));
    if !spec.name.as_str().is_empty() {
        stack.set(CUSTOM_NAME, parse_text(spec.name.as_str()));
    }
    if !spec.lore.is_empty() {
        let lines = spec
            .lore
            .iter()
            .map(|line| parse_text(line.as_str()))
            .collect();
        stack.set(
            LORE,
            ItemLore::new(lines).map_err(|error| error.to_string())?,
        );
    }
    match spec.glint {
        Glint::Default => {}
        Glint::On => stack.set(ENCHANTMENT_GLINT_OVERRIDE, true),
        Glint::Off => stack.set(ENCHANTMENT_GLINT_OVERRIDE, false),
    }
    if spec.unbreakable {
        stack.set(UNBREAKABLE, ());
    }
    if !spec.enchantments.is_empty() {
        let mut enchantments = ItemEnchantments::empty();
        for entry in spec.enchantments.iter() {
            let key: Identifier = entry.key.as_str().parse().map_err(str::to_owned)?;
            if REGISTRY.enchantments.by_key(&key).is_none() {
                return Err(format!("unknown enchantment {key}"));
            }
            enchantments.levels.insert(key, entry.level.max(1));
        }
        stack.set(ENCHANTMENTS, enchantments);
    }
    for entry in spec.data.iter() {
        write_data(&mut stack, entry.key.as_str(), entry.value.as_str())?;
    }
    Ok(stack)
}

fn json(text: &text_components::TextComponent) -> ApiString {
    serde_json::to_string(text).unwrap_or_default().into()
}

/// Describes a host stack.
pub(crate) fn to_spec(stack: &ItemStack) -> ItemSpec {
    if stack.is_empty() {
        return ItemSpec::empty();
    }
    let mut spec = ItemSpec::new(&stack.item().key.to_string()).count(stack.count());
    if let Some(name) = stack.get(CUSTOM_NAME) {
        spec.name = json(name);
    }
    if let Some(lore) = stack.get(LORE) {
        spec.lore = lore.lines().iter().map(json).collect();
    }
    spec.glint = match stack.get(ENCHANTMENT_GLINT_OVERRIDE) {
        Some(true) => Glint::On,
        Some(false) => Glint::Off,
        None => Glint::Default,
    };
    spec.unbreakable = stack.has(UNBREAKABLE);
    if let Some(enchantments) = stack.get(ENCHANTMENTS) {
        let mut levels: Vec<_> = enchantments.levels.iter().collect();
        levels.sort_by_key(|(key, _)| key.to_string());
        spec.enchantments = levels
            .into_iter()
            .map(|(key, level)| EnchantmentLevel {
                key: key.to_string().into(),
                level: *level,
            })
            .collect();
    }
    spec.data = read_data(stack);
    spec
}

fn read_data(stack: &ItemStack) -> ApiVec<DataEntry> {
    let Some(data) = stack.get(CUSTOM_DATA) else {
        return ApiVec::new();
    };
    let Some(entries) = data.as_compound().compound(EXTENSION_DATA) else {
        return ApiVec::new();
    };
    entries
        .iter()
        .filter_map(|(key, value)| match value {
            NbtTag::String(value) => Some(DataEntry {
                key: key.to_str().as_ref().into(),
                value: value.to_str().as_ref().into(),
            }),
            _ => None,
        })
        .collect()
}

/// Sets one extension data entry, removing it for an empty value; other custom data is kept.
fn write_data(stack: &mut ItemStack, key: &str, value: &str) -> Result<(), String> {
    if !key.contains(':') {
        return Err(format!(
            "extension data key {key:?} must be namespaced (plugin:key)"
        ));
    }
    let mut root = stack
        .get(CUSTOM_DATA)
        .map_or_else(NbtCompound::new, CustomData::copy_tag);
    let mut entries = root
        .take(EXTENSION_DATA)
        .and_then(|tag| match tag {
            NbtTag::Compound(compound) => Some(compound),
            _ => None,
        })
        .unwrap_or_default();
    entries.take(key);
    if !value.is_empty() {
        entries.insert(key, value.to_owned());
    }
    if !entries.is_empty() {
        root.insert(EXTENSION_DATA, NbtTag::Compound(entries));
    }
    match CustomData::try_from_compound(root) {
        Some(data) if !data.is_empty() => stack.set(CUSTOM_DATA, data),
        _ => stack.remove(CUSTOM_DATA),
    }
    Ok(())
}

const INVENTORY_SLOTS: i32 = 41;

fn slot_index(slot: i32) -> Result<usize, String> {
    if (0..INVENTORY_SLOTS).contains(&slot) {
        usize::try_from(slot).map_err(|error| error.to_string())
    } else {
        Err(format!(
            "inventory slot {slot} is outside 0..{INVENTORY_SLOTS}"
        ))
    }
}

fn changed(player: &Player) {
    player.broadcast_inventory_changes();
}

pub(crate) fn slot(server: &CoreServer, id: PlayerId, slot: i32) -> Result<ItemSpec, String> {
    let index = slot_index(slot)?;
    let player = find(server, id)?;
    let spec = to_spec(player.inventory.lock().get_item(index));
    Ok(spec)
}

pub(crate) fn set_slot(
    server: &CoreServer,
    id: PlayerId,
    slot: i32,
    item: &ItemSpec,
) -> Result<(), String> {
    let index = slot_index(slot)?;
    let stack = to_stack(item)?;
    let player = find(server, id)?;
    player.inventory.lock().set_item(index, stack);
    changed(&player);
    Ok(())
}

pub(crate) fn held_slot(server: &CoreServer, id: PlayerId) -> Result<i32, String> {
    Ok(i32::from(
        find(server, id)?.inventory.lock().get_selected_slot(),
    ))
}

pub(crate) fn set_held_slot(server: &CoreServer, id: PlayerId, slot: i32) -> Result<(), String> {
    let slot = u8::try_from(slot)
        .ok()
        .filter(|slot| *slot < 9)
        .ok_or("hotbar slots are 0..9")?;
    let player = find(server, id)?;
    player.inventory.lock().set_selected_slot(slot);
    player.send_packet(steel_protocol::packets::game::CSetHeldSlot {
        slot: i32::from(slot),
    });
    Ok(())
}

pub(crate) fn give(server: &CoreServer, id: PlayerId, item: &ItemSpec) -> Result<i32, String> {
    let mut stack = to_stack(item)?;
    if stack.is_empty() {
        return Ok(0);
    }
    let player = find(server, id)?;
    let added = player.inventory.lock().add(&mut stack);
    let dropped = if !added || !stack.is_empty() {
        let count = stack.count();
        let _ = player.drop_item(stack, false, Prediction::ServerOnly);
        count
    } else {
        0
    };
    changed(&player);
    Ok(dropped)
}

pub(crate) fn set_item_data(
    server: &CoreServer,
    id: PlayerId,
    slot: i32,
    key: &str,
    value: &str,
) -> Result<(), String> {
    let index = slot_index(slot)?;
    let player = find(server, id)?;
    {
        let mut inventory = player.inventory.lock();
        let stack = inventory.get_item_mut(index);
        if stack.is_empty() {
            return Err("that slot is empty".into());
        }
        write_data(stack, key, value)?;
        inventory.set_changed();
    }
    changed(&player);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Text;

    #[test]
    fn specs_round_trip_through_host_stacks() {
        steel_registry::init_vanilla_registry();
        let spec = ItemSpec::new("minecraft:diamond_pickaxe")
            .name(&Text::new("Prison Pick").color("gold").to_json())
            .lore(["Jackhammer I", "Fortune II"])
            .glint(Glint::On)
            .unbreakable(true)
            .enchant("minecraft:efficiency", 5)
            .with_data("prison:jackhammer", "1")
            .with_data("prison:fortune", "2");
        let stack = to_stack(&spec).map_err(|e| e.to_string());
        let Ok(stack) = stack else {
            panic!("spec should build: {stack:?}");
        };
        let back = to_spec(&stack);
        assert_eq!(back.item, spec.item);
        assert_eq!(back.count, 1);
        assert_eq!(back.glint, Glint::On);
        assert!(back.unbreakable);
        assert_eq!(back.lore.len(), 2);
        assert!(back.name.as_str().contains("Prison Pick"));
        assert_eq!(back.enchantments[0].level, 5);
        assert_eq!(back.data("prison:jackhammer"), Some("1"));
        assert_eq!(back.data("prison:fortune"), Some("2"));
    }

    #[test]
    fn data_writes_keep_other_entries_and_reject_unnamespaced_keys() {
        steel_registry::init_vanilla_registry();
        let Ok(mut stack) = to_stack(&ItemSpec::new("minecraft:stick").with_data("a:x", "1"))
        else {
            panic!("stick should build");
        };
        assert!(write_data(&mut stack, "b:y", "2").is_ok());
        assert!(write_data(&mut stack, "a:x", "").is_ok());
        assert!(write_data(&mut stack, "nonamespace", "3").is_err());
        let back = to_spec(&stack);
        assert_eq!(back.data("b:y"), Some("2"));
        assert_eq!(back.data("a:x"), None);
    }
}
