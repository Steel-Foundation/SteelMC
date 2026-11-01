use std::{
    borrow::Cow,
    cell::Cell,
    io::{Cursor, Error, Result},
};

use simdnbt::owned::NbtCompound;

use crate::{
    ItemStackTemplate, REGISTRY,
    data_components::{
        ComponentData, ComponentPatchEntry, DataComponentPatch,
        components::{
            BundleContents, ChargedProjectiles, CustomData, ItemContainerContents, PotDecorations,
            SulfurCubeContent, UseRemainder,
        },
        vanilla_components::CUSTOM_DATA,
    },
    item_stack::ItemStack,
    items::ItemRef,
};

const SERVER_ITEM: &str = "steel:server_item";

pub(crate) fn has_server_item(patch: &DataComponentPatch) -> bool {
    match patch.get_entry(CUSTOM_DATA.key()) {
        Some(ComponentPatchEntry::Set(value)) => value
            .downcast_ref::<CustomData>()
            .is_some_and(|data| data.as_compound().get(SERVER_ITEM).is_some()),
        _ => false,
    }
}

const MAX_DEPTH: usize = 512;
thread_local! {
    static PROJECTION_DEPTH: Cell<usize> = const { Cell::new(0) };
}

struct DepthGuard;
impl DepthGuard {
    fn enter() -> Result<Self> {
        PROJECTION_DEPTH.with(|depth| {
            if depth.get() >= MAX_DEPTH {
                return Err(Error::other(
                    "client item projection exceeds maximum nesting",
                ));
            }
            depth.set(depth.get() + 1);
            Ok(Self)
        })
    }
}
impl Drop for DepthGuard {
    fn drop(&mut self) {
        PROJECTION_DEPTH.with(|depth| depth.set(depth.get() - 1));
    }
}

/// Projects identity, default components, and recursive item templates together.
/// Vanilla stacks without custom descendants are borrowed without modification.
pub fn client_stack(stack: &ItemStack) -> Result<Cow<'_, ItemStack>> {
    if stack.is_empty() || !REGISTRY.client_content.has_items() {
        return Ok(Cow::Borrowed(stack));
    }
    let _depth = DepthGuard::enter()?;
    let Some((item, patch)) = project_parts(stack.item(), stack.patch())? else {
        return Ok(Cow::Borrowed(stack));
    };
    Ok(Cow::Owned(ItemStack::with_count_and_patch(
        item,
        stack.count(),
        patch,
    )))
}

/// The template codec also occurs inside recipes, particles, bundles, and containers.
pub fn client_template(template: &ItemStackTemplate) -> Result<Cow<'_, ItemStackTemplate>> {
    if !REGISTRY.client_content.has_items() {
        return Ok(Cow::Borrowed(template));
    }
    let _depth = DepthGuard::enter()?;
    let Some((item, patch)) = project_parts(template.item(), template.components())? else {
        return Ok(Cow::Borrowed(template));
    };
    Ok(Cow::Owned(ItemStackTemplate::try_with_count_and_patch(
        item,
        template.count(),
        patch,
    )?))
}

fn project_parts(
    item: ItemRef,
    patch: &DataComponentPatch,
) -> Result<Option<(ItemRef, DataComponentPatch)>> {
    let fallback = REGISTRY.client_content.item(item);
    let mut projected = if REGISTRY.client_content.is_custom_item(item) {
        let mut projected = patch.rebased(&item.components, &fallback.components);
        // The full server patch is retained for creative copies, including custom
        // data overwritten by this transport envelope. Count belongs to the outer stack.
        let mut record = NbtCompound::new();
        record.insert("id", item.key.to_string());
        record.insert("count", 1_i32);
        if !patch.is_empty() {
            record.insert("components", patch.try_to_nbt_tag_ref()?);
        }
        let mut envelope = NbtCompound::new();
        envelope.insert(SERVER_ITEM, record);
        let custom_data = CustomData::try_from_compound(envelope)
            .ok_or_else(|| Error::other("custom item transport data is not valid NBT"))?;
        projected.set(CUSTOM_DATA, custom_data);
        Some(projected)
    } else {
        None
    };

    // Process effective components, including recursive values in item defaults.
    // The envelope above is deliberately opaque; its contents remain server data.
    for (key, default) in item.components.iter() {
        let value = match patch.get_entry(key) {
            Some(ComponentPatchEntry::Set(value)) => value,
            Some(ComponentPatchEntry::Removed) => continue,
            None => default,
        };
        if let Some(value) = project_component(value)? {
            projected
                .get_or_insert_with(|| patch.clone())
                .set_component_data(key.clone(), value);
        }
    }
    for (key, entry) in patch.iter() {
        if item.components.get_raw(key).is_some() {
            continue;
        }
        if let ComponentPatchEntry::Set(value) = entry
            && let Some(value) = project_component(value)?
        {
            projected
                .get_or_insert_with(|| patch.clone())
                .set_component_data(key.clone(), value);
        }
    }
    Ok(projected.map(|patch| (fallback, patch)))
}

