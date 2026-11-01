use std::{borrow::Cow, error::Error, io::Cursor};

use simdnbt::{FromNbtTag, borrow::read_tag};
use steel_registry::{
    ItemStackTemplate, REGISTRY, RegistryEntry, RegistryExt,
    client_content::{client_stack, restore_client_stack},
    data_components::{
        ComponentPatchEntry, DataComponentPatch,
        components::BundleContents,
        vanilla_components::{BUNDLE_CONTENTS, CUSTOM_DATA, MAX_STACK_SIZE, PROFILE},
    },
    item_stack::ItemStack,
    items::ItemRef,
    vanilla_items,
};
use steel_utils::{
    codec::VarInt,
    serial::{ReadFrom, WriteTo},
};

pub fn check(item: ItemRef) -> Result<(), Box<dyn Error>> {
    let stack = ItemStack::with_count(item, 7);
    let projected = client_stack(&stack)?;
    assert_eq!(projected.item(), &*vanilla_items::PLAYER_HEAD);
    assert!(projected.get(PROFILE).is_some());
    assert_eq!(projected.get(MAX_STACK_SIZE), Some(&64));
    assert!(projected.get(CUSTOM_DATA).is_some());

    let mut wire = Vec::new();
    stack.write(&mut wire)?;
    let mut cursor = Cursor::new(wire.as_slice());
    assert_eq!(VarInt::read(&mut cursor)?.0, 7);
    assert_eq!(
        VarInt::read(&mut cursor)?.0,
        vanilla_items::PLAYER_HEAD.id() as i32
    );
    let client_patch = DataComponentPatch::read(&mut cursor)?;
    let client_view = ItemStack::with_count_and_patch(&vanilla_items::PLAYER_HEAD, 7, client_patch);
    assert_eq!(client_view.get(PROFILE), stack.get(PROFILE));
    assert_eq!(cursor.position() as usize, wire.len());
    let restored = ItemStack::read(&mut Cursor::new(wire.as_slice()))?;
    assert!(ItemStack::matches(&stack, &restored));

    check_creative(&stack, &projected)?;

    let mut counterfeit = projected.into_owned();
    counterfeit.item = &vanilla_items::STONE;
    assert!(restore_client_stack(counterfeit).is_err());
    let mut raw_server_id = Vec::new();
    VarInt(1).write(&mut raw_server_id)?;
    VarInt(item.id() as i32).write(&mut raw_server_id)?;
    DataComponentPatch::new().write(&mut raw_server_id)?;
    assert!(ItemStack::read(&mut Cursor::new(raw_server_id.as_slice())).is_err());
    assert!(
        restore_client_stack(ItemStack::new(&vanilla_items::PLAYER_HEAD))?
            .is(&vanilla_items::PLAYER_HEAD)
    );

    // Nested custom items affect both the component bytes and their persistent
    // hash on the client, without changing authoritative server templates.
    let mut bundle = ItemStack::new(&vanilla_items::BUNDLE);
    bundle.set(
        BUNDLE_CONTENTS,
        BundleContents::new(vec![ItemStackTemplate::from_stack(&stack)?]),
    );
    let projected_bundle = client_stack(&bundle)?;
    let contents = projected_bundle
        .get(BUNDLE_CONTENTS)
        .ok_or("missing bundle contents")?;
    assert_eq!(contents.items()[0].item(), &*vanilla_items::PLAYER_HEAD);
    assert_ne!(bundle.patch(), projected_bundle.patch());
    assert_eq!(
        bundle
            .get(BUNDLE_CONTENTS)
            .ok_or("missing contents")?
            .items()[0]
            .item(),
        item
    );
    let mut wire = Vec::new();
    bundle.write(&mut wire)?;
    assert!(ItemStack::matches(
        &bundle,
        &ItemStack::read(&mut Cursor::new(wire.as_slice()))?
    ));

    let saved = stack.to_nbt_tag_ref();
    let mut saved_bytes = Vec::new();
    saved.write(&mut saved_bytes);
    let borrowed = read_tag(&mut Cursor::new(saved_bytes.as_slice()))?;
    let loaded = ItemStack::from_nbt_tag(borrowed.as_tag()).ok_or("saved stack did not reload")?;
    assert!(ItemStack::matches(&stack, &loaded));
    assert!(
        loaded.patch().is_empty(),
        "client transport metadata must never enter saved stacks"
    );
    let vanilla = ItemStack::new(&vanilla_items::OAK_PLANKS);
    assert!(matches!(client_stack(&vanilla)?, Cow::Borrowed(_)));
    check_component_overrides(item)?;
    Ok(())
}

fn check_component_overrides(item: ItemRef) -> Result<(), Box<dyn Error>> {
    let mut altered = ItemStack::with_count(item, 3);
    altered.set(MAX_STACK_SIZE, 16);
    altered.remove(PROFILE);
    let projected = client_stack(&altered)?;
    assert_eq!(projected.get(MAX_STACK_SIZE), Some(&16));
    assert!(projected.get(PROFILE).is_none());
    let mut bytes = Vec::new();
    altered.write(&mut bytes)?;
    let restored = ItemStack::read(&mut Cursor::new(bytes.as_slice()))?;
    assert!(ItemStack::matches(&altered, &restored));
    Ok(())
}

fn check_creative(stack: &ItemStack, projected: &ItemStack) -> Result<(), Box<dyn Error>> {
    // Creative's delimited component format must restore the same server identity.
    let mut creative = Vec::new();
    VarInt(projected.count()).write(&mut creative)?;
    VarInt(projected.item().id() as i32).write(&mut creative)?;
    VarInt(projected.patch().count_set() as i32).write(&mut creative)?;
    VarInt(projected.patch().count_removed() as i32).write(&mut creative)?;
    for (key, entry) in projected.patch().iter() {
        if let ComponentPatchEntry::Set(value) = entry {
            let component = REGISTRY
                .data_components
                .by_key(key)
                .ok_or("missing component")?;
            VarInt(
                REGISTRY
                    .data_components
                    .id_from_key(key)
                    .ok_or("missing component ID")? as i32,
            )
            .write(&mut creative)?;
            let mut value_bytes = Vec::new();
            component.write_network(value, &mut value_bytes)?;
            VarInt(value_bytes.len() as i32).write(&mut creative)?;
            creative.extend(value_bytes);
        }
    }
    for key in projected.patch().iter_removed() {
        VarInt(
            REGISTRY
                .data_components
                .id_from_key(key)
                .ok_or("missing component ID")? as i32,
        )
        .write(&mut creative)?;
    }
    assert!(ItemStack::matches(
        stack,
        &ItemStack::read_untrusted(&mut Cursor::new(creative.as_slice()))?
    ));

    Ok(())
}