fn project_component(value: &ComponentData) -> Result<Option<ComponentData>> {
    if let Some(value) = value.downcast_ref::<BundleContents>() {
        return project_templates(value.items())
            .map(|items| items.map(|items| ComponentData::new(BundleContents::new(items))));
    }
    if let Some(value) = value.downcast_ref::<ChargedProjectiles>() {
        return project_templates(value.items())?
            .map(|items| ChargedProjectiles::new(items).map(ComponentData::new))
            .transpose();
    }
    if let Some(value) = value.downcast_ref::<ItemContainerContents>() {
        let mut changed = false;
        let items = value
            .items()
            .iter()
            .map(|template| {
                template
                    .as_ref()
                    .map(|template| {
                        let projected = client_template(template)?;
                        changed |= matches!(projected, Cow::Owned(_));
                        Ok(projected.into_owned())
                    })
                    .transpose()
            })
            .collect::<Result<Vec<_>>>()?;
        return if changed {
            ItemContainerContents::new(items).map(|value| Some(ComponentData::new(value)))
        } else {
            Ok(None)
        };
    }
    if let Some(value) = value.downcast_ref::<UseRemainder>() {
        return Ok(match client_template(value.convert_into())? {
            Cow::Borrowed(_) => None,
            Cow::Owned(template) => Some(ComponentData::new(UseRemainder::new(template))),
        });
    }
    if let Some(value) = value.downcast_ref::<PotDecorations>() {
        let mut changed = false;
        let mut side = |template: Option<&ItemStackTemplate>| -> Result<Option<ItemStackTemplate>> {
            template
                .map(|template| {
                    let projected = client_template(template)?;
                    changed |= matches!(projected, Cow::Owned(_));
                    Ok(projected.into_owned())
                })
                .transpose()
        };
        let result = PotDecorations::new(
            side(value.back())?,
            side(value.left())?,
            side(value.right())?,
            side(value.front())?,
        );
        return Ok(changed.then(|| ComponentData::new(result)));
    }
    if let Some(value) = value.downcast_ref::<SulfurCubeContent>() {
        return Ok(match client_template(value.absorbed_block_item_stack())? {
            Cow::Borrowed(_) => None,
            Cow::Owned(template) => Some(ComponentData::new(SulfurCubeContent::new(template))),
        });
    }
    Ok(None)
}

fn project_templates(items: &[ItemStackTemplate]) -> Result<Option<Vec<ItemStackTemplate>>> {
    let mut changed = false;
    let items = items
        .iter()
        .map(|item| {
            let projected = client_template(item)?;
            changed |= matches!(projected, Cow::Owned(_));
            Ok(projected.into_owned())
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(changed.then_some(items))
}

/// Recovers known server identities from their vanilla representation. This is
/// codec validation, not permission to change inventories; creative handlers
/// retain their normal player permission and stack validation checks.
pub fn restore_client_stack(stack: ItemStack) -> Result<ItemStack> {
    if stack.is_empty() {
        return Ok(stack);
    }
    if !REGISTRY.client_content.is_vanilla_item(stack.item()) {
        return Err(Error::other("client sent a server-only numeric item ID"));
    }
    let Some(custom_data) = stack.get(CUSTOM_DATA) else {
        return Ok(stack);
    };
    let Some(record) = custom_data.as_compound().get(SERVER_ITEM) else {
        return Ok(stack);
    };
    let Some(record) = record.compound() else {
        return Err(Error::other("invalid server item envelope"));
    };
    let mut bytes = Vec::new();
    record.write(&mut bytes);
    let borrowed = simdnbt::borrow::read_compound(&mut Cursor::new(bytes.as_slice()))
        .map_err(|error| Error::other(format!("invalid server item envelope: {error}")))?;
    let mut restored = ItemStack::from_borrowed_compound(&(&borrowed).into())
        .ok_or_else(|| Error::other("unknown or invalid server item in envelope"))?;
    if !REGISTRY.client_content.is_custom_item(restored.item())
        || REGISTRY.client_content.item(restored.item()) != stack.item()
    {
        return Err(Error::other(
            "server item envelope does not match its vanilla representation",
        ));
    }
    restored.set_count(stack.count());
    restored.validate_persistent_encoding()?;
    Ok(restored)
}
